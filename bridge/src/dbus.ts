// SPDX-License-Identifier: GPL-3.0-or-later
// Clients of deskd (io.github.OlehDzhumyk.Desk) and of the theme service (io.github.OlehDzhumyk.Themes).
// Interface XML comes from data/dbus/ of the repository (copied into the extension's dbus/ at build
// time), so deskd and the bridge share one definition.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {errorText} from './shellTypes.js';

export const DESK_BUS = 'io.github.OlehDzhumyk.Desk';
export const CLIPBOARD_PATH = '/io/github/OlehDzhumyk/Desk/Clipboard';
export const MIXER_PATH = '/io/github/OlehDzhumyk/Desk/Mixer';
export const THEMES_BUS = 'io.github.OlehDzhumyk.Themes';
export const THEMES_PATH = '/io/github/OlehDzhumyk/Themes';

/** An item of the clipboard history, as deskd's List() returns it: (tssssbxt). */
export interface ClipItem {
    id: number;
    kind: 'text' | 'image';
    text: string;
    mime: string;
    image: string;
    pinned: boolean;
    time: number;
    size: number;
}

type Tuple = [number, string, string, string, string, boolean, number, number];

function toItem(t: Tuple): ClipItem {
    const [id, kind, text, mime, image, pinned, time, size] = t;
    return {id, kind: kind === 'image' ? 'image' : 'text', text, mime, image, pinned, time, size};
}

export function readXml(dir: string, name: string): string {
    const [ok, bytes] = GLib.file_get_contents(`${dir}/dbus/${name}.xml`);
    if (!ok)
        throw new Error(`no ${name}.xml`);
    return new TextDecoder().decode(bytes);
}

/** What GJS generates for a D-Bus interface: the typings only know the generic proxy. */
export interface Proxy {
    readonly g_name_owner: string | null;
    connectSignal(name: string, callback: () => void): number;
    disconnectSignal(id: number): void;
    connect(signal: string, callback: () => void): number;
    disconnect(id: number): void;
}

type ProxyConstructor<T> = new (
    bus: Gio.DBusConnection, name: string, path: string,
    onReady: ((proxy: T | null, error: unknown) => void) | null,
    cancellable: Gio.Cancellable | null, flags: Gio.DBusProxyFlags,
) => T;

/**
 * Proxy class for an interface XML. The one cast of this file: GJS builds a constructor function
 * at run time (`new Wrapper(...)`), the typings describe it as a plain call.
 */
export function proxyClass<T>(xml: string): ProxyConstructor<T> {
    return Gio.DBusProxy.makeProxyWrapper<T>(xml) as unknown as ProxyConstructor<T>;
}

/** Methods and properties of io.github.OlehDzhumyk.Desk.Clipboard (data/dbus/). */
interface ClipboardProxy extends Proxy {
    Private: boolean;
    readonly Enabled: boolean | null;
    PushAsync(mime: string, data: Uint8Array, offered: string[], sourceApp: string): Promise<[number]>;
    ListAsync(query: string, limit: number): Promise<[Tuple[]]>;
    ActivateAsync(id: number): Promise<[string, Uint8Array | number[]]>;
    PinAsync(id: number, pinned: boolean): Promise<unknown>;
    DeleteAsync(id: number): Promise<unknown>;
    ClearAsync(keepPinned: boolean): Promise<unknown>;
}

/** deskd's clipboard engine. Calls auto-start deskd through D-Bus activation. */
export class ClipboardClient {
    private _proxy: ClipboardProxy | null = null;
    private _signalId = 0;
    private _propsId = 0;
    private _onChanged: () => void;

    constructor(extDir: string, onChanged: () => void) {
        this._onChanged = onChanged;
        const Wrapper = proxyClass<ClipboardProxy>(readXml(extDir, 'io.github.OlehDzhumyk.Desk.Clipboard'));
        // asynchronous construction: never block the shell while deskd is being activated
        const proxy = new Wrapper(Gio.DBus.session, DESK_BUS, CLIPBOARD_PATH, (_p, error) => {
            if (error)
                console.warn(`desk-bridge: deskd proxy: ${errorText(error)}`);
            else
                this._onChanged();
        }, null, Gio.DBusProxyFlags.NONE);
        this._proxy = proxy;
        this._signalId = proxy.connectSignal('Changed', () => { this._onChanged(); });
        this._propsId = proxy.connect('g-properties-changed', () => { this._onChanged(); });
    }

    private get _p(): ClipboardProxy {
        if (!this._proxy)
            throw new Error('the deskd client is destroyed');
        return this._proxy;
    }

    get available(): boolean {
        return this._proxy?.g_name_owner != null;
    }

    get private(): boolean {
        return this._proxy?.Private ?? false;
    }

    set private(v: boolean) {
        if (this._proxy)
            this._proxy.Private = v;
    }

    get enabled(): boolean {
        return this._proxy?.Enabled ?? true;
    }

    async push(mime: string, data: Uint8Array, offered: string[], sourceApp: string): Promise<number> {
        const [id] = await this._p.PushAsync(mime, data, offered, sourceApp);
        return id;
    }

    async list(query = '', limit = 0): Promise<ClipItem[]> {
        const [items] = await this._p.ListAsync(query, limit);
        return items.map(toItem);
    }

    async activate(id: number): Promise<[string, Uint8Array]> {
        const [mime, data] = await this._p.ActivateAsync(id);
        return [mime, data instanceof Uint8Array ? data : new Uint8Array(data)];
    }

    async pin(id: number, pinned: boolean): Promise<void> {
        await this._p.PinAsync(id, pinned);
    }

    async remove(id: number): Promise<void> {
        await this._p.DeleteAsync(id);
    }

    async clear(keepPinned: boolean): Promise<void> {
        await this._p.ClearAsync(keepPinned);
    }

    destroy(): void {
        if (this._proxy) {
            if (this._signalId)
                this._proxy.disconnectSignal(this._signalId);
            if (this._propsId)
                this._proxy.disconnect(this._propsId);
        }
        this._proxy = null;
    }
}

const THEMES_XML = `<node><interface name="io.github.OlehDzhumyk.Themes">
  <method name="List"><arg type="a(sssas)" direction="out"/></method>
  <method name="Current"><arg type="a{ss}" direction="out"/></method>
  <method name="Experimental"><arg type="a{ss}" direction="out"/></method>
  <method name="Apply"><arg type="s" direction="in"/><arg type="as" direction="in"/>
    <arg type="b" direction="out"/><arg type="s" direction="out"/><arg type="as" direction="out"/></method>
  <signal name="Changed"><arg type="a{ss}"/></signal>
</interface></node>`;

export type ThemeRow = [string, string, string, string[]];

/** Methods of io.github.OlehDzhumyk.Themes (THEMES_XML above). */
interface ThemesProxy extends Proxy {
    ListAsync(): Promise<[ThemeRow[]]>;
    CurrentAsync(): Promise<[Record<string, string>]>;
    ExperimentalAsync(): Promise<[Record<string, string>]>;
    ApplyAsync(id: string, modules: string[]): Promise<[boolean, string, string[]]>;
}

/** The theme manager service (themes --dbus-service, activated on demand, exits when idle). */
export class ThemesClient {
    private _proxy: ThemesProxy;
    private _signalId: number;

    constructor(onChanged: () => void) {
        const Wrapper = proxyClass<ThemesProxy>(THEMES_XML);
        this._proxy = new Wrapper(Gio.DBus.session, THEMES_BUS, THEMES_PATH, null, null,
            Gio.DBusProxyFlags.DO_NOT_LOAD_PROPERTIES | Gio.DBusProxyFlags.DO_NOT_AUTO_START_AT_CONSTRUCTION);
        this._signalId = this._proxy.connectSignal('Changed', () => { onChanged(); });
    }

    async listAndCurrent(): Promise<[ThemeRow[], Record<string, string>]> {
        const [[themes], [current]] = await Promise.all([this._proxy.ListAsync(), this._proxy.CurrentAsync()]);
        return [themes, current];
    }

    /** Experimental themes: id → warning text (needs confirmation before Apply). */
    async experimental(): Promise<Record<string, string>> {
        try {
            const [map] = await this._proxy.ExperimentalAsync();
            return map;
        } catch {
            return {}; // an older service without the method
        }
    }

    async apply(id: string): Promise<[boolean, string, string[]]> {
        const [relogin, message, warnings] = await this._proxy.ApplyAsync(id, []);
        return [relogin, message, warnings];
    }

    destroy(): void {
        if (this._signalId)
            this._proxy.disconnectSignal(this._signalId);
        this._signalId = 0;
    }
}
