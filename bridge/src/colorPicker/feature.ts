// SPDX-License-Identifier: GPL-3.0-or-later
// Colour picker feature of the bridge (docs/spec/features/color-picker.md): the global shortcut,
// the deskd client, the eyedropper overlay, the history panel and the «Copied …» toast. deskd
// formats the colours and keeps the history; the bridge reads the pixels and draws.
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_, fmt} from '../i18n.js';
import {errorText} from '../shellTypes.js';
import {DeskToast} from '../ui/toast.js';
import {ColorPickerClient, asFormat} from './client.js';
import {PickerOverlay, type PickerHost, toHex} from './overlay.js';
import {HistoryPanel, type HistoryHost} from './panel.js';

const SHORTCUT_KEY = 'color-picker-shortcut';

export class ColorPickerFeature implements PickerHost, HistoryHost {
    readonly client: ColorPickerClient;
    readonly settings: Gio.Settings;
    private _overlay: PickerOverlay;
    private _panel: HistoryPanel | null = null;
    private _toast: DeskToast;
    private _skin = '';
    private _bound = false;
    private _settingsIds: number[] = [];
    private _destroyed = false;

    constructor(settings: Gio.Settings, skin: string) {
        this.settings = settings;
        this.client = new ColorPickerClient(() => {
            if (!this._destroyed && this._panel?.isOpen)
                void this._panel.refresh();
        });
        this._overlay = new PickerOverlay(this);
        this._toast = new DeskToast({variant: 'desk-toast-pill', fade: true, bottom: 0.82});
        this._settingsIds.push(settings.connect(`changed::${SHORTCUT_KEY}`, () => { this._bind(); }));
        this._settingsIds.push(settings.connect('changed::enabled', () => {
            this._bind();
            if (!this._enabled) {
                this._overlay.close();
                this._panel?.close();
            }
        }));
        this.setSkin(skin);
        this._bind();
    }

    private get _enabled(): boolean {
        return this.settings.get_boolean('enabled');
    }

    setSkin(skin: string): void {
        if (skin === this._skin && this._panel)
            return;
        this._skin = skin;
        this._overlay.setSkin(skin);
        this._panel?.destroy();
        this._panel = new HistoryPanel(this, skin);
    }

    /** The shortcut: start picking, or stop when the picker or the history is open. */
    toggle(): void {
        if (this._destroyed || !this._enabled)
            return;
        if (this._overlay.isOpen) {
            this._overlay.close();
            return;
        }
        if (this._panel?.isOpen) {
            this._panel.close();
            return;
        }
        this.startPicking();
    }

    startPicking(): void {
        if (!this._destroyed && this._enabled)
            this._overlay.open();
    }

    openHistory(): void {
        if (!this._destroyed && this._enabled)
            this._panel?.open();
    }

    picked(r: number, g: number, b: number): void {
        const format = asFormat(this.settings.get_string('format'));
        // Record() puts the pick into the history; Format() with the format shown in the loupe
        // gives the text to copy, so the clipboard holds what the user saw.
        Promise.all([this.client.record(r, g, b), this.client.format(r, g, b, format)])
            .then(([, text]) => { this._deliver(text); })
            .catch((e: unknown) => {
                // deskd is not there: the pick is not remembered, #RRGGBB is still copied
                console.warn(`desk-bridge: colour picker Record: ${errorText(e)}`);
                this._deliver(toHex(r, g, b));
            });
    }

    private _deliver(text: string): void {
        if (this._destroyed)
            return;
        if (this.settings.get_boolean('copy-on-pick')) {
            this.copy(text);
        } else {
            this._toast.show(fmt(_('Picked {value}'), {value: text}), this._skin);
        }
    }

    copy(text: string): void {
        if (this._destroyed)
            return;
        St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, text);
        this._toast.show(fmt(_('Copied {value}'), {value: text}), this._skin);
    }

    private _bind(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        if (!this._enabled)
            return;
        // NORMAL only: the lock screen, the login screen and modal dialogs never start it (AC-COLOR-07)
        Main.wm.addKeybinding(SHORTCUT_KEY, this.settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL, () => { this.toggle(); });
        this._bound = true;
    }

    destroy(): void {
        this._destroyed = true;
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        for (const id of this._settingsIds)
            this.settings.disconnect(id);
        this._settingsIds = [];
        this._overlay.destroy();
        this._panel?.destroy();
        this._panel = null;
        this._toast.destroy();
        this.client.destroy();
    }
}
