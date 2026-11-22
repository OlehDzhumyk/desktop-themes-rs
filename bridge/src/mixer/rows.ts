// SPDX-License-Identifier: GPL-3.0-or-later
// Rows of the mixer, built from the shell's own popup-menu items so that they sit in the quick
// settings menu like GNOME's: keyboard navigation, focus, theme colours, light/dark and accent all
// come from the shell. One row per application: icon, name, slider, mute button, “Play on…”.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import St from 'gi://St';

import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Slider} from 'resource:///org/gnome/shell/ui/slider.js';

import {_, fmt} from '../i18n.js';
import type {MixerApp, MixerDevice} from './client.js';

/** What a row asks of the mixer; the row itself decides nothing. */
export interface RowHost {
    readonly maxVolume: number;
    /** Arrow-key step, 0..1 (GSettings volume-step). */
    readonly step: number;
    setAppVolume(app: MixerApp, volume: number): void;
    setAppMute(app: MixerApp, mute: boolean): void;
    moveApp(app: MixerApp, device: MixerDevice): void;
    outputs(): MixerDevice[];
}

export function percent(volume: number): number {
    return Math.round(volume * 100);
}

/** An icon from a GIcon string (an icon name or a file); a generic one when there is none. */
export function appIcon(icon: string): Gio.Icon {
    if (icon) {
        try {
            return Gio.icon_new_for_string(icon);
        } catch {
            // an unusable string: fall through to the generic icon
        }
    }
    return new Gio.ThemedIcon({name: 'application-x-executable-symbolic'});
}

/** Symbolic icon for a device: what the server says, else by bus. */
export function deviceIcon(d: MixerDevice, input: boolean): string {
    if (d.icon)
        return d.icon.endsWith('-symbolic') ? d.icon : `${d.icon}-symbolic`;
    if (d.bus === 'bluetooth')
        return 'audio-headphones-symbolic';
    return input ? 'audio-input-microphone-symbolic' : 'audio-speakers-symbolic';
}

/** A non-reactive heading row (“Output”, “Applications”, ...). */
export function heading(text: string, styleClass = 'desk-mixer-heading'): PopupMenu.PopupMenuItem {
    const item = new PopupMenu.PopupMenuItem(text, {reactive: false, can_focus: false, style_class: styleClass});
    item.accessible_role = Atk.Role.HEADING;
    return item;
}

/** A row with a message (“No applications are playing sound”). */
export function note(text: string): PopupMenu.PopupMenuItem {
    const item = new PopupMenu.PopupMenuItem(text, {reactive: false, style_class: 'desk-mixer-note'});
    item.label.clutter_text.line_wrap = true;
    return item;
}

/** A device to choose, with a check mark on the default (announced as text too). */
export function deviceItem(d: MixerDevice, input: boolean, label: string, checked: boolean,
    onActivate: () => void): PopupMenu.PopupImageMenuItem {
    const item = new PopupMenu.PopupImageMenuItem(label, deviceIcon(d, input));
    item.add_style_class_name('desk-mixer-device');
    item.setOrnament(checked ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NONE);
    item.accessible_name = checked ? fmt(_('{device}, default'), {device: label}) : label;
    item.connect('activate', () => { onActivate(); });
    return item;
}

/**
 * One application: a menu item holding the icon, the name (and what plays), a slider and a mute
 * button. The item takes the keyboard focus; Left/Right (or Up/Down with the slider focused) change
 * the volume by `step`, Shift by 1 %, Home/End go to the ends, M mutes.
 */
export class AppRow {
    readonly item: PopupMenu.PopupBaseMenuItem;
    /** The devices to play on, shown under the row when “Play on…” is open. */
    readonly targets: PopupMenu.PopupMenuSection;
    private _app: MixerApp;
    private _icon: St.Icon;
    private _name: St.Label;
    private _media: St.Label;
    private _slider: Slider;
    private _mute: St.Button;
    private _playOn: St.Button;
    private _dragging = false;
    private _syncing = false;

    constructor(private _host: RowHost, app: MixerApp, compact: boolean) {
        this._app = app;
        this.item = new PopupMenu.PopupBaseMenuItem({activate: false, style_class: 'desk-mixer-app'});
        if (compact)
            this.item.add_style_class_name('compact');
        this._icon = new St.Icon({style_class: 'desk-mixer-app-icon', y_align: Clutter.ActorAlign.CENTER});
        this.item.add_child(this._icon);

        const column = new St.BoxLayout({orientation: Clutter.Orientation.VERTICAL, x_expand: true});
        const names = new St.BoxLayout({x_expand: true});
        this._name = new St.Label({style_class: 'desk-mixer-app-name', y_align: Clutter.ActorAlign.CENTER});
        this._media = new St.Label({style_class: 'desk-mixer-app-media', x_expand: true,
            y_align: Clutter.ActorAlign.CENTER});
        names.add_child(this._name);
        names.add_child(this._media);
        column.add_child(names);
        this._slider = new Slider(0);
        this._slider.x_expand = true;
        this._slider.can_focus = false;
        this._slider.add_style_class_name('desk-mixer-slider');
        column.add_child(this._slider);
        this.item.add_child(column);

        this._mute = new St.Button({style_class: 'desk-mixer-button', can_focus: true, toggle_mode: true,
            y_align: Clutter.ActorAlign.CENTER, child: new St.Icon({icon_name: 'audio-volume-muted-symbolic', icon_size: 16})});
        this._mute.connect('clicked', () => { this._host.setAppMute(this._app, !this._app.mute); });
        this.item.add_child(this._mute);

        this._playOn = new St.Button({style_class: 'desk-mixer-button', can_focus: true, toggle_mode: true,
            y_align: Clutter.ActorAlign.CENTER, accessible_name: _('Play on…'),
            child: new St.Icon({icon_name: 'view-more-symbolic', icon_size: 16})});
        this._playOn.connect('clicked', () => { this._toggleTargets(); });
        this.item.add_child(this._playOn);

        this.targets = new PopupMenu.PopupMenuSection();
        this.targets.actor.visible = false;
        if (this.targets.actor instanceof St.Widget)
            this.targets.actor.add_style_class_name('desk-mixer-targets');

        this._slider.connect('notify::value', () => {
            if (!this._syncing)
                this._host.setAppVolume(this._app, this._slider.value);
        });
        // drag-begin / drag-end are signals of the shell's Slider that the typings do not list
        const dragSignals = this._slider as unknown as {connect(signal: string, cb: () => void): number};
        dragSignals.connect('drag-begin', () => { this._dragging = true; });
        dragSignals.connect('drag-end', () => { this._dragging = false; });
        this.item.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._onKey(ev));
        this.update(app);
    }

    get key(): string {
        return this._app.key;
    }

    get dragging(): boolean {
        return this._dragging;
    }

    /** Show new values from deskd (not while the user drags this slider). */
    update(app: MixerApp): void {
        this._app = app;
        this._icon.gicon = appIcon(app.icon);
        this._name.text = app.name;
        this._media.text = app.media && app.media !== app.name ? `  ${app.media}` : '';
        this._media.visible = this._media.text !== '';
        if (!this._dragging) {
            this._syncing = true;
            this._slider.maximumValue = this._host.maxVolume;
            this._slider.value = Math.min(app.volume, this._host.maxVolume);
            this._syncing = false;
        }
        this._mute.checked = app.mute;
        this._mute.accessible_name = app.mute ? fmt(_('Unmute {app}'), {app: app.name})
            : fmt(_('Mute {app}'), {app: app.name});
        this._slider.accessible_name = fmt(_('{app}: volume'), {app: app.name});
        // what a screen reader says on the row: name, value in percent, muted as text
        const value = fmt(_('{app}: volume {percent} %'), {app: app.name, percent: percent(app.volume)});
        this.item.accessible_name = app.mute ? fmt(_('{row}, muted'), {row: value}) : value;
        if (app.mute)
            this.item.add_style_class_name('muted');
        else
            this.item.remove_style_class_name('muted');
        if (this.targets.actor.visible)
            this._fillTargets();
    }

    focus(): void {
        this.item.grab_key_focus();
    }

    private _toggleTargets(): void {
        const show = !this.targets.actor.visible;
        this.targets.actor.visible = show;
        this._playOn.checked = show;
        if (show) {
            this._fillTargets();
            const first = this.targets.firstMenuItem;
            if (first instanceof PopupMenu.PopupBaseMenuItem)
                first.grab_key_focus();
        }
    }

    private _fillTargets(): void {
        this.targets.removeAll();
        for (const d of this._host.outputs()) {
            this.targets.addMenuItem(deviceItem(d, false, d.description, d.id === this._app.device, () => {
                this._host.moveApp(this._app, d);
                this.targets.actor.visible = false;
                this._playOn.checked = false;
                this.focus();
            }));
        }
    }

    private _setVolume(v: number): void {
        const clamped = Math.max(0, Math.min(this._host.maxVolume, v));
        this._slider.value = clamped; // notify::value sends it
    }

    private _onKey(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const shift = (ev.get_state() & Clutter.ModifierType.SHIFT_MASK) !== 0;
        const step = shift ? 0.01 : this._host.step;
        switch (sym) {
        case Clutter.KEY_Left:
        case Clutter.KEY_KP_Left:
            this._setVolume(this._slider.value - step);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Right:
        case Clutter.KEY_KP_Right:
            this._setVolume(this._slider.value + step);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Home:
            this._setVolume(0);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_End:
            this._setVolume(this._host.maxVolume);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_m:
        case Clutter.KEY_M:
            this._host.setAppMute(this._app, !this._app.mute);
            return Clutter.EVENT_STOP;
        }
        return Clutter.EVENT_PROPAGATE;
    }

    destroy(): void {
        this.targets.destroy();
        this.item.destroy();
    }
}
