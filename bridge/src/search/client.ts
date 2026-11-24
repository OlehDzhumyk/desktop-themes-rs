// SPDX-License-Identifier: GPL-3.0-or-later
// Client of deskd's search engine (io.github.OlehDzhumyk.Desk.Search,
// data/dbus/io.github.OlehDzhumyk.Desk.Search.xml). Calls go only to a verified deskd owner
// (trust.ts, ADR 0008). deskd parses and ranks the query; the bridge draws the box and performs
// the chosen result.
//
// QueryScheduler sits between the search field and Query: it waits a few milliseconds while the
// user types and numbers every request, so a slow reply to an old text never replaces the
// results of a newer one.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {DESK_BUS, SEARCH_IFACE, SEARCH_PATH} from '../dbus.js';
import {errorText} from '../shellTypes.js';
import {dbusCall, ownerOf, peerOf, policyFor, untrustedReason} from '../trust.js';

/** The kinds of result desk-search returns (ResultKind, kebab-case). */
export type SearchKind = 'calculator' | 'app' | 'action' | 'file' | 'clipboard' | 'emoji' | 'web';

const KINDS: readonly SearchKind[] = ['calculator', 'app', 'action', 'file', 'clipboard', 'emoji', 'web'];

/** One result of Query (desk_search::SearchResult). */
export interface SearchItem {
    /** Stable id, also the usage key: `app:<desktop id>`, `action:<id>`, `calc`, `web`, ... */
    id: string;
    kind: SearchKind;
    title: string;
    subtitle: string;
    /** Theme icon name or absolute path; empty for none. */
    icon: string;
    /** The desktop id (app), the action id (action), the text to copy (calculator, emoji) or the
     * query for the browser (web). */
    activate: string;
    score: number;
}

const str = (v: unknown): string | null => typeof v === 'string' ? v : null;

/** The results of a Query reply; malformed items and unknown kinds are dropped. */
function parseResults(json: string): SearchItem[] {
    const data: unknown = JSON.parse(json);
    if (!Array.isArray(data))
        return [];
    const out: SearchItem[] = [];
    for (const item of data as unknown[]) {
        if (typeof item !== 'object' || item === null)
            continue;
        const o = item as Record<string, unknown>;
        const id = str(o['id']), kind = str(o['kind']), title = str(o['title']), activate = str(o['activate']);
        if (id === null || kind === null || title === null || activate === null)
            continue;
        if (!KINDS.includes(kind as SearchKind))
            continue;
        const score = o['score'];
        out.push({
            id, kind: kind as SearchKind, title, activate,
            subtitle: str(o['subtitle']) ?? '',
            icon: str(o['icon']) ?? '',
            score: typeof score === 'number' && Number.isFinite(score) ? score : 0,
        });
    }
    return out;
}

/** True for the error of a cancelled call (destroy, or a newer query). */
function isCancelled(e: unknown): boolean {
    return e instanceof GLib.Error && e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED);
}

export class SearchClient {
    private _trusted: string | null = null;
    private _refused: string | null = null;
    private _destroyed = false;
    /** Cancels every call still in flight when the client is destroyed. */
    private _cancellable = new Gio.Cancellable();

    private async _owner(): Promise<string> {
        if (this._destroyed)
            throw new Error('the deskd search client is destroyed');
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
                `(${why}); the search box sends no query to it`);
        }
        throw new Error(`${DESK_BUS} is not owned by the installed deskd`);
    }

    private async _call(method: string, params: GLib.Variant, replyType: string): Promise<GLib.Variant> {
        const owner = await this._owner();
        return dbusCall(owner, SEARCH_PATH, SEARCH_IFACE, method, params, replyType,
            Gio.DBusCallFlags.NO_AUTO_START, this._cancellable);
    }

    /** Results for the text of the field; `limit` 0 uses the max-results setting. */
    async query(text: string, limit = 0): Promise<SearchItem[]> {
        const reply = await this._call('Query', new GLib.Variant('(su)', [text, limit]), '(s)');
        return parseResults(reply.deepUnpack<[string]>()[0]);
    }

    async recordLaunch(itemId: string): Promise<void> {
        await this._call('RecordLaunch', new GLib.Variant('(s)', [itemId]), '()');
    }

    async clearUsage(): Promise<void> {
        await this._call('ClearUsage', new GLib.Variant('()', []), '()');
    }

    destroy(): void {
        this._destroyed = true;
        this._cancellable.cancel();
        this._trusted = null;
        this._refused = null;
    }
}

/** How long typing pauses before a query goes out (the spec allows 50 ms to the first results). */
const DEBOUNCE_MS = 25;

/**
 * Debounced, numbered queries. `onResults` runs only for the newest request; replies to older
 * texts and replies that arrive after `cancel` or `destroy` are dropped.
 */
export class QueryScheduler {
    private _seq = 0;
    private _timerId = 0;
    private _destroyed = false;

    constructor(
        private _client: SearchClient,
        private _onResults: (text: string, items: SearchItem[]) => void,
    ) {}

    /** Ask for `text`; an empty text goes out at once (it lists the most used items). */
    request(text: string, limit = 0): void {
        if (this._destroyed)
            return;
        this._clearTimer();
        const seq = ++this._seq;
        if (text.length === 0) {
            this._send(seq, text, limit);
            return;
        }
        this._timerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, DEBOUNCE_MS, () => {
            this._timerId = 0;
            this._send(seq, text, limit);
            return GLib.SOURCE_REMOVE;
        });
    }

    private _send(seq: number, text: string, limit: number): void {
        this._client.query(text, limit)
            .then(items => {
                if (!this._destroyed && seq === this._seq)
                    this._onResults(text, items);
            })
            .catch((e: unknown) => {
                if (this._destroyed || isCancelled(e))
                    return;
                console.warn(`desk-bridge: search query failed: ${errorText(e)}`);
                if (seq === this._seq)
                    this._onResults(text, []);
            });
    }

    private _clearTimer(): void {
        if (this._timerId)
            GLib.source_remove(this._timerId);
        this._timerId = 0;
    }

    /** Forget the pending request; a reply still in flight is dropped. */
    cancel(): void {
        this._clearTimer();
        this._seq++;
    }

    destroy(): void {
        this.cancel();
        this._destroyed = true;
    }
}
