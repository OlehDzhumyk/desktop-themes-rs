// SPDX-License-Identifier: GPL-3.0-or-later
// Screenshots feature of the bridge (docs/spec/features/screenshots.md): the four shortcuts (normal
// mode only, so the lock screen never sees them, AC-SHOT-08), the toolbar, the region selector, the
// delay countdown, the capture through GNOME Shell's screenshot machinery, the clipboard copy, the
// deskd call that saves the file, the skin's preview and the screen recording toggle. The bridge
// keeps no pixels after a capture; deskd removes the metadata and writes the file.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_, fmt} from '../i18n.js';
import {runThemes} from '../quickSettings.js';
import {errorText, sessionLocked} from '../shellTypes.js';
import {type Region, RegionOverlay, captureRegion} from '../ui/regionOverlay.js';
import {CaptureClient, captureErrorMessage} from './client.js';
import {Countdown} from './countdown.js';
import {CapturePreview} from './preview.js';
import {Recorder} from './recorder.js';
import {CaptureToolbar, type CaptureMode, DELAYS} from './toolbar.js';

const REGION_KEY = 'capture-region-shortcut';
const SCREEN_KEY = 'capture-screen-shortcut';
const TOOLBAR_KEY = 'capture-toolbar-shortcut';
const RECORD_KEY = 'capture-record-shortcut';
const SHORTCUT_KEYS = [REGION_KEY, SCREEN_KEY, TOOLBAR_KEY, RECORD_KEY] as const;
/** How long an error stays on screen. */
const ERROR_MS = 5000;

/** The PNG of the whole stage (all monitors), at physical pixels. */
function captureScreen(includePointer: boolean): Promise<Uint8Array> {
    const stream = Gio.MemoryOutputStream.new_resizable();
    const shooter = new Shell.Screenshot();
    return new Promise((resolve, reject) => {
        shooter.screenshot(includePointer, stream, (_s: Shell.Screenshot | null, res: Gio.AsyncResult) => {
            try {
                shooter.screenshot_finish(res);
                stream.close(null);
                resolve(stream.steal_as_bytes().toArray());
            } catch (e) {
                reject(e instanceof Error ? e : new Error(String(e)));
            }
        });
    });
}

/** The `delay` setting as a number of seconds (an unknown value counts as no delay). */
function delaySetting(settings: Gio.Settings): number {
    const n = Number(settings.get_string('delay'));
    return DELAYS.find(d => d === n) ?? 0;
}

export class CaptureFeature {
    private _settings: Gio.Settings;
    private _skin: string;
    private _client = new CaptureClient();
    private _preview = new CapturePreview();
    private _recorder: Recorder;
    private _toolbar: CaptureToolbar | null = null;
    private _overlay: RegionOverlay | null = null;
    private _countdown: Countdown | null = null;
    private _busy = false;
    private _bound: string[] = [];
    private _settingsIds: number[] = [];
    private _sessionId = 0;
    private _destroyed = false;

    constructor(settings: Gio.Settings, skin: string) {
        this._settings = settings;
        this._skin = skin;
        this._recorder = new Recorder({
            saved: path => {
                this._preview.toast().show(fmt(_('Recording saved: {name}'), {name: GLib.path_get_basename(path)}),
                    this._skin, ERROR_MS);
            },
            failed: message => { this._error(fmt(_('Could not record the screen: {error}'), {error: message})); },
        });
        try {
            for (const key of [...SHORTCUT_KEYS, 'enabled'])
                this._settingsIds.push(settings.connect(`changed::${key}`, () => { this._bind(); }));
            // the lock screen closes whatever is open (the shortcuts are not active there)
            this._sessionId = Main.sessionMode.connect('updated', () => {
                if (sessionLocked())
                    this._cancelInteractive();
            });
            this._bind();
        } catch (e) {
            // a half-built feature must not leave handlers or keybindings behind
            this.destroy();
            throw e;
        }
    }

    setSkin(skin: string): void {
        if (skin === this._skin)
            return;
        // an open toolbar or selector is in the old skin: close it; the next one uses the new skin
        this._cancelInteractive();
        this._preview.hideThumbnail();
        this._skin = skin;
    }

    private _unbind(): void {
        for (const key of this._bound)
            Main.wm.removeKeybinding(key);
        this._bound = [];
    }

    private _bind(): void {
        this._unbind();
        if (!this._settings.get_boolean('enabled')) {
            this._cancelInteractive();
            return;
        }
        const handlers: Record<typeof SHORTCUT_KEYS[number], () => void> = {
            [REGION_KEY]: () => { this.capture('region', delaySetting(this._settings)); },
            [SCREEN_KEY]: () => { this.capture('screen', delaySetting(this._settings)); },
            [TOOLBAR_KEY]: () => { this.toggleToolbar(); },
            [RECORD_KEY]: () => { this.toggleRecording(); },
        };
        for (const key of SHORTCUT_KEYS) {
            Main.wm.addKeybinding(key, this._settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
                Shell.ActionMode.NORMAL, handlers[key]);
            this._bound.push(key);
        }
    }

    /** Close the toolbar, the selector and the countdown; nothing is captured. */
    private _cancelInteractive(): void {
        this._toolbar?.destroy();
        this._toolbar = null;
        this._overlay?.destroy();
        this._overlay = null;
        this._countdown?.cancel();
        this._countdown = null;
    }

    /** The toolbar shortcut: opens the toolbar, or closes it (and a countdown) when one is open. */
    toggleToolbar(): void {
        if (this._destroyed)
            return;
        if (this._toolbar || this._countdown) {
            this._cancelInteractive();
            return;
        }
        if (this._busy)
            return;
        this._preview.hideAll();
        this._toolbar = new CaptureToolbar(this._skin, {
            delay: delaySetting(this._settings),
            recording: this._recorder.active,
        }, (mode, delay) => {
            this._toolbar = null;
            if (mode === 'record')
                this._recordAfter(delay);
            else
                this.capture(mode, delay);
        }, () => { this._toolbar = null; });
        if (!this._toolbar.open())
            this._toolbar = null;
    }

    toggleRecording(): void {
        if (this._destroyed)
            return;
        if (this._countdown) {
            this._cancelInteractive();
            return;
        }
        this._recorder.toggle(this._settings.get_boolean('include-pointer'));
    }

    /** Record from the toolbar: stop a running recording at once, or start after the delay. */
    private _recordAfter(delay: number): void {
        if (this._recorder.active || delay === 0) {
            this.toggleRecording();
            return;
        }
        this._busy = true;
        void this._wait(delay).then(go => {
            this._busy = false;
            if (go && !this._destroyed)
                this.toggleRecording();
        });
    }

    /** Re-read after each await: destroy() can run while a call is pending. */
    private _gone(): boolean {
        return this._destroyed;
    }

    /** A capture: a second press of a shortcut during the countdown cancels it instead. */
    capture(mode: Exclude<CaptureMode, 'record'>, delay: number): void {
        if (this._destroyed || !this._settings.get_boolean('enabled'))
            return;
        if (this._countdown) {
            this._cancelInteractive();
            return;
        }
        if (this._busy || this._toolbar)
            return;
        this._busy = true;
        void this._run(mode, delay).finally(() => { this._busy = false; });
    }

    private async _run(mode: Exclude<CaptureMode, 'record'>, delay: number): Promise<void> {
        // an earlier preview must not end up in the picture
        this._preview.hideAll();
        let region: Region | null = null;
        if (mode === 'region') {
            region = await this._select();
            if (!region || this._gone())
                return;
        }
        if (delay > 0 && !await this._wait(delay))
            return;
        if (this._gone())
            return;
        let png: Uint8Array;
        try {
            png = region ? await captureRegion(region) : await captureScreen(this._settings.get_boolean('include-pointer'));
        } catch (e) {
            console.warn(`desk-bridge: screenshot failed: ${errorText(e)}`);
            this._error(fmt(_('Could not take the screenshot: {error}'), {error: errorText(e)}));
            return;
        }
        if (this._gone())
            return;
        // AC-SHOT-06: with copy-to-clipboard off the clipboard is not touched
        const copied = this._settings.get_boolean('copy-to-clipboard');
        if (copied)
            St.Clipboard.get_default().set_content(St.ClipboardType.CLIPBOARD, 'image/png', new GLib.Bytes(png));
        let path: string;
        try {
            path = await this._client.saveScreenshot(png);
        } catch (e) {
            const error = captureErrorMessage(e);
            console.warn(`desk-bridge: saving the screenshot failed: ${error}`);
            this._error(copied
                ? fmt(_('Could not save the screenshot: {error}. It is on the clipboard.'), {error})
                : fmt(_('Could not save the screenshot: {error}'), {error}));
            return;
        }
        if (this._gone())
            return;
        if (this._settings.get_boolean('annotate-after-capture'))
            this._openEditor(path);
        else if (this._settings.get_boolean('show-preview'))
            this._preview.showSaved(path, copied, this._skin, p => { this._openEditor(p); });
    }

    /** The region the user selects, or null when cancelled (Esc) or the grab was refused. */
    private _select(): Promise<Region | null> {
        return new Promise(resolve => {
            this._overlay = new RegionOverlay(this._skin, {
                accessibleName: _('Select the region to capture'),
                hint: _('Drag over the region to capture it. Arrow keys and Enter select with the keyboard; Esc cancels.'),
            }, region => {
                this._overlay = null;
                resolve(region);
            });
            if (!this._overlay.open()) {
                this._overlay = null;
                resolve(null);
            }
        });
    }

    /** The visible countdown; true when it ran to the end, false when it was cancelled. */
    private _wait(seconds: number): Promise<boolean> {
        return new Promise(resolve => {
            this._countdown = new Countdown(this._preview.toast(), this._skin, seconds, go => {
                this._countdown = null;
                resolve(go);
            });
            this._countdown.start();
        });
    }

    /** `themes capture-edit <path>`: the annotation editor, a separate short-lived process. */
    private _openEditor(path: string): void {
        try {
            runThemes(['capture-edit', path]);
        } catch (e) {
            this._error(fmt(_('Could not open the editor: {error}'), {error: errorText(e)}));
        }
    }

    private _error(text: string): void {
        if (!this._destroyed)
            this._preview.toast().show(text, this._skin, ERROR_MS);
    }

    destroy(): void {
        this._destroyed = true;
        this._unbind();
        for (const id of this._settingsIds)
            this._settings.disconnect(id);
        this._settingsIds = [];
        if (this._sessionId)
            Main.sessionMode.disconnect(this._sessionId);
        this._sessionId = 0;
        this._cancelInteractive();
        this._recorder.destroy();
        this._preview.destroy();
        this._client.destroy();
    }
}
