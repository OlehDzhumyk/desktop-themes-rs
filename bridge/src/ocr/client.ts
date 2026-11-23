// SPDX-License-Identifier: GPL-3.0-or-later
// Client of deskd's OCR engine (io.github.OlehDzhumyk.Desk.Ocr). Screen pixels go only to a
// verified deskd owner (trust.ts, ADR 0008).
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {DESK_BUS, OCR_IFACE, OCR_PATH} from '../dbus.js';
import {dbusCall, ownerOf, peerOf, policyFor, untrustedReason} from '../trust.js';

/** What Recognize returns. An empty text: no text was found. */
export interface OcrResult {
    text: string;
    preview: string;
    missingLangs: string[];
}

/** Prefix of the error names of Recognize (desk_proto::ocr::ERROR_PREFIX). */
const ERROR_PREFIX = 'io.github.OlehDzhumyk.Desk.Ocr.Error.';

export type OcrErrorKind = 'Disabled' | 'InvalidImage' | 'TooLarge' | 'Busy' | 'MissingEngine' |
    'NoLanguages' | 'Timeout' | 'Failed' | 'Other';

const KINDS: readonly OcrErrorKind[] = ['Disabled', 'InvalidImage', 'TooLarge', 'Busy', 'MissingEngine',
    'NoLanguages', 'Timeout', 'Failed'];

/** Which documented OCR error a rejected call carries ('Other' for anything else). */
export function ocrErrorKind(e: unknown): OcrErrorKind {
    if (!(e instanceof GLib.Error))
        return 'Other';
    const name = Gio.DBusError.get_remote_error(e) ?? '';
    if (!name.startsWith(ERROR_PREFIX))
        return 'Other';
    const kind = name.slice(ERROR_PREFIX.length);
    return KINDS.find(k => k === kind) ?? 'Other';
}

/** The message of a D-Bus error without the GDBus.Error:<name>: prefix. */
export function ocrErrorMessage(e: unknown): string {
    if (e instanceof GLib.Error) {
        const copy = e.copy();
        Gio.DBusError.strip_remote_error(copy);
        return copy.message;
    }
    return e instanceof Error ? e.message : String(e);
}

export class OcrClient {
    private _trusted: string | null = null;
    private _refused: string | null = null;
    private _destroyed = false;

    private async _owner(): Promise<string> {
        if (this._destroyed)
            throw new Error('the deskd OCR client is destroyed');
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
                `(${why}); the text extractor refuses to send screen pixels to it`);
        }
        throw new Error(`${DESK_BUS} is not owned by the installed deskd`);
    }

    private async _call(method: string, params: GLib.Variant, replyType: string): Promise<GLib.Variant> {
        const owner = await this._owner();
        return dbusCall(owner, OCR_PATH, OCR_IFACE, method, params, replyType,
            Gio.DBusCallFlags.NO_AUTO_START);
    }

    async isAvailable(): Promise<boolean> {
        const r = await this._call('IsAvailable', new GLib.Variant('()', []), '(b)');
        return r.deepUnpack<[boolean]>()[0];
    }

    async recognize(png: Uint8Array, width: number, height: number): Promise<OcrResult> {
        const r = await this._call('Recognize', new GLib.Variant('(ayuu)', [png, width, height]), '(ssas)');
        const [text, preview, missingLangs] = r.deepUnpack<[string, string, string[]]>();
        return {text, preview, missingLangs};
    }

    destroy(): void {
        this._destroyed = true;
        this._trusted = null;
        this._refused = null;
    }
}
