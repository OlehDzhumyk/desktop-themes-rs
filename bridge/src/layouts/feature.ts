// SPDX-License-Identifier: GPL-3.0-or-later
// Window layouts feature of the bridge (docs/spec/features/window-layouts.md): the layout picker
// shortcut and the always-on-top shortcut (normal mode only, so the lock screen never sees them),
// snapping the focused window into a zone that deskd resolves (gaps applied), snap assist and the
// pin toggle. deskd keeps the layouts and the active layout of each monitor; the bridge moves and
// resizes the windows. Only normal, managed windows are touched: never dialogs, tool windows or
// override-redirect surfaces (AC-WIN-05).
import type Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_} from '../i18n.js';
import {errorText, focusWindow, sessionLocked} from '../shellTypes.js';
import {DeskToast} from '../ui/toast.js';
import {type EmptyZone, SnapAssist} from './assist.js';
import {type LayoutDef, LayoutsClient, type Rect} from './client.js';
import {LayoutPicker} from './picker.js';
import {ZonePreview} from './preview.js';

const PICKER_KEY = 'layout-picker-shortcut';
const ON_TOP_KEY = 'always-on-top-shortcut';
const SHORTCUT_KEYS = [PICKER_KEY, ON_TOP_KEY] as const;
/** deskd's limit for a monitor key, in bytes. */
const MAX_MONITOR_KEY = 256;
/** A window counts as filling a zone when its frame is this close (pixels) on every side. */
const ZONE_SLACK = 4;

/** A window the layouts may move: a normal, managed window (AC-WIN-05). */
function snappable(win: Meta.Window | null): win is Meta.Window {
    return win !== null && win.get_window_type() === Meta.WindowType.NORMAL && !win.is_override_redirect() &&
        !win.is_skip_taskbar();
}

/** The key deskd stores the active layout under: connector and product of the monitor. */
function monitorKey(index: number): string {
    try {
        const manager = global.backend.get_monitor_manager();
        const logical = manager.get_logical_monitors()?.find(l => l.get_number() === index);
        const monitor = logical?.get_monitors()[0];
        if (monitor) {
            const key = `${monitor.get_connector()}:${monitor.get_product()}`.replace(/\p{Cc}/gu, '');
            if (key.length > 0)
                return key.slice(0, MAX_MONITOR_KEY);
        }
    } catch (e) {
        console.warn(`desk-bridge: cannot read the monitor's connector: ${errorText(e)}`);
    }
    return `monitor-${String(index)}`;
}

/** The work area of the window's monitor on its workspace. */
function workArea(win: Meta.Window): Rect {
    const r = win.get_work_area_for_monitor(win.get_monitor());
    return {x: r.x, y: r.y, w: r.width, h: r.height};
}

function fills(win: Meta.Window, zone: Rect): boolean {
    const f = win.get_frame_rect();
    return Math.abs(f.x - zone.x) <= ZONE_SLACK && Math.abs(f.y - zone.y) <= ZONE_SLACK &&
        Math.abs(f.width - zone.w) <= ZONE_SLACK && Math.abs(f.height - zone.h) <= ZONE_SLACK;
}

/** Move and resize `win` into `zone`; a window that cannot be resized is centred at its own size. */
function place(win: Meta.Window, zone: Rect): void {
    if (win.is_fullscreen())
        win.unmake_fullscreen();
    if (win.get_maximize_flags() !== Meta.MaximizeFlags.NONE)
        win.unmaximize();
    if (win.minimized)
        win.unminimize();
    if (win.allows_resize()) {
        win.move_resize_frame(true, zone.x, zone.y, zone.w, zone.h);
    } else {
        const f = win.get_frame_rect();
        win.move_frame(true, Math.round(zone.x + (zone.w - f.width) / 2), Math.round(zone.y + (zone.h - f.height) / 2));
    }
}

/** What an interaction (picker, then snap assist) acts on, fixed when it starts. */
interface Target {
    win: Meta.Window;
    area: Rect;
    monitor: string;
}

export class LayoutsFeature {
    private _settings: Gio.Settings;
    private _skin: string;
    private _client: LayoutsClient;
    private _toast = new DeskToast();
    private _layouts: LayoutDef[] | null = null;
    /** Resolved zones by `layout@x,y,w,h`; deskd's Changed empties it. */
    private _zones = new Map<string, Rect[]>();
    private _picker: LayoutPicker | null = null;
    private _assist: SnapAssist | null = null;
    private _preview: ZonePreview | null = null;
    private _target: Target | null = null;
    private _targetId = 0;
    /** Increases with every new interaction, so a stale asynchronous answer is ignored. */
    private _generation = 0;
    private _bound: string[] = [];
    private _settingsIds: number[] = [];
    private _monitorsId = 0;
    private _sessionId = 0;
    private _destroyed = false;

    constructor(settings: Gio.Settings, skin: string) {
        this._settings = settings;
        this._skin = skin;
        this._client = new LayoutsClient(() => { this._invalidate(); });
        try {
            for (const key of [...SHORTCUT_KEYS, 'enabled'])
                this._settingsIds.push(settings.connect(`changed::${key}`, () => { this._bind(); }));
            for (const key of ['gap-outer', 'gap-inner'])
                this._settingsIds.push(settings.connect(`changed::${key}`, () => { this._invalidate(); }));
            // AC-WIN-04: a monitor came or went: the open picker and preview belong to the old
            // arrangement; close them (Mutter keeps the windows on the remaining monitors)
            this._monitorsId = global.backend.get_monitor_manager().connect('monitors-changed', () => {
                this._zones.clear();
                this._endInteraction();
            });
            this._sessionId = Main.sessionMode.connect('updated', () => {
                if (sessionLocked())
                    this._endInteraction();
            });
            this._bind();
        } catch (e) {
            // a half-built feature must not leave handlers or keybindings behind
            this.destroy();
            throw e;
        }
    }

    setSkin(skin: string): void {
        if (skin === this._skin)
            return;
        // an open picker is in the old skin: close it; the next one uses the new skin
        this._endInteraction();
        this._toast.hide();
        this._skin = skin;
    }

    private _invalidate(): void {
        this._layouts = null;
        this._zones.clear();
    }

    private _unbind(): void {
        for (const key of this._bound)
            Main.wm.removeKeybinding(key);
        this._bound = [];
    }

    private _bind(): void {
        this._unbind();
        if (!this._settings.get_boolean('enabled')) {
            this._endInteraction();
            return;
        }
        const handlers: Record<typeof SHORTCUT_KEYS[number], () => void> = {
            [PICKER_KEY]: () => { this.togglePicker(); },
            [ON_TOP_KEY]: () => { this.toggleAlwaysOnTop(); },
        };
        for (const key of SHORTCUT_KEYS) {
            Main.wm.addKeybinding(key, this._settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
                Shell.ActionMode.NORMAL, handlers[key]);
            this._bound.push(key);
        }
    }

    /** AC-WIN-03: pin the focused window above others, or unpin it. */
    toggleAlwaysOnTop(): void {
        const win = focusWindow();
        if (this._destroyed || !snappable(win))
            return;
        if (win.is_above()) {
            win.unmake_above();
            this._toast.show(_('Unpinned'), this._skin);
        } else {
            win.make_above();
            this._toast.show(_('Pinned on top'), this._skin);
        }
    }

    /** The picker shortcut: opens the picker for the focused window, or closes an open one. */
    togglePicker(): void {
        if (this._destroyed)
            return;
        if (this._picker || this._assist) {
            this._endInteraction();
            return;
        }
        const win = focusWindow();
        if (!snappable(win))
            return;
        const pending = this._openPicker(win);
        // _openPicker starts its interaction before its first await
        const generation = this._generation;
        pending.catch((e: unknown) => {
            console.warn(`desk-bridge: the layout picker failed: ${errorText(e)}`);
            if (generation === this._generation)
                this._endInteraction();
        });
    }

    private async _openPicker(win: Meta.Window): Promise<void> {
        const generation = this._begin(win);
        const target = this._target;
        if (!target)
            return;
        this._layouts ??= await this._client.listLayouts();
        const layouts = this._layouts;
        let active = 'halves';
        try {
            active = await this._client.activeLayout(target.monitor);
        } catch (e) {
            console.warn(`desk-bridge: cannot read the active layout: ${errorText(e)}`);
        }
        if (generation !== this._generation)
            return;
        this._ensurePreview();
        this._picker = new LayoutPicker(this._skin, layouts, active, target.area, {
            hover: (layoutId, zone) => { this._hover(generation, layoutId, zone); },
            choose: (layoutId, zone) => {
                this._picker = null;
                this._preview?.hide();
                void this._snap(generation, layoutId, zone, active).catch((e: unknown) => {
                    console.warn(`desk-bridge: snapping the window failed: ${errorText(e)}`);
                    if (generation === this._generation)
                        this._endInteraction();
                });
            },
            close: () => {
                this._picker = null;
                this._endInteraction();
            },
        });
        if (!this._picker.open()) {
            this._picker = null;
            this._endInteraction();
        }
    }

    /** Start an interaction for `win`; any earlier one ends. Returns its generation. */
    private _begin(win: Meta.Window): number {
        this._endInteraction();
        const generation = ++this._generation;
        this._target = {win, area: workArea(win), monitor: monitorKey(win.get_monitor())};
        // the window closes meanwhile: nothing is left to place
        this._targetId = win.connect('unmanaging', () => { this._endInteraction(); });
        return generation;
    }

    /** Close the picker, snap assist and the preview; forget the target window. */
    private _endInteraction(): void {
        this._generation++;
        this._picker?.destroy();
        this._picker = null;
        this._assist?.destroy();
        this._assist = null;
        this._preview?.destroy();
        this._preview = null;
        if (this._target && this._targetId)
            this._target.win.disconnect(this._targetId);
        this._targetId = 0;
        this._target = null;
    }

    /** The preview exists before the picker's or the assist's layer, so it stays under them. */
    private _ensurePreview(): ZonePreview {
        this._preview ??= new ZonePreview(this._skin);
        return this._preview;
    }

    private async _resolve(layoutId: string, area: Rect): Promise<Rect[]> {
        const key = `${layoutId}@${String(area.x)},${String(area.y)},${String(area.w)},${String(area.h)}`;
        const cached = this._zones.get(key);
        if (cached)
            return cached;
        const zones = await this._client.resolveZones(layoutId, area);
        this._zones.set(key, zones);
        return zones;
    }

    private _hover(generation: number, layoutId: string, zone: number | null): void {
        const target = this._target;
        if (zone === null || !target) {
            this._preview?.hide();
            return;
        }
        this._resolve(layoutId, target.area).then(zones => {
            const rect = zones[zone];
            if (generation === this._generation && this._picker && rect)
                this._preview?.show(rect);
        }).catch((e: unknown) => {
            console.warn(`desk-bridge: cannot resolve the zones: ${errorText(e)}`);
        });
    }

    /** AC-WIN-01: place the target window into zone `zone` of `layoutId`, then offer snap assist. */
    private async _snap(generation: number, layoutId: string, zone: number, active: string): Promise<void> {
        const target = this._target;
        if (!target)
            return;
        const zones = await this._resolve(layoutId, target.area);
        const rect = zones[zone];
        if (generation !== this._generation || !rect)
            return;
        place(target.win, rect);
        if (layoutId !== active) {
            this._client.setActiveLayout(target.monitor, layoutId).catch((e: unknown) => {
                console.warn(`desk-bridge: cannot save the active layout: ${errorText(e)}`);
            });
        }
        if (!this._offerAssist(generation, target, zones, zone))
            this._endInteraction();
    }

    /** AC-WIN-02: offer the other windows for the empty zones. True when the assist opened. */
    private _offerAssist(generation: number, target: Target, zones: Rect[], placed: number): boolean {
        if (!this._settings.get_boolean('snap-assist') || zones.length < 2)
            return false;
        const workspace = global.workspace_manager.get_active_workspace();
        const others = global.display.get_tab_list(Meta.TabList.NORMAL, workspace)
            .filter(w => w !== target.win && snappable(w));
        const empty: EmptyZone[] = [];
        zones.forEach((rect, i) => {
            if (i !== placed && !others.some(w => fills(w, rect)))
                empty.push({rect, number: i + 1, count: zones.length});
        });
        const candidates = others.filter(w => !zones.some(rect => fills(w, rect)));
        if (empty.length === 0 || candidates.length === 0)
            return false;
        this._assist = new SnapAssist(this._skin, target.area, empty, candidates, this._ensurePreview(), {
            place: (win, z) => {
                if (generation === this._generation)
                    place(win, z.rect);
            },
            close: () => {
                this._assist = null;
                this._endInteraction();
            },
        });
        if (!this._assist.open()) {
            this._assist = null;
            return false;
        }
        return true;
    }

    destroy(): void {
        this._destroyed = true;
        this._unbind();
        for (const id of this._settingsIds)
            this._settings.disconnect(id);
        this._settingsIds = [];
        if (this._monitorsId)
            global.backend.get_monitor_manager().disconnect(this._monitorsId);
        this._monitorsId = 0;
        if (this._sessionId)
            Main.sessionMode.disconnect(this._sessionId);
        this._sessionId = 0;
        this._endInteraction();
        this._toast.destroy();
        this._client.destroy();
    }
}
