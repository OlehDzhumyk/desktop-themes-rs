// SPDX-License-Identifier: GPL-3.0-or-later
// Focus modes in the shell (docs/spec/features/focus.md): a «Focus» quick settings tile with the
// mode list, a status icon in the panel while a mode is on, the allowed-apps exception and the
// summary of missed notifications. deskd owns the state and writes show-banners. Skins:
//   default    symbolic moon icon; tile with the modes.
//   macos      crescent icon; tile with the modes (like Control Center).
//   windows11  bell-slash icon; the tile also starts focus sessions (15/25/45/60 min) and shows
//              the countdown (the shell has no Windows-style notification flyout to host it).
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as MessageTray from 'resource:///org/gnome/shell/ui/messageTray.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {QuickMenuToggle, SystemIndicator} from 'resource:///org/gnome/shell/ui/quickSettings.js';

import {_, fmt} from '../i18n.js';
import {errorText} from '../shellTypes.js';
import {FocusClient, type FocusMode, type FocusStatus} from './client.js';

type Skin = 'default' | 'macos' | 'windows11';

const MOON = 'weather-clear-night-symbolic';
const BELL_OFF = 'notifications-disabled-symbolic';
const SESSION_MINUTES = [15, 25, 45, 60];

function toSkin(s: string): Skin {
    return s === 'macos' || s === 'windows11' ? s : 'default';
}

function logError(e: unknown): void {
    console.warn(`desk-bridge: focus: ${errorText(e)}`);
}

function hhmm(unix: number): string {
    return GLib.DateTime.new_from_unix_local(unix).format('%H:%M') ?? '';
}

function nowUnix(): number {
    return GLib.DateTime.new_now_local().to_unix();
}

/** Tomorrow 07:00 local time (an hour off on a night the clocks change: good enough here). */
function tomorrowMorning(): number {
    const n = GLib.DateTime.new_now_local();
    const midnight = n.to_unix() - (n.get_hour() * 3600 + n.get_minute() * 60 + n.get_second());
    return midnight + 24 * 3600 + 7 * 3600;
}

/** The state in words, as the CLI and the window say it (same gettext ids). */
export function statusLine(st: FocusStatus | null): string {
    if (!st)
        return _('Unavailable');
    if (st.mode) {
        if (st.until > 0 && st.source === 'session')
            return fmt(_('Focus session until {time}'), {time: hhmm(st.until)});
        if (st.until > 0)
            return fmt(_('Focus: {mode} until {time}'), {mode: st.name, time: hhmm(st.until)});
        return fmt(_('Focus: {mode}'), {mode: st.name});
    }
    if (st.breakUntil > 0)
        return fmt(_('Break until {time}'), {time: hhmm(st.breakUntil)});
    if (st.nextStart > 0)
        return fmt(_('Focus is off; {mode} starts at {time}'), {mode: st.nextMode, time: hhmm(st.nextStart)});
    return _('Focus is off');
}

// ------------------------------------------------------------------ allowed apps

/** App ids whose banners show although show-banners is false (the active mode's exceptions). */
const allowed = new Set<string>();

function canonical(id: string): string {
    return id.replace(/\.desktop$/, '').toLowerCase();
}

/**
 * Let allowed apps through: GNOME asks the notification source's policy whether to show a banner
 * (critical notifications always pass). The application policy's showBanners getter is wrapped
 * so that an allowed app answers with its own per-app setting instead of the global switch.
 * Returns the undo, or null when this shell has no such getter (then every banner stays hidden).
 */
function patchPolicy(): (() => void) | null {
    try {
        const proto = MessageTray.NotificationApplicationPolicy.prototype;
        const desc = Object.getOwnPropertyDescriptor(proto, 'showBanners');
        if (!desc?.get)
            return null;
        Object.defineProperty(proto, 'showBanners', {
            configurable: true,
            get(this: MessageTray.NotificationApplicationPolicy): boolean {
                if (allowed.size > 0 && allowed.has(canonical(this.id))) {
                    try {
                        return this._settings.get_boolean('show-banners');
                    } catch {
                        // Fall through to GNOME's original policy getter if shell internals change.
                    }
                }
                return desc.get ? Boolean(desc.get.call(this)) : true;
            },
        });
        return () => { Object.defineProperty(proto, 'showBanners', desc); };
    } catch (e) {
        logError(e);
        return null;
    }
}

/** Notifications in the list now (for the «missed» summary). */
function trayCount(): number {
    return Main.messageTray.getSources().reduce((n, s) => n + s.count, 0);
}

// ------------------------------------------------------------------ shell objects

const FocusToggle = GObject.registerClass(
class FocusToggle extends QuickMenuToggle {
    declare modes: PopupMenu.PopupMenuSection;
    declare extra: PopupMenu.PopupMenuSection;

    constructor() {
        super({title: _('Focus'), iconName: MOON, toggleMode: false});
        this.menu.setHeader(MOON, _('Focus'), '');
        this.modes = new PopupMenu.PopupMenuSection();
        this.extra = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this.modes);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this.menu.addMenuItem(this.extra);
        // the shell does not destroy an item's menu with the item (see quickSettings.ts)
        this.connect('destroy', () => { this.menu.destroy(); });
    }
});

const FocusIndicator = GObject.registerClass(
class FocusIndicator extends SystemIndicator {
    declare icon: ReturnType<SystemIndicator['_addIndicator']>;
    declare toggle: InstanceType<typeof FocusToggle>;

    constructor() {
        super();
        this.icon = this._addIndicator();
        this.icon.icon_name = MOON;
        this.icon.visible = false;
        this.toggle = new FocusToggle();
        this.quickSettingsItems.push(this.toggle);
    }

    override destroy(): void {
        this.quickSettingsItems.forEach(i => { i.destroy(); });
        super.destroy();
    }
});

// ------------------------------------------------------------------ the feature

export class FocusFeature {
    private _client: FocusClient;
    private _settings: Gio.Settings | null;
    private _skin: Skin;
    private _indicator: InstanceType<typeof FocusIndicator>;
    private _status: FocusStatus | null = null;
    private _missedBase: number | null = null;
    private _tick = 0;
    private _serial = 0;
    private _enabledId = 0;
    private _destroyed = false;
    private _unpatch: (() => void) | null;

    constructor(extDir: string, settings: Gio.Settings | null, skin: string, private _openPrefs: () => void) {
        this._settings = settings;
        this._skin = toSkin(skin);
        this._unpatch = patchPolicy();
        if (this._settings)
            this._enabledId = this._settings.connect('changed::enabled', () => { this._render(); });
        this._indicator = new FocusIndicator();
        const toggle = this._indicator.toggle;
        toggle.connect('clicked', () => { this._toggle(); });
        toggle.menu.connect('open-state-changed', (_m: unknown, open: boolean) => {
            if (open)
                void this._fillMenu();
            return undefined;
        });
        Main.panel.statusArea.quickSettings?.addExternalIndicator(this._indicator);
        this._client = new FocusClient(extDir, () => { void this._refresh(); }, () => { this._sessionEnded(); });
        this._render();
        void this._refresh();
    }

    setSkin(skin: string): void {
        this._skin = toSkin(skin);
        this._render();
    }

    private async _refresh(): Promise<void> {
        const serial = ++this._serial;
        let st: FocusStatus | null;
        try {
            st = await this._client.status();
        } catch {
            st = null;
        }
        if (this._destroyed || serial !== this._serial)
            return;
        this._onStatus(st);
    }

    private _onStatus(st: FocusStatus | null): void {
        const was = this._status?.mode ?? '';
        const now = st?.mode ?? '';
        if (!was && now) {
            this._missedBase = trayCount();
        } else if (was && !now && this._missedBase !== null) {
            const missed = trayCount() - this._missedBase;
            this._missedBase = null;
            if (missed > 0 && (this._settings?.get_boolean('summary-on-end') ?? true))
                Main.notify(_('Focus'), fmt(_('Missed notifications: {count}'), {count: missed}));
        }
        allowed.clear();
        if (this._enabled()) {
            for (const a of st?.allowedApps ?? [])
                allowed.add(canonical(a));
        }
        this._status = st;
        this._render();
        this._armCountdown();
    }

    /** While a session or its break runs, the countdown in the tile is refreshed twice a minute. */
    private _armCountdown(): void {
        const st = this._status;
        const counting = st !== null && ((st.source === 'session' && st.until > 0) || st.breakUntil > 0);
        if (counting && !this._tick) {
            this._tick = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 30, () => {
                this._render();
                return GLib.SOURCE_CONTINUE;
            });
        } else if (!counting && this._tick) {
            GLib.source_remove(this._tick);
            this._tick = 0;
        }
    }

    private _subtitle(st: FocusStatus | null): string {
        if (!st)
            return _('Unavailable');
        const left = (t: number) => Math.max(0, Math.ceil((t - nowUnix()) / 60));
        if (st.mode && st.source === 'session' && st.until > 0)
            return fmt(_('{minutes} min left'), {minutes: left(st.until)});
        if (st.mode)
            return st.until > 0 ? fmt(_('{mode} until {time}'), {mode: st.name, time: hhmm(st.until)}) : st.name;
        if (st.breakUntil > 0)
            return fmt(_('Break: {minutes} min left'), {minutes: left(st.breakUntil)});
        return '';
    }

    private _enabled(): boolean {
        return this._settings?.get_boolean('enabled') ?? true;
    }

    private _render(): void {
        if (this._destroyed)
            return;
        const enabled = this._enabled();
        const st = this._status;
        const active = enabled && Boolean(st?.mode);
        if (!enabled)
            allowed.clear();
        const iconName = this._skin === 'windows11' ? BELL_OFF : MOON;
        const toggle = this._indicator.toggle;
        toggle.visible = enabled;
        toggle.checked = active;
        toggle.iconName = iconName;
        toggle.subtitle = this._subtitle(st);
        // the state in words for screen readers, not only the icon
        toggle.accessible_name = statusLine(st);
        toggle.menu.setHeader(iconName, _('Focus'), statusLine(st));
        this._indicator.icon.icon_name = iconName;
        this._indicator.icon.visible = active;
        this._indicator.icon.accessible_name = statusLine(st);
    }

    private _call(p: Promise<void>): void {
        p.catch((e: unknown) => {
            Main.notifyError(_('Focus'), errorText(e));
        }).finally(() => { void this._refresh(); });
    }

    private _toggle(): void {
        if (!this._enabled())
            return;
        if (!this._status) {
            void this._refresh();
            return;
        }
        this._call(this._status.mode ? this._client.stop() : this._client.start('dnd', 0));
    }

    private async _fillMenu(): Promise<void> {
        const toggle = this._indicator.toggle;
        let modes: FocusMode[] = [];
        try {
            modes = await this._client.modes();
        } catch {
            // deskd unreachable: the tile says so; GNOME's own switch still works
        }
        if (this._destroyed)
            return;
        toggle.modes.removeAll();
        toggle.extra.removeAll();
        if (!modes.length) {
            toggle.modes.addMenuItem(new PopupMenu.PopupMenuItem(_('Unavailable'), {reactive: false}));
        }
        const active = this._status?.mode ?? '';
        for (const m of modes) {
            const item = new PopupMenu.PopupMenuItem(m.label);
            item.setOrnament(m.id === active ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NO_DOT);
            item.accessible_name = m.id === active ? fmt(_('{mode}, on'), {mode: m.label}) : m.label;
            item.connect('activate', () => { this._call(this._client.start(m.id, 0)); });
            toggle.modes.addMenuItem(item);
        }
        if (modes.length) {
            toggle.extra.addAction(_('Do not disturb for 1 hour'), () => {
                this._call(this._client.start('dnd', nowUnix() + 3600));
            });
            toggle.extra.addAction(_('Do not disturb until tomorrow morning'), () => {
                this._call(this._client.start('dnd', tomorrowMorning()));
            });
            if (this._skin === 'windows11') {
                const sub = new PopupMenu.PopupSubMenuMenuItem(_('Focus session'));
                for (const min of SESSION_MINUTES) {
                    sub.menu.addAction(fmt(_('{minutes} minutes'), {minutes: min}), () => {
                        this._call(this._client.startSession(min));
                    });
                }
                toggle.extra.addMenuItem(sub);
            }
            if (active)
                toggle.extra.addAction(_('End focus'), () => { this._call(this._client.stop()); });
        }
        toggle.extra.addAction(_('Focus settings…'), () => {
            this._openPrefs();
            Main.panel.closeQuickSettings();
        });
    }

    private _sessionEnded(): void {
        void this._client.status().then(st => {
            if (this._destroyed)
                return;
            const body = st.breakUntil > 0 ? fmt(_('Break until {time}'), {time: hhmm(st.breakUntil)}) : '';
            Main.notify(_('Focus session ended'), body);
        }).catch(logError);
    }

    destroy(): void {
        this._destroyed = true;
        if (this._enabledId && this._settings)
            this._settings.disconnect(this._enabledId);
        this._enabledId = 0;
        if (this._tick)
            GLib.source_remove(this._tick);
        this._tick = 0;
        allowed.clear();
        this._unpatch?.();
        this._unpatch = null;
        this._client.destroy();
        this._indicator.destroy();
    }
}
