// SPDX-License-Identifier: GPL-3.0-or-later
// Clipboard capture on GNOME Wayland. Mutter offers no wlr-data-control, so only the shell can see
// the clipboard: Meta.Selection "owner-changed" tells us a new owner appeared and St.Clipboard reads
// its content, which is then handed to deskd (Push). deskd decides what to keep.
//
// Adapted from Clipboard Indicator (clipboard-indicator@tudmotu.com, extension.js:
// _setupSelectionTracking, _refreshIndicator, #getClipboardContent), Copyright (c) 2014 Yotam Bar-On
// and contributors, MIT licence (https://github.com/Tudmotu/gnome-shell-extension-clipboard-indicator).
// Changes: TypeScript, mime list passed to deskd for password-manager hints, storage moved to deskd.
import Meta from 'gi://Meta';
import St from 'gi://St';
import type Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {type ClipboardClient, INTERNAL_MIME} from '../dbus.js';
import {errorText, focusWindow} from '../shellTypes.js';

const CLIPBOARD = St.ClipboardType.CLIPBOARD;
const TEXT_TYPES = ['text/plain;charset=utf-8', 'UTF8_STRING', 'text/plain', 'STRING'];
/** Raster types only: SVG would be rendered by the shell's image loader (S-9); deskd also checks
 * the bytes. */
const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/jpg', 'image/gif', 'image/webp'];
/** Password managers mark secrets with these types; they are never even read. Same list as
 * SECRET_HINTS in crates/deskd/src/clipboard.rs. */
const SECRET_HINTS = ['x-kde-passwordManagerHint', 'application/x-nspasteboard-concealed-type',
    'org.nspasteboard.ConcealedType', 'org.nspasteboard.TransientType',
    'ExcludeClipboardContentFromMonitorProcessing'].map(m => m.toLowerCase());

function getContent(type: string): Promise<Uint8Array | null> {
    return new Promise(resolve => {
        St.Clipboard.get_default().get_content(CLIPBOARD, type, (_c: St.Clipboard, bytes: GLib.Bytes | null) => {
            if (!bytes || bytes.get_size() === 0) {
                resolve(null);
                return;
            }
            resolve(bytes.get_data() ?? null);
        });
    });
}

export class ClipboardWatcher {
    private _selection: Meta.Selection;
    private _ownerId = 0;
    private _busy = false;
    private _again = false;
    private _destroyed = false;
    /** The next clipboard owner change is our own paste (Paster.setClipboard). */
    private _ownPending = false;
    private _own = false;

    constructor(private _client: ClipboardClient, private _settings: Gio.Settings) {
        this._selection = global.display.get_selection();
        this._ownerId = this._selection.connect('owner-changed',
            (_s: Meta.Selection, type: Meta.SelectionType) => {
                if (type !== Meta.SelectionType.SELECTION_CLIPBOARD)
                    return;
                if (this._ownPending) {
                    this._ownPending = false;
                    this._own = true;
                } else {
                    this._own = false;
                }
                void this._refresh();
            });
    }

    /** Called right before the bridge sets the clipboard itself. */
    expectOwnContent(): void {
        this._ownPending = true;
    }

    /** One read at a time; a change during a read triggers one more read afterwards. */
    private async _refresh(): Promise<void> {
        if (this._busy) {
            this._again = true;
            return;
        }
        this._busy = true;
        try {
            do {
                this._again = false;
                await this._capture();
            } while (this._repeat());
        } catch (e) {
            console.warn(`desk-bridge: clipboard capture failed: ${errorText(e)}`);
        } finally {
            this._busy = false;
        }
    }

    /** Another read is wanted (read through a method: TypeScript cannot see the change across the await). */
    private _repeat(): boolean {
        return this._again && !this._destroyed;
    }

    private async _capture(): Promise<void> {
        if (this._destroyed || !this._settings.get_boolean('enabled') ||
            this._settings.get_boolean('private-mode') || this._client.private)
            return;
        const offered: string[] = this._selection.get_mimetypes(Meta.SelectionType.SELECTION_CLIPBOARD);
        if (offered.some(m => SECRET_HINTS.includes(m.toLowerCase())))
            return;
        // our own paste coming back: deskd skips it (the item was already moved up by Activate)
        if (this._own)
            offered.push(INTERNAL_MIME);
        const app = focusWindow()?.get_wm_class() ?? '';
        const types = [...TEXT_TYPES];
        if (this._settings.get_boolean('images'))
            types.push(...IMAGE_TYPES);
        for (const type of types) {
            if (offered.length && !offered.includes(type))
                continue;
            const data = await getContent(type);
            if (!data)
                continue;
            // GNOME mangles UTF8_STRING on the 2nd+ copy (gnome-shell#8233) — store as UTF-8 text
            const mime = type === 'UTF8_STRING' || type === 'STRING' || type === 'text/plain'
                ? 'text/plain;charset=utf-8' : type;
            await this._client.push(mime, data, offered, app);
            return;
        }
    }

    destroy(): void {
        this._destroyed = true;
        if (this._ownerId)
            this._selection.disconnect(this._ownerId);
        this._ownerId = 0;
    }
}
