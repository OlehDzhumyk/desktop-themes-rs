// SPDX-License-Identifier: GPL-3.0-or-later
// Client of deskd's capture engine (io.github.OlehDzhumyk.Desk.Capture). Screen pixels go only to a
// verified deskd owner (trust.ts, ADR 0008). deskd removes the PNG's metadata and writes the file
// atomically; it keeps no image between calls.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {CAPTURE_IFACE, CAPTURE_PATH, DESK_BUS} from '../dbus.js';
import {dbusCall, ownerOf, peerOf, policyFor, untrustedReason} from '../trust.js';

/** The message of a D-Bus error without the GDBus.Error:<name>: prefix. */
export function captureErrorMessage(e: unknown): string {
    if (e instanceof GLib.Error) {
        const copy = e.copy();
        Gio.DBusError.strip_remote_error(copy);
        return copy.message;
    }
    return e instanceof Error ? e.message : String(e);
}

export class CaptureClient {
    private _trusted: string | null = null;
    private _refused: string | null = null;
    private _destroyed = false;

    private async _owner(): Promise<string> {
        if (this._destroyed)
            throw new Error('the deskd capture client is destroyed');
        const owner = await ownerOf(DESK_BUS);
        if (owner === this._trusted)
            return owner;
        const policy = policyFor(DESK_BUS);
        const why = policy ? untrustedReason(await peerOf(owner), policy) : 'cannot read the shell\'s own process data';
        if (why === null) {
            this._trusted = owner;
            return owner;
        }
        if (this._refused !== owner) {
            this._refused = owner;
            console.warn(`desk-bridge: ${DESK_BUS} is owned by ${owner}, which is not the installed deskd ` +
                `(${why}); screenshots are not sent to it`);
        }
        throw new Error(`${DESK_BUS} is not owned by the installed deskd`);
    }

    /** Save the PNG in the save folder; returns the full path of the new file. */
    async saveScreenshot(png: Uint8Array): Promise<string> {
        const owner = await this._owner();
        const reply = await dbusCall(owner, CAPTURE_PATH, CAPTURE_IFACE, 'SaveScreenshot',
            new GLib.Variant('(ay)', [png]), '(s)', Gio.DBusCallFlags.NO_AUTO_START);
        return reply.deepUnpack<[string]>()[0];
    }

    destroy(): void {
        this._destroyed = true;
        this._trusted = null;
        this._refused = null;
    }
}
