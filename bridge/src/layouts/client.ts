// SPDX-License-Identifier: GPL-3.0-or-later
// Client of deskd's window layouts engine (io.github.OlehDzhumyk.Desk.Layouts,
// data/dbus/io.github.OlehDzhumyk.Desk.Layouts.xml). Calls go only to a verified deskd owner
// (trust.ts, ADR 0008). deskd keeps the layouts and the active layout of each monitor and
// resolves zones to pixels (gaps applied); the bridge moves the windows.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {DESK_BUS, LAYOUTS_IFACE, LAYOUTS_PATH} from '../dbus.js';
import {dbusCall, ownerOf, peerOf, policyFor, untrustedReason} from '../trust.js';

/** A rectangle in logical pixels (a resolved zone, a work area). */
export interface Rect {
    x: number;
    y: number;
    w: number;
    h: number;
}

/** A layout as ListLayouts returns it; zones are fractions 0..1 of a landscape work area. */
export interface LayoutDef {
    id: string;
    name: string;
    builtin: boolean;
    zones: Rect[];
}

function num(v: unknown): number | null {
    return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

/** A {x, y, w, h} object, or null when a field is missing or not a number. */
function toRect(v: unknown): Rect | null {
    if (typeof v !== 'object' || v === null)
        return null;
    const o = v as Record<string, unknown>;
    const x = num(o['x']), y = num(o['y']), w = num(o['w']), h = num(o['h']);
    return x === null || y === null || w === null || h === null ? null : {x, y, w, h};
}

/** The layouts of a ListLayouts reply; malformed items are dropped (deskd is trusted, not infallible). */
function parseLayouts(json: string): LayoutDef[] {
    const data: unknown = JSON.parse(json);
    if (!Array.isArray(data))
        return [];
    const out: LayoutDef[] = [];
    for (const item of data as unknown[]) {
        if (typeof item !== 'object' || item === null)
            continue;
        const o = item as Record<string, unknown>;
        const id = o['id'], name = o['name'], zones = o['zones'];
        if (typeof id !== 'string' || typeof name !== 'string' || !Array.isArray(zones))
            continue;
        const rects = (zones as unknown[]).map(toRect);
        if (rects.length === 0 || rects.some(r => r === null))
            continue;
        out.push({id, name, builtin: o['builtin'] === true, zones: rects as Rect[]});
    }
    return out;
}

/** The zones of a ResolveZones reply, in the order of the layout's zones. */
function parseZones(json: string): Rect[] {
    const data: unknown = JSON.parse(json);
    if (!Array.isArray(data))
        throw new Error('ResolveZones did not return a list');
    return (data as unknown[]).map(v => {
        const r = toRect(v);
        if (!r)
            throw new Error('ResolveZones returned a malformed zone');
        return r;
    });
}

export class LayoutsClient {
    private _trusted: string | null = null;
    private _refused: string | null = null;
    private _destroyed = false;
    private _signalId = 0;

    /** `onChanged` runs when deskd says the layouts, an active layout or the gaps changed. */
    constructor(onChanged: () => void) {
        // a hint to read again; the data itself always comes from the verified owner
        this._signalId = Gio.DBus.session.signal_subscribe(DESK_BUS, LAYOUTS_IFACE, 'Changed', LAYOUTS_PATH,
            null, Gio.DBusSignalFlags.NONE, () => {
                if (!this._destroyed)
                    onChanged();
            });
    }

    private async _owner(): Promise<string> {
        if (this._destroyed)
            throw new Error('the deskd layouts client is destroyed');
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
                `(${why}); window layouts do not use it`);
        }
        throw new Error(`${DESK_BUS} is not owned by the installed deskd`);
    }

    private async _call(method: string, params: GLib.Variant | null, replyType: string): Promise<GLib.Variant> {
        const owner = await this._owner();
        return dbusCall(owner, LAYOUTS_PATH, LAYOUTS_IFACE, method, params ?? new GLib.Variant('()', []), replyType,
            Gio.DBusCallFlags.NO_AUTO_START);
    }

    async listLayouts(): Promise<LayoutDef[]> {
        const reply = await this._call('ListLayouts', null, '(s)');
        return parseLayouts(reply.deepUnpack<[string]>()[0]);
    }

    async activeLayout(monitorKey: string): Promise<string> {
        const reply = await this._call('ActiveLayout', new GLib.Variant('(s)', [monitorKey]), '(s)');
        return reply.deepUnpack<[string]>()[0];
    }

    async setActiveLayout(monitorKey: string, layoutId: string): Promise<void> {
        await this._call('SetActiveLayout', new GLib.Variant('(ss)', [monitorKey, layoutId]), '()');
    }

    async resolveZones(layoutId: string, area: Rect): Promise<Rect[]> {
        const reply = await this._call('ResolveZones',
            new GLib.Variant('(siiii)', [layoutId, area.x, area.y, area.w, area.h]), '(s)');
        return parseZones(reply.deepUnpack<[string]>()[0]);
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
