// SPDX-License-Identifier: GPL-3.0-or-later
// Insert a picked emoji or symbol into the focused window without clobbering the user's clipboard
// or recording the emoji in clipboard history (AC-EMO-04, AC-EMO-10).
import GLib from 'gi://GLib';
import St from 'gi://St';

import {Paster} from '../clipboard/paste.js';

const CLIPBOARD = St.ClipboardType.CLIPBOARD;
const PASTE_DELAY_MS = 120;
const RESTORE_DELAY_MS = 320;

export class EmojiInserter {
    private _paster: Paster;
    private _timeouts = new Set<number>();
    private _destroyed = false;

    constructor(private _expectOwnContent: () => void) {
        this._paster = new Paster();
    }

    private _later(ms: number, f: () => void): void {
        const id = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            this._timeouts.delete(id);
            if (!this._destroyed)
                f();
            return GLib.SOURCE_REMOVE;
        });
        this._timeouts.add(id);
    }

    /**
     * Save current clipboard text, place `glyph` on the clipboard marked internal so deskd skips
     * recording it, synthesize Shift+Insert (or Ctrl+Shift+Insert in terminals), then restore the
     * previous clipboard text.
     */
    insert(glyph: string): void {
        if (this._destroyed || !glyph)
            return;
        const cb = St.Clipboard.get_default();
        cb.get_text(CLIPBOARD, (_c: St.Clipboard, prevText: string | null) => {
            if (this._destroyed)
                return;
            const saved = prevText;
            this._expectOwnContent();
            this._paster.paste('text/plain;charset=utf-8', new TextEncoder().encode(glyph), true, PASTE_DELAY_MS);
            if (saved !== null && saved.length > 0) {
                this._later(PASTE_DELAY_MS + RESTORE_DELAY_MS, () => {
                    this._expectOwnContent();
                    St.Clipboard.get_default().set_text(CLIPBOARD, saved);
                });
            }
        });
    }

    destroy(): void {
        this._destroyed = true;
        for (const id of this._timeouts)
            GLib.source_remove(id);
        this._timeouts.clear();
        this._paster.destroy();
    }
}
