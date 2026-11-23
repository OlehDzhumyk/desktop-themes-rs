// SPDX-License-Identifier: GPL-3.0-or-later
// OCR text extractor feature of the bridge (docs/spec/features/ocr.md): the global shortcut
// (normal mode only, so the lock screen never sees it), the availability check, the region
// overlay, the capture, the deskd call and the clipboard. deskd runs Tesseract; the bridge never
// keeps the pixels.
import type Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import type {ClipboardFeature} from '../clipboard/feature.js';
import {_, fmt} from '../i18n.js';
import {errorText} from '../shellTypes.js';
import {type Region, RegionOverlay, captureRegion, pngSize} from '../ui/regionOverlay.js';
import {DeskToast} from '../ui/toast.js';
import {OcrClient, type OcrResult, ocrErrorKind, ocrErrorMessage} from './client.js';

const SHORTCUT_KEY = 'ocr-shortcut';
/** How long a result stays on screen. */
const RESULT_MS = 4000;

/** The toast text for a result: what was copied, or that nothing was found, and missing data. */
function resultMessage(r: OcrResult): string {
    const lines = [r.text ? fmt(_('Copied text: {preview}'), {preview: r.preview}) : _('No text found')];
    for (const code of r.missingLangs)
        lines.push(fmt(_('No data for language {code}: install tesseract-langpack-{code}'), {code}));
    return lines.join('\n');
}

function missingEngine(): string {
    return _('Text recognition needs tesseract: install the tesseract package');
}

export class OcrFeature {
    private _settings: Gio.Settings;
    private _clipboard: ClipboardFeature | null;
    private _client = new OcrClient();
    private _skin: string;
    private _overlay: RegionOverlay | null = null;
    private _toast: DeskToast | null = null;
    private _running = false;
    private _bound = false;
    private _settingsIds: number[] = [];
    private _destroyed = false;

    constructor(settings: Gio.Settings, skin: string, clipboard: ClipboardFeature | null) {
        this._settings = settings;
        this._clipboard = clipboard;
        this._skin = skin;
        try {
            this._settingsIds.push(settings.connect(`changed::${SHORTCUT_KEY}`, () => { this._bind(); }));
            this._settingsIds.push(settings.connect('changed::enabled', () => { this._bind(); }));
            this._bind();
        } catch (e) {
            // a half-built feature must not leave handlers or a keybinding behind
            this.destroy();
            throw e;
        }
    }

    setSkin(skin: string): void {
        if (skin === this._skin)
            return;
        // the next run builds the overlay and styles the toast in the new skin
        this._skin = skin;
    }

    private _bind(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        if (!this._settings.get_boolean('enabled'))
            return;
        Main.wm.addKeybinding(SHORTCUT_KEY, this._settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL, () => { this.start(); });
        this._bound = true;
    }

    /** Re-read after each await: destroy() can run while a call is pending. */
    private _gone(): boolean {
        return this._destroyed;
    }

    private _toastWidget(): DeskToast {
        this._toast ??= new DeskToast();
        return this._toast;
    }

    private _showResult(text: string): void {
        this._toastWidget().show(text, this._skin, RESULT_MS);
    }

    /** The shortcut: a press while a run is going is ignored (one recognition at a time). */
    start(): void {
        if (this._running || this._destroyed || !this._settings.get_boolean('enabled'))
            return;
        this._running = true;
        void this._run().finally(() => { this._running = false; });
    }

    private async _run(): Promise<void> {
        let available: boolean;
        try {
            available = await this._client.isAvailable();
        } catch (e) {
            this._report(e);
            return;
        }
        if (this._gone())
            return;
        if (!available) {
            // AC-OCR-03: say so at once, and no overlay
            this._showResult(missingEngine());
            return;
        }
        // an earlier result must not end up in the screenshot
        this._toast?.hide();
        const region = await this._select();
        if (!region || this._gone())
            return;
        try {
            const png = await captureRegion(region);
            if (this._gone())
                return;
            const size = pngSize(png);
            if (!size)
                throw new Error('the screenshot is not a PNG image');
            this._toastWidget().busy(_('Recognising text…'), this._skin);
            const result = await this._client.recognize(png, size[0], size[1]);
            if (this._gone())
                return;
            if (result.text)
                this._copy(result.text);
            this._showResult(resultMessage(result));
        } catch (e) {
            this._report(e);
        }
    }

    /** The region the user selects, or null when cancelled (Esc) or the grab was refused. */
    private _select(): Promise<Region | null> {
        return new Promise(resolve => {
            this._overlay = new RegionOverlay(this._skin, {
                accessibleName: _('Select the text to copy'),
                hint: _('Drag over the text to copy it. Arrow keys and Enter select with the keyboard; Esc cancels.'),
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

    /** AC-OCR-05: with record-in-history off the clipboard history skips this content. */
    private _copy(text: string): void {
        if (!this._settings.get_boolean('record-in-history'))
            this._clipboard?.expectOwnContent();
        St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, text);
    }

    private _report(e: unknown): void {
        if (this._destroyed)
            return;
        let text: string;
        switch (ocrErrorKind(e)) {
        case 'Busy':
            return;
        case 'MissingEngine':
            text = missingEngine();
            break;
        case 'Timeout':
            text = _('Recognition was interrupted');
            break;
        case 'TooLarge':
            text = _('The region is too large to recognise');
            break;
        case 'NoLanguages':
            text = fmt(_('No language data for text recognition: {error}'), {error: ocrErrorMessage(e)});
            break;
        default:
            console.warn(`desk-bridge: text recognition failed: ${errorText(e)}`);
            text = fmt(_('Could not recognise text: {error}'), {error: ocrErrorMessage(e)});
        }
        this._showResult(text);
    }

    destroy(): void {
        this._destroyed = true;
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        for (const id of this._settingsIds)
            this._settings.disconnect(id);
        this._settingsIds = [];
        this._overlay?.destroy();
        this._overlay = null;
        this._toast?.destroy();
        this._toast = null;
        this._clipboard = null;
        this._client.destroy();
    }
}
