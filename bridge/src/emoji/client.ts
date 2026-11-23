// SPDX-License-Identifier: GPL-3.0-or-later
// Client of deskd's emoji and symbol picker engine (io.github.OlehDzhumyk.Desk.Emoji).
// Calls go only to a verified deskd owner (trust.ts, ADR 0008).
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {DESK_BUS, EMOJI_IFACE, EMOJI_PATH} from '../dbus.js';
import {dbusCall, ownerOf, peerOf, policyFor, untrustedReason} from '../trust.js';

/** One emoji, Unicode symbol or text face returned by deskd: (sssssb). */
export interface EmojiItem {
    id: string;
    glyph: string;
    name: string;
    category: string;
    keywords: string;
    supportsTone: boolean;
}

type EmojiTuple = [string, string, string, string, string, boolean];

function toEmojiItem([id, glyph, name, category, keywords, supportsTone]: EmojiTuple): EmojiItem {
    return {id, glyph, name, category, keywords, supportsTone};
}

export class EmojiClient {
    private _trusted: string | null = null;
    private _refused: string | null = null;
    private _destroyed = false;

    private async _owner(): Promise<string> {
        if (this._destroyed)
            throw new Error('the deskd emoji client is destroyed');
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
                `(${why}); the emoji picker refuses to talk to it`);
        }
        throw new Error(`${DESK_BUS} is not owned by the installed deskd`);
    }

    private async _call(method: string, params: GLib.Variant, replyType: string): Promise<GLib.Variant> {
        const owner = await this._owner();
        return dbusCall(owner, EMOJI_PATH, EMOJI_IFACE, method, params, replyType,
            Gio.DBusCallFlags.NO_AUTO_START);
    }

    async search(query: string, lang: string, limit = 120): Promise<EmojiItem[]> {
        const r = await this._call('Search', new GLib.Variant('(ssu)', [query, lang, limit]), '(a(sssssb))');
        return r.deepUnpack<[EmojiTuple[]]>()[0].map(toEmojiItem);
    }

    async category(category: string, limit = 0): Promise<EmojiItem[]> {
        const r = await this._call('Category', new GLib.Variant('(su)', [category, limit]), '(a(sssssb))');
        return r.deepUnpack<[EmojiTuple[]]>()[0].map(toEmojiItem);
    }

    async recents(): Promise<EmojiItem[]> {
        const r = await this._call('Recents', new GLib.Variant('()', []), '(a(sssssb))');
        return r.deepUnpack<[EmojiTuple[]]>()[0].map(toEmojiItem);
    }

    async pick(id: string, tone: string): Promise<string> {
        const r = await this._call('Pick', new GLib.Variant('(ss)', [id, tone]), '(s)');
        return r.deepUnpack<[string]>()[0];
    }

    async clearRecents(): Promise<void> {
        await this._call('ClearRecents', new GLib.Variant('()', []), '()');
    }

    destroy(): void {
        this._destroyed = true;
        this._trusted = null;
        this._refused = null;
    }
}
