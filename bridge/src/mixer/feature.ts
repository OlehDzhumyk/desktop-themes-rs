// SPDX-License-Identifier: GPL-3.0-or-later
// Volume mixer of the bridge (docs/spec/features/mixer.md). It lives in the menu of GNOME's own
// output slider in quick settings (the chevron next to the volume slider), so it follows the
// shell theme, light/dark and the accent colour like the rest of quick settings. Skins:
//   default    GNOME's device chooser stays; an “Applications” group with one slider per app.
//   windows11  “Volume mixer” sub-panel: Output, Volume mixer (apps), Input (Win+Ctrl+V).
//   macos      “Sound” page: Output and Input with check marks, a collapsed “Apps” disclosure
//              shown only while something plays (macOS has no per-app volume; adapted).
// deskd owns the audio graph (io.github.OlehDzhumyk.Desk.Mixer); nothing here talks to PipeWire.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as BoxPointer from 'resource:///org/gnome/shell/ui/boxpointer.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import type {OutputStreamSlider} from 'resource:///org/gnome/shell/ui/status/volume.js';

import {animationsEnabled} from '../clipboard/panel.js';
import {_} from '../i18n.js';
import {errorText, outputSlider} from '../shellTypes.js';
import {MixerClient, type MixerApp, type MixerDevice} from './client.js';
import {AppRow, type RowHost, deviceItem, heading, note} from './rows.js';

const SHORTCUT_KEY = 'mixer-shortcut';
const SOUND_SCHEMA = 'org.gnome.desktop.sound';

type Skin = 'default' | 'macos' | 'windows11';

/** Device names as Apple would show them: without the technical profile suffix. */
function plainName(description: string): string {
    return description
        .replace(/\s+(Analog|Digital)\s+(Stereo|Mono|Surround\s+[\d.]+)(\s+\(.*\))?$/i, '')
        .replace(/\s+(Stereo|Mono)$/i, '')
        .trim() || description;
}

function logError(e: unknown): void {
    console.warn(`desk-bridge: mixer: ${errorText(e)}`);
}

/** Sends at most one volume request at a time per target, always the latest value. */
class Latest {
    private _pending = new Map<string, () => Promise<void>>();
    private _busy = new Set<string>();

    send(key: string, call: () => Promise<void>): void {
        this._pending.set(key, call);
        if (!this._busy.has(key))
            this._next(key);
    }

    private _next(key: string): void {
        const call = this._pending.get(key);
        if (!call) {
            this._busy.delete(key);
            return;
        }
        this._pending.delete(key);
        this._busy.add(key);
        call().catch(logError).finally(() => { this._next(key); });
    }
}

export class MixerFeature implements RowHost {
    readonly client: MixerClient;
    private _settings: Gio.Settings;
    private _sound: Gio.Settings | null;
    private _skin: Skin;
    private _slider: OutputStreamSlider | null = null;
    private _savedHeader: {icon: Gio.Icon | null; title: string; subtitle: string} | null = null;
    private _root: PopupMenu.PopupMenuSection | null = null;
    private _outputs = new PopupMenu.PopupMenuSection();
    private _inputs = new PopupMenu.PopupMenuSection();
    private _apps = new PopupMenu.PopupMenuSection();
    private _appsMenu: PopupMenu.PopupSubMenuMenuItem | null = null;
    private _status: PopupMenu.PopupMenuItem | null = null;
    private _rows = new Map<string, AppRow>();
    private _outputList: MixerDevice[] = [];
    private _serial = 0;
    private _latest = new Latest();
    private _ids: [object, number][] = [];
    private _bound = false;

    constructor(extDir: string, settings: Gio.Settings, skin: string, private _openPrefs: () => void) {
        this._settings = settings;
        this._skin = toSkin(skin);
        this._sound = Gio.SettingsSchemaSource.get_default()?.lookup(SOUND_SCHEMA, true)
            ? new Gio.Settings({schema_id: SOUND_SCHEMA}) : null;
        this.client = new MixerClient(extDir, () => { this._changed(); });
        this._connect(settings, 'changed', (_s: unknown, key: string) => {
            if (key === 'enabled') {
                this._detach();
                this._attach();
                this._bind();
            } else if (key === SHORTCUT_KEY) {
                this._bind();
            } else {
                this._changed();
            }
        });
        if (this._sound)
            this._connect(this._sound, 'changed::allow-volume-above-100-percent', () => { this._changed(); });
        // the output slider is created asynchronously by quick settings: attach on first open
        const qs = Main.panel.statusArea.quickSettings;
        if (qs) {
            this._connect(qs.menu, 'open-state-changed', (_m: unknown, open: boolean) => {
                if (open)
                    this._attach();
            });
            // entry point "menu-and-icon": a secondary click on the system area opens the mixer
            this._connect(qs, 'event', (_a: unknown, ev: Clutter.Event) => {
                if (ev.type() === Clutter.EventType.BUTTON_PRESS && ev.get_button() === Clutter.BUTTON_SECONDARY &&
                    this._settings.get_string('entry-point') === 'menu-and-icon' && this._enabled)
                    idle(() => { this.open(false); });
                return Clutter.EVENT_PROPAGATE;
            });
        }
        this._attach();
        this._bind();
    }

    private _connect(obj: object, signal: string, cb: (...args: never[]) => unknown): void {
        const o = obj as {connect(s: string, cb: (...args: never[]) => unknown): number};
        this._ids.push([obj, o.connect(signal, cb)]);
    }

    private get _enabled(): boolean {
        return this._settings.get_boolean('enabled');
    }

    // ------------------------------------------------------------------ RowHost

    get maxVolume(): number {
        return this._sound?.get_boolean('allow-volume-above-100-percent') ? 1.5 : 1.0;
    }

    get step(): number {
        return Math.max(1, Math.min(20, this._settings.get_uint('volume-step'))) / 100;
    }

    setAppVolume(app: MixerApp, volume: number): void {
        this._latest.send(`app:${app.key}`, () => this.client.setAppVolume(app.key, volume));
    }

    setAppMute(app: MixerApp, mute: boolean): void {
        this.client.setAppMute(app.key, mute).catch(logError);
    }

    moveApp(app: MixerApp, device: MixerDevice): void {
        this.client.moveApp(app.key, device.id).catch((e: unknown) => {
            logError(e);
            Main.notifyError(_('Volume mixer'), _('Could not switch the device'));
        });
    }

    outputs(): MixerDevice[] {
        return this._outputList;
    }

    // ------------------------------------------------------------------ placement

    setSkin(skin: string): void {
        const s = toSkin(skin);
        if (s === this._skin)
            return;
        this._detach();
        this._skin = s;
        this._attach();
    }

    /** Put our sections into the output slider's menu (once it exists and the mixer is on). */
    private _attach(): void {
        if (this._root || !this._enabled)
            return;
        const slider = outputSlider();
        if (!slider)
            return;
        this._slider = slider;
        const menu = slider.menu;
        const root = new PopupMenu.PopupMenuSection();
        if (root.actor instanceof St.Widget)
            root.actor.add_style_class_name(`desk-mixer desk-skin-${this._skin}`);
        this._root = root;
        this._outputs = new PopupMenu.PopupMenuSection();
        this._inputs = new PopupMenu.PopupMenuSection();
        this._apps = new PopupMenu.PopupMenuSection();
        this._status = note('');
        this._status.visible = false;
        root.addMenuItem(this._status);

        if (this._skin === 'default') {
            root.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            root.addMenuItem(heading(_('Applications')));
            root.addMenuItem(this._apps);
            menu.addMenuItem(root, 1); // after GNOME's own device list
        } else {
            const title = this._skin === 'macos' ? _('Sound') : _('Volume mixer');
            this._savedHeader = {
                icon: menu._headerIcon.gicon,
                title: menu._headerTitle.text,
                subtitle: menu._headerSubtitle.text,
            };
            menu.setHeader('audio-speakers-symbolic', title, '');
            slider._deviceSection.actor.hide();
            root.addMenuItem(heading(_('Output')));
            root.addMenuItem(this._outputs);
            if (this._skin === 'windows11') {
                root.addMenuItem(heading(_('Volume mixer')));
                root.addMenuItem(this._apps);
                root.addMenuItem(heading(_('Input')));
                root.addMenuItem(this._inputs);
            } else {
                root.addMenuItem(heading(_('Input')));
                root.addMenuItem(this._inputs);
                this._appsMenu = new PopupMenu.PopupSubMenuMenuItem(_('Apps'), false);
                this._appsMenu.add_style_class_name('desk-mixer-disclosure');
                this._appsMenu.menu.addMenuItem(this._apps);
                root.addMenuItem(this._appsMenu);
                const badge = note(_('From Windows, adapted for macOS'));
                badge.add_style_class_name('desk-mixer-badge');
                this._appsMenu.menu.addMenuItem(badge);
            }
            menu.addMenuItem(root, 0);
        }
        // the chevron must be there even with one output device: the apps are behind it
        slider.menuEnabled = true;
        this._connect(slider, 'notify::menu-enabled', () => {
            if (this._root && !slider.menuEnabled)
                slider.menuEnabled = true;
        });
        this._connect(menu, 'open-state-changed', (_m: unknown, open: boolean) => {
            if (open)
                void this.refresh();
        });
        if (menu.isOpen)
            void this.refresh();
    }

    private _detach(): void {
        const slider = this._slider;
        if (!slider || !this._root)
            return;
        for (const [obj, id] of this._ids.filter(([o]) => o === slider || o === slider.menu))
            (obj as {disconnect(id: number): void}).disconnect(id);
        this._ids = this._ids.filter(([o]) => o !== slider && o !== slider.menu);
        for (const r of this._rows.values())
            r.destroy();
        this._rows.clear();
        this._root.destroy();
        this._root = null;
        this._appsMenu = null;
        this._status = null;
        slider._deviceSection.actor.show();
        if (this._savedHeader) {
            const h = this._savedHeader;
            slider.menu.setHeader(h.icon ?? 'audio-headphones-symbolic', h.title, h.subtitle);
            this._savedHeader = null;
        }
        // GNOME shows the chevron only with more than one device
        slider.menuEnabled = slider._deviceItems.size > 1;
        this._slider = null;
    }

    // ------------------------------------------------------------------ content

    private _isAttached(): boolean {
        return this._root !== null;
    }

    private _changed(): void {
        if (this._slider?.menu.isOpen)
            void this.refresh();
    }

    /** Ask deskd for the lists and show them (only while the menu is open). */
    async refresh(): Promise<void> {
        if (!this._root)
            return;
        const serial = ++this._serial;
        const silent = this._settings.get_boolean('show-silent-apps');
        let outputs: MixerDevice[] = [], inputs: MixerDevice[] = [], apps: MixerApp[] = [];
        let ok: boolean;
        try {
            [outputs, inputs, apps] = await Promise.all([
                this.client.devices('output'), this.client.devices('input'), this.client.apps(silent),
            ]);
            ok = this.client.available;
        } catch (e) {
            logError(e);
            ok = false;
        }
        if (serial !== this._serial || !this._isAttached())
            return;
        this._outputList = outputs;
        if (this._status) {
            this._status.visible = !ok;
            this._status.label.text = _('The audio server is not available');
        }
        if (this._skin !== 'default') {
            this._fillDevices(this._outputs, ok ? outputs : [], false);
            this._fillDevices(this._inputs, ok ? inputs : [], true);
        }
        this._fillApps(ok ? apps : [], ok);
    }

    private _fillDevices(section: PopupMenu.PopupMenuSection, list: MixerDevice[], input: boolean): void {
        const focused = global.stage.get_key_focus();
        let refocus: number | null = null;
        for (const it of section._getMenuItems()) {
            if (it === focused && it instanceof PopupMenu.PopupBaseMenuItem)
                refocus = (it as PopupMenu.PopupBaseMenuItem & {_deviceId?: number})._deviceId ?? null;
        }
        section.removeAll();
        const sorted = this._skin === 'macos'
            ? [...list].sort((a, b) => Number(b.bus === 'bluetooth') - Number(a.bus === 'bluetooth'))
            : list;
        for (const d of sorted) {
            const name = this._skin === 'macos' ? plainName(d.description) : d.description;
            const item = deviceItem(d, input, name, d.isDefault, () => {
                this.client.setDefault(input ? 'input' : 'output', d.id).catch(logError);
            }) as PopupMenu.PopupImageMenuItem & {_deviceId?: number};
            item._deviceId = d.id;
            section.addMenuItem(item);
            if (refocus === d.id)
                item.grab_key_focus();
        }
        section.actor.visible = sorted.length > 0;
    }

    private _fillApps(apps: MixerApp[], ok: boolean): void {
        const keys = new Set(apps.map(a => a.key));
        for (const [key, row] of this._rows) {
            if (!keys.has(key) && !row.dragging) {
                row.destroy();
                this._rows.delete(key);
            }
        }
        // the empty-state note is the section's first item when there is nothing to show
        for (const it of this._apps._getMenuItems()) {
            if (it instanceof PopupMenu.PopupMenuItem && it.has_style_class_name('desk-mixer-note'))
                it.destroy();
        }
        for (const app of apps) {
            const row = this._rows.get(app.key);
            if (row) {
                row.update(app);
            } else {
                const r = new AppRow(this, app, this._skin === 'macos');
                this._rows.set(app.key, r);
                this._apps.addMenuItem(r.item);
                this._apps.addMenuItem(r.targets);
            }
        }
        if (ok && apps.length === 0)
            this._apps.addMenuItem(note(_('No applications are playing sound')), 0);
        // macOS: the disclosure only while at least one app plays (AC-MIX-04)
        if (this._appsMenu)
            this._appsMenu.visible = apps.some(a => a.playing);
    }

    // ------------------------------------------------------------------ opening

    /** Open quick settings on the mixer (shortcut, secondary click); again: close. */
    open(toggle = true): void {
        if (!this._enabled)
            return;
        const qs = Main.panel.statusArea.quickSettings;
        if (!qs)
            return;
        const anim = animationsEnabled() ? BoxPointer.PopupAnimation.FULL : BoxPointer.PopupAnimation.NONE;
        if (toggle && qs.menu.isOpen && this._slider?.menu.isOpen) {
            qs.menu.close(anim);
            return;
        }
        if (!qs.menu.isOpen)
            qs.menu.open(anim);
        this._attach();
        const slider = this._slider;
        if (!slider)
            return;
        slider.menu.open(anim);
        void this.refresh().then(() => {
            const first = [...this._rows.values()][0];
            if (first)
                first.focus();
            else
                slider.menu.actor.navigate_focus(null, St.DirectionType.TAB_FORWARD, false);
        });
    }

    private _bind(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        if (!this._enabled)
            return;
        Main.wm.addKeybinding(SHORTCUT_KEY, this._settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW, () => { this.open(); });
        this._bound = true;
    }

    openSettings(): void {
        this._openPrefs();
    }

    destroy(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        this._detach();
        for (const [obj, id] of this._ids)
            (obj as {disconnect(id: number): void}).disconnect(id);
        this._ids = [];
        this.client.destroy();
    }
}

function toSkin(s: string): Skin {
    return s === 'macos' || s === 'windows11' ? s : 'default';
}

function idle(f: () => void): void {
    GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
        f();
        return GLib.SOURCE_REMOVE;
    });
}
