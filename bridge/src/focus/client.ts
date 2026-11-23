// SPDX-License-Identifier: GPL-3.0-or-later
// Client of deskd's focus engine (io.github.OlehDzhumyk.Desk.Focus, data/dbus/). deskd owns the
// state machine and writes show-banners; the bridge only shows it and asks for changes.
import Gio from 'gi://Gio';

import {DESK_BUS, type Proxy, proxyClass, readXml} from '../dbus.js';
import {errorText} from '../shellTypes.js';

const FOCUS_PATH = '/io/github/OlehDzhumyk/Desk/Focus';

/** Status() → (ssssxxsxas). */
export interface FocusStatus {
    mode: string;
    name: string;
    icon: string;
    /** 'manual' | 'schedule' | 'session' | '' */
    source: string;
    /** Unix seconds, 0 = until turned off. */
    until: number;
    breakUntil: number;
    nextMode: string;
    nextStart: number;
    allowedApps: string[];
}

/** One entry of ListModes() (JSON). */
export interface FocusMode {
    id: string;
    label: string;
    icon: string;
    builtin: boolean;
}

type StatusTuple = [string, string, string, string, number, number, string, number, string[]];

interface FocusProxy extends Proxy {
    StatusAsync(): Promise<StatusTuple>;
    ListModesAsync(): Promise<[string]>;
    StartAsync(mode: string, until: number): Promise<unknown>;
    StartSessionAsync(minutes: number): Promise<unknown>;
    StopAsync(): Promise<unknown>;
}

export class FocusClient {
    private _proxy: FocusProxy | null;
    private _ids: number[] = [];
    private _gobjIds: number[] = [];

    constructor(extDir: string, onChanged: () => void, onSessionEnded: () => void) {
        const Wrapper = proxyClass<FocusProxy>(readXml(extDir, 'io.github.OlehDzhumyk.Desk.Focus'));
        const proxy = new Wrapper(Gio.DBus.session, DESK_BUS, FOCUS_PATH, (_p, error) => {
            if (error)
                console.warn(`desk-bridge: deskd focus proxy: ${errorText(error)}`);
            else
                onChanged();
        }, null, Gio.DBusProxyFlags.DO_NOT_LOAD_PROPERTIES);
        this._proxy = proxy;
        this._ids.push(proxy.connectSignal('Changed', () => { onChanged(); }));
        this._ids.push(proxy.connectSignal('SessionEnded', () => { onSessionEnded(); }));
        this._gobjIds.push(proxy.connect('notify::g-name-owner', () => { onChanged(); }));
    }

    private get _p(): FocusProxy {
        if (!this._proxy)
            throw new Error('the deskd focus client is destroyed');
        return this._proxy;
    }

    async status(): Promise<FocusStatus> {
        const [mode, name, icon, source, until, breakUntil, nextMode, nextStart, allowedApps] =
            await this._p.StatusAsync();
        return {mode, name, icon, source, until, breakUntil, nextMode, nextStart, allowedApps};
    }

    async modes(): Promise<FocusMode[]> {
        const [json] = await this._p.ListModesAsync();
        const list = JSON.parse(json) as Partial<FocusMode>[];
        return list.map(m => ({
            id: m.id ?? '',
            label: m.label ?? m.id ?? '',
            icon: m.icon ?? '',
            builtin: m.builtin ?? false,
        }));
    }

    async start(mode: string, until: number): Promise<void> {
        await this._p.StartAsync(mode, until);
    }

    async startSession(minutes: number): Promise<void> {
        await this._p.StartSessionAsync(minutes);
    }

    async stop(): Promise<void> {
        await this._p.StopAsync();
    }

    destroy(): void {
        const p = this._proxy;
        if (p) {
            this._ids.forEach(id => { p.disconnectSignal(id); });
            this._gobjIds.forEach(id => { p.disconnect(id); });
        }
        this._ids = [];
        this._gobjIds = [];
        this._proxy = null;
    }
}
