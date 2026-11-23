// SPDX-License-Identifier: GPL-3.0-or-later
// Skin-aware on-screen displays (docs/spec/features/osd.md). For the windows11 and macos skins the
// bridge takes over Main.osdWindowManager's show methods and draws its own OSD (popup.ts); for the
// default skin, or with `enabled` off, it does not touch the manager at all (AC-OSD-05).
//
// The patch is instance-level: the stock methods live on OsdWindowManager.prototype, and the
// bridge only adds own properties on the instance. Undo deletes them again (or puts back an own
// property that was there), so the stock object is restored exactly (AC-OSD-07). When another
// extension has already replaced a method (or the manager is missing), the bridge writes one
// warning and keeps the stock OSD.
import type Gio from 'gi://Gio';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {errorText} from '../shellTypes.js';
import {OsdPopup, type OsdOptions, type OsdPosition, type OsdRequest, type OsdSkin} from './popup.js';

type Skin = 'default' | OsdSkin;
type Manager = Record<string, unknown>;
type Method = (...args: unknown[]) => unknown;

/** The manager's methods the bridge wraps (show: GNOME 45 and older; showOne/showAll: 46+). */
const METHODS = ['show', 'showOne', 'showAll', 'hideAll'] as const;
type MethodName = typeof METHODS[number];

interface Saved {
    /** Own property of the instance before the patch (normally none). */
    desc: PropertyDescriptor | undefined;
    original: Method;
    wrapper: Method;
}

function toSkin(s: string): Skin {
    return s === 'macos' || s === 'windows11' ? s : 'default';
}

function manager(): Manager | null {
    const m = Main.osdWindowManager as unknown;
    return typeof m === 'object' && m !== null ? m as Manager : null;
}

function gicon(v: unknown): Gio.Icon | null {
    return typeof v === 'object' && v !== null ? v as Gio.Icon : null;
}

function num(v: unknown): number | null {
    return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

function request(monitor: unknown, icon: unknown, label: unknown, level: unknown, maxLevel: unknown): OsdRequest {
    return {
        monitor: num(monitor) ?? -1,
        icon: gicon(icon),
        label: typeof label === 'string' && label ? label : null,
        level: num(level),
        maxLevel: num(maxLevel) ?? 1,
    };
}

export class OsdFeature {
    private _skin: Skin;
    private _settingsId = 0;
    private _saved = new Map<MethodName, Saved>();
    private _popup: OsdPopup | null = null;
    private _popupKey = '';
    private _warned = false;

    constructor(private readonly _settings: Gio.Settings, skin: string) {
        this._skin = toSkin(skin);
        this._settingsId = this._settings.connect('changed', () => {
            // a change while the OSD is visible hides it; the next one uses the new settings
            this._dropPopup();
            this._sync();
        });
        try {
            this._sync();
        } catch (e) {
            // never leave a half patch or a connection behind
            this.destroy();
            throw e;
        }
    }

    setSkin(skin: string): void {
        const next = toSkin(skin);
        if (next === this._skin)
            return;
        this._skin = next;
        this._dropPopup();
        this._sync();
    }

    destroy(): void {
        if (this._settingsId)
            this._settings.disconnect(this._settingsId);
        this._settingsId = 0;
        this._unpatch();
        this._dropPopup();
    }

    private _wanted(): boolean {
        return this._skin !== 'default' && this._settings.get_boolean('enabled');
    }

    private _sync(): void {
        if (this._wanted())
            this._patch();
        else
            this._unpatch();
    }

    private _warnOnce(why: string): void {
        if (this._warned)
            return;
        this._warned = true;
        console.warn(`desk-bridge: osd: ${why}; the stock GNOME OSD stays in use`);
    }

    private _patch(): void {
        if (this._saved.size)
            return;
        const mgr = manager();
        if (!mgr) {
            this._warnOnce('Main.osdWindowManager is missing');
            return;
        }
        // the stock methods are on the prototype; an own property means someone replaced them
        const foreign = METHODS.filter(n => Object.prototype.hasOwnProperty.call(mgr, n));
        if (foreign.length) {
            this._warnOnce(`another extension replaced osdWindowManager.${foreign.join(', ')}`);
            return;
        }
        const originals = new Map<MethodName, Method>();
        for (const name of METHODS) {
            const fn = mgr[name];
            if (typeof fn === 'function')
                originals.set(name, fn as Method);
        }
        if (!originals.has('showAll') && !originals.has('show')) {
            this._warnOnce('osdWindowManager has no show method this bridge knows');
            return;
        }
        const call = (name: MethodName, args: unknown[]) => originals.get(name)?.apply(mgr, args);
        const wrappers: Record<MethodName, Method> = {
            show: (...a) => { this._show(request(a[0], a[1], a[2], a[3], a[4]), () => call('show', a)); },
            showOne: (...a) => { this._show(request(a[0], a[1], a[2], a[3], a[4]), () => call('showOne', a)); },
            showAll: (...a) => { this._show(request(-1, a[0], a[1], a[2], a[3]), () => call('showAll', a)); },
            hideAll: (...a) => {
                this._popup?.hide(true);
                return call('hideAll', a);
            },
        };
        for (const [name, original] of originals) {
            const wrapper = wrappers[name];
            this._saved.set(name, {desc: Object.getOwnPropertyDescriptor(mgr, name), original, wrapper});
            Object.defineProperty(mgr, name, {value: wrapper, writable: true, configurable: true, enumerable: false});
        }
        // a stock OSD on screen right now goes away; the next trigger draws ours
        call('hideAll', []);
    }

    private _unpatch(): void {
        if (!this._saved.size)
            return;
        const mgr = manager();
        for (const [name, s] of this._saved) {
            if (!mgr)
                break;
            if (mgr[name] !== s.wrapper) {
                // replaced on top of ours: removing it would break the other extension
                console.warn(`desk-bridge: osd: osdWindowManager.${name} was replaced after the bridge; left as is`);
                continue;
            }
            if (s.desc)
                Object.defineProperty(mgr, name, s.desc);
            else
                Reflect.deleteProperty(mgr, name);
        }
        this._saved.clear();
        this._popup?.hide(true);
    }

    private _options(): OsdOptions {
        const p = this._settings.get_string('position');
        let position: OsdPosition;
        if (p === 'bottom-center' || p === 'center' || p === 'top-right')
            position = p;
        else
            position = this._skin === 'windows11' ? 'bottom-center' : 'center';
        return {
            position,
            showPercentage: this._settings.get_boolean('show-percentage'),
            timeoutMs: Math.min(4000, Math.max(500, this._settings.get_uint('timeout-ms'))),
        };
    }

    private _show(req: OsdRequest, stock: () => unknown): void {
        if (this._skin === 'default') {
            stock();
            return;
        }
        try {
            const opts = this._options();
            const square = this._skin === 'macos' && opts.position === 'center';
            const key = `${this._skin}:${String(square)}`;
            if (this._popup && this._popupKey !== key)
                this._dropPopup();
            if (!this._popup) {
                this._popup = new OsdPopup(this._skin, square);
                this._popupKey = key;
            }
            this._popup.show(req, opts);
        } catch (e) {
            // the user still gets feedback for the key press
            console.warn(`desk-bridge: osd: ${errorText(e)}`);
            this._dropPopup();
            stock();
        }
    }

    private _dropPopup(): void {
        this._popup?.destroy();
        this._popup = null;
        this._popupKey = '';
    }
}
