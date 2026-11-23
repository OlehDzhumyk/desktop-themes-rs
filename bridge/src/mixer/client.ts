// SPDX-License-Identifier: GPL-3.0-or-later
// Client of deskd's mixer engine (io.github.OlehDzhumyk.Desk.Mixer, data/dbus/). The bridge never
// opens an audio connection: everything it shows comes from here, every change goes back here.
import Gio from 'gi://Gio';

import {DESK_BUS, MIXER_PATH, type Proxy, proxyClass, readXml} from '../dbus.js';
import {errorText} from '../shellTypes.js';

/** Interface version this bridge speaks (the Version property of deskd). */
const MIXER_VERSION = 1;

/** An output or input device: ListDevices → (ussssdbb). */
export interface MixerDevice {
    id: number;
    name: string;
    description: string;
    icon: string;
    bus: string;
    volume: number;
    mute: boolean;
    isDefault: boolean;
}

/** One application row (its streams merged): ListApps → (ssssdbbuau). */
export interface MixerApp {
    key: string;
    name: string;
    icon: string;
    media: string;
    volume: number;
    mute: boolean;
    playing: boolean;
    device: number;
    streams: number[];
}

type DeviceTuple = [number, string, string, string, string, number, boolean, boolean];
type AppTuple = [string, string, string, string, number, boolean, boolean, number, number[]];

interface MixerProxy extends Proxy {
    readonly Version: number | null;
    readonly Available: boolean | null;
    ListDevicesAsync(kind: string): Promise<[DeviceTuple[]]>;
    ListAppsAsync(includeSilent: boolean): Promise<[AppTuple[]]>;
    SetVolumeAsync(id: number, volume: number): Promise<unknown>;
    SetMuteAsync(id: number, mute: boolean): Promise<unknown>;
    SetAppVolumeAsync(app: string, volume: number): Promise<unknown>;
    SetAppMuteAsync(app: string, mute: boolean): Promise<unknown>;
    SetDefaultAsync(kind: string, id: number): Promise<unknown>;
    MoveAppAsync(app: string, device: number): Promise<unknown>;
}

function toDevice([id, name, description, icon, bus, volume, mute, isDefault]: DeviceTuple): MixerDevice {
    return {id, name, description, icon, bus, volume, mute, isDefault};
}

function toApp([key, name, icon, media, volume, mute, playing, device, streams]: AppTuple): MixerApp {
    return {key, name, icon, media, volume, mute, playing, device, streams};
}

/** Calls never block the shell: the proxy is built asynchronously and every call is async. */
export class MixerClient {
    private _proxy: MixerProxy | null;
    private _signalId: number;
    private _propsId: number;
    private _ownerId: number;

    constructor(extDir: string, onChanged: () => void) {
        const Wrapper = proxyClass<MixerProxy>(readXml(extDir, 'io.github.OlehDzhumyk.Desk.Mixer'));
        const proxy = new Wrapper(Gio.DBus.session, DESK_BUS, MIXER_PATH, (_p, error) => {
            if (error)
                console.warn(`desk-bridge: deskd mixer proxy: ${errorText(error)}`);
            else
                onChanged();
        }, null, Gio.DBusProxyFlags.NONE);
        this._proxy = proxy;
        this._signalId = proxy.connectSignal('Changed', () => { onChanged(); });
        this._propsId = proxy.connect('g-properties-changed', () => { onChanged(); });
        this._ownerId = proxy.connect('notify::g-name-owner', () => { onChanged(); });
    }

    private get _p(): MixerProxy {
        if (!this._proxy)
            throw new Error('the deskd mixer client is destroyed');
        return this._proxy;
    }

    /** deskd runs, speaks this version, and is connected to the audio server. */
    get available(): boolean {
        const p = this._proxy;
        return p?.g_name_owner != null && p.Version === MIXER_VERSION && p.Available === true;
    }

    async devices(kind: 'output' | 'input'): Promise<MixerDevice[]> {
        const [list] = await this._p.ListDevicesAsync(kind);
        return list.map(toDevice);
    }

    async apps(includeSilent: boolean): Promise<MixerApp[]> {
        const [list] = await this._p.ListAppsAsync(includeSilent);
        return list.map(toApp);
    }

    async setVolume(id: number, volume: number): Promise<void> {
        await this._p.SetVolumeAsync(id, volume);
    }

    async setMute(id: number, mute: boolean): Promise<void> {
        await this._p.SetMuteAsync(id, mute);
    }

    async setAppVolume(app: string, volume: number): Promise<void> {
        await this._p.SetAppVolumeAsync(app, volume);
    }

    async setAppMute(app: string, mute: boolean): Promise<void> {
        await this._p.SetAppMuteAsync(app, mute);
    }

    async setDefault(kind: 'output' | 'input', id: number): Promise<void> {
        await this._p.SetDefaultAsync(kind, id);
    }

    async moveApp(app: string, device: number): Promise<void> {
        await this._p.MoveAppAsync(app, device);
    }

    destroy(): void {
        const p = this._proxy;
        if (p) {
            p.disconnectSignal(this._signalId);
            p.disconnect(this._propsId);
            p.disconnect(this._ownerId);
        }
        this._proxy = null;
    }
}
