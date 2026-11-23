// SPDX-License-Identifier: GPL-3.0-or-later
// Client of deskd's colour picker engine (io.github.OlehDzhumyk.Desk.ColorPicker,
// data/dbus/io.github.OlehDzhumyk.Desk.ColorPicker.xml). deskd formats the colours and keeps the
// history; the bridge reads the pixels and draws the loupe. Calls go only to a verified deskd
// owner (trust.ts, ADR 0008): the text deskd returns goes onto the clipboard.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {DESK_BUS} from '../dbus.js';
import {dbusCall, ownerOf, peerOf, policyFor, untrustedReason} from '../trust.js';

const COLOR_PATH = '/io/github/OlehDzhumyk/Desk/ColorPicker';
const COLOR_IFACE = 'io.github.OlehDzhumyk.Desk.ColorPicker';

/** The formats of the `format` setting, in the order the F key cycles them. */
export const FORMATS = ['hex', 'rgb', 'hsl', 'hsv', 'name', 'gtk', 'decimal'] as const;
export type ColorFormat = typeof FORMATS[number];

export function asFormat(s: string): ColorFormat {
    return FORMATS.find(f => f === s) ?? 'hex';
}

/** One pick of the history, List() → a(sssx). */
export interface ColorItem {
    /** #RRGGBB */
    hex: string;
    /** The colour in the format in force. */
    formatted: string;
    /** The nearest CSS colour name. */
    name: string;
    /** Unix seconds of the last pick. */
    pickedAt: number;
}

/** What Record() returns: (sss). */
export interface RecordResult {
    formatted: string;
    hex: string;
    name: string;
}

type ItemTuple = [string, string, string, number];

export class ColorPickerClient {
    private _trusted: string | null = null;
    private _refused: string | null = null;
    private _signalId = 0;
    private _destroyed = false;

    /** `onChanged`: deskd says the history, the format or history-size changed. */
    constructor(onChanged: () => void) {
        // the signal only asks for a new List(), which goes through the trust check
        this._signalId = Gio.DBus.session.signal_subscribe(DESK_BUS, COLOR_IFACE, 'Changed', COLOR_PATH,
            null, Gio.DBusSignalFlags.NONE, () => {
                if (!this._destroyed)
                    onChanged();
            });
    }

    private async _owner(): Promise<string> {
        if (this._destroyed)
            throw new Error('the deskd colour picker client is destroyed');
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
                `(${why}); the colour picker refuses to talk to it`);
        }
        throw new Error(`${DESK_BUS} is not owned by the installed deskd`);
    }

    private async _call(method: string, params: GLib.Variant, replyType: string): Promise<GLib.Variant> {
        const owner = await this._owner();
        return dbusCall(owner, COLOR_PATH, COLOR_IFACE, method, params, replyType,
            Gio.DBusCallFlags.NO_AUTO_START);
    }

    async record(r: number, g: number, b: number): Promise<RecordResult> {
        const v = await this._call('Record', new GLib.Variant('(yyy)', [r, g, b]), '(sss)');
        const [formatted, hex, name] = v.deepUnpack<[string, string, string]>();
        return {formatted, hex, name};
    }

    async list(): Promise<ColorItem[]> {
        const v = await this._call('List', new GLib.Variant('()', []), '(a(sssx))');
        return v.deepUnpack<[ItemTuple[]]>()[0].map(([hex, formatted, name, pickedAt]) =>
            ({hex, formatted, name, pickedAt}));
    }

    async remove(hex: string): Promise<boolean> {
        const v = await this._call('Remove', new GLib.Variant('(s)', [hex]), '(b)');
        return v.deepUnpack<[boolean]>()[0];
    }

    async clear(): Promise<void> {
        await this._call('Clear', new GLib.Variant('()', []), '()');
    }

    /** `format`: one of FORMATS, or '' for the setting in force. */
    async format(r: number, g: number, b: number, format: ColorFormat | ''): Promise<string> {
        const v = await this._call('Format', new GLib.Variant('(yyys)', [r, g, b, format]), '(s)');
        return v.deepUnpack<[string]>()[0];
    }

    /** WCAG contrast ratio (1..21) of the two newest picks; 0 with fewer than two. */
    async contrast(): Promise<number> {
        const v = await this._call('Contrast', new GLib.Variant('()', []), '(d)');
        return v.deepUnpack<[number]>()[0];
    }

    destroy(): void {
        this._destroyed = true;
        if (this._signalId)
            Gio.DBus.session.signal_unsubscribe(this._signalId);
        this._signalId = 0;
        this._trusted = null;
        this._refused = null;
    }
}
