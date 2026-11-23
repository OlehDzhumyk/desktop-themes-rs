// SPDX-License-Identifier: GPL-3.0-or-later
// The skinned on-screen display (docs/spec/features/osd.md): one click-through surface with a
// symbolic icon, a level bar and an optional number. It never takes focus or pointer input.
//   windows11  compact acrylic bar: icon, horizontal bar, number (bottom centre by default).
//   macos      translucent square: large icon over a smooth bar (centred by default); a compact
//              pill when placed at the bottom centre or top right.
// The part of the level above 100 % (over-amplification) gets its own colour, a marker at 100 %
// and the words «above 100 %», so it is never shown by colour alone.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {HORIZONTAL, VERTICAL, animationsEnabled, syncSystemClasses} from '../clipboard/panel.js';
import {_, fmt} from '../i18n.js';

export type OsdSkin = 'macos' | 'windows11';
export type OsdPosition = 'bottom-center' | 'center' | 'top-right';

/** What the shell asks to show (the arguments of OsdWindowManager.show*). */
export interface OsdRequest {
    /** Monitor index, or -1 for the monitor with the pointer. */
    monitor: number;
    icon: Gio.Icon | null;
    label: string | null;
    /** Level as a fraction (1 = 100 %), null when the trigger has no level (microphone mute). */
    level: number | null;
    /** Highest level (1, or above 1 when over-amplification is allowed). */
    maxLevel: number;
}

export interface OsdOptions {
    position: OsdPosition;
    showPercentage: boolean;
    timeoutMs: number;
}

const TRACK_WIDTH: Record<OsdSkin, number> = {windows11: 180, macos: 150};
const FADE_IN_MS = 120;
const FADE_OUT_MS = 150;
const SLIDE_PX = 8;
const MARGIN_PX = 48;
const EDGE_PX = 12;

function iconName(icon: Gio.Icon | null): string {
    return icon instanceof Gio.ThemedIcon ? (icon.get_names()[0] ?? '') : '';
}

/** The sentence a screen reader says: the kind of level and its value, or the state in words. */
function announcement(req: OsdRequest): string {
    const name = iconName(req.icon);
    if (req.level === null) {
        if (name.startsWith('microphone'))
            return name.includes('muted') ? _('Microphone muted') : _('Microphone on');
        return req.label ?? '';
    }
    const level = Math.round(req.level * 100);
    let text: string;
    if (name.startsWith('audio-volume-muted'))
        text = _('Volume muted');
    else if (name.startsWith('audio-volume') || name.startsWith('audio-speakers'))
        text = fmt(_('Volume {level} percent'), {level});
    else if (name.startsWith('microphone'))
        text = fmt(_('Microphone {level} percent'), {level});
    else if (name.startsWith('keyboard-brightness'))
        text = fmt(_('Keyboard backlight {level} percent'), {level});
    else if (name.startsWith('display-brightness'))
        text = fmt(_('Brightness {level} percent'), {level});
    else
        text = fmt(_('Level {level} percent'), {level});
    return req.level > 1 ? `${text}, ${_('above 100 %')}` : text;
}

export class OsdPopup {
    readonly actor: St.BoxLayout;
    private _icon: St.Icon;
    private _track: St.Widget;
    private _fill: St.Widget;
    private _over: St.Widget;
    private _marker: St.Widget;
    private _number: St.Label;
    private _overText: St.Label;
    private _text: St.Label;
    private _hideId = 0;
    private _shown = false;
    private _gone = false;

    /** `square`: the macOS centred square (icon above the bar); otherwise one row. */
    constructor(private readonly _skin: OsdSkin, square: boolean) {
        const shape = square ? 'desk-osd-square' : 'desk-osd-row';
        this.actor = new St.BoxLayout({
            ...(square ? VERTICAL : HORIZONTAL),
            style_class: `desk-osd desk-skin-${_skin} ${shape}`,
            reactive: false,
            can_focus: false,
            track_hover: false,
            visible: false,
        });
        this.actor.accessible_role = Atk.Role.NOTIFICATION;
        this._icon = new St.Icon({style_class: 'desk-osd-icon', y_align: Clutter.ActorAlign.CENTER,
            x_align: Clutter.ActorAlign.CENTER});
        this._track = new St.Widget({
            style_class: 'desk-osd-track',
            layout_manager: new Clutter.FixedLayout(),
            y_align: Clutter.ActorAlign.CENTER,
            x_align: Clutter.ActorAlign.CENTER,
        });
        this._over = new St.Widget({style_class: 'desk-osd-over'});
        this._fill = new St.Widget({style_class: 'desk-osd-fill'});
        this._marker = new St.Widget({style_class: 'desk-osd-marker', visible: false});
        this._track.add_child(this._over);
        this._track.add_child(this._fill);
        this._track.add_child(this._marker);
        this._number = new St.Label({style_class: 'desk-osd-number', y_align: Clutter.ActorAlign.CENTER});
        this._overText = new St.Label({style_class: 'desk-osd-over-text', y_align: Clutter.ActorAlign.CENTER,
            text: _('above 100 %'), visible: false});
        this._text = new St.Label({style_class: 'desk-osd-text', y_align: Clutter.ActorAlign.CENTER, visible: false});
        const values = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-osd-values',
            x_align: Clutter.ActorAlign.CENTER});
        values.add_child(this._track);
        values.add_child(this._text);
        values.add_child(this._number);
        values.add_child(this._overText);
        this.actor.add_child(this._icon);
        this.actor.add_child(values);
        this.actor.connect('destroy', () => { this._gone = true; });
        Main.layoutManager.addTopChrome(this.actor);
    }

    /** Show or update the one OSD and restart its one hide timer. */
    show(req: OsdRequest, opts: OsdOptions): void {
        if (this._gone)
            return;
        syncSystemClasses(this.actor);
        const anim = animationsEnabled();
        this._render(req, opts, anim && this._shown);
        this.actor.accessible_name = announcement(req);
        if (!this._shown) {
            this._shown = true;
            this.actor.remove_all_transitions();
            this.actor.show();
            if (anim) {
                const from = opts.position === 'bottom-center' ? SLIDE_PX
                    : opts.position === 'top-right' ? -SLIDE_PX : 0;
                this.actor.opacity = 0;
                this.actor.translation_y = from;
                // translation_y is the GObject property the shell's own ease() calls use;
                // the typings only know the camelCase spelling
                this.actor.ease({opacity: 255, ...({translation_y: 0} as object), duration: FADE_IN_MS,
                    mode: Clutter.AnimationMode.EASE_OUT_QUAD});
            } else {
                this.actor.opacity = 255;
                this.actor.translation_y = 0;
            }
        }
        // placed while visible, so the size comes from the current style and content
        this._place(req.monitor, opts.position);
        this._armTimer(opts.timeoutMs);
    }

    /** Hide; `now` skips the fade (skin switch, disable). No timer stays behind. */
    hide(now = false): void {
        this._clearTimer();
        if (this._gone || !this._shown)
            return;
        this._shown = false;
        this.actor.remove_all_transitions();
        if (now || !animationsEnabled()) {
            this.actor.hide();
            return;
        }
        this.actor.ease({opacity: 0, duration: FADE_OUT_MS, mode: Clutter.AnimationMode.EASE_IN_QUAD,
            onComplete: () => {
                if (!this._gone && !this._shown)
                    this.actor.hide();
            }});
    }

    destroy(): void {
        this._clearTimer();
        if (this._gone)
            return;
        this.actor.remove_all_transitions();
        this._fill.remove_all_transitions();
        Main.layoutManager.removeChrome(this.actor);
        this.actor.destroy();
    }

    private _place(monitor: number, position: OsdPosition): void {
        const mon = this._monitor(monitor);
        if (!mon)
            return;
        const [, , natW, natH] = this.actor.get_preferred_size();
        const wa = Main.layoutManager.getWorkAreaForMonitor(mon.index);
        const scale = St.ThemeContext.get_for_stage(global.stage).scale_factor;
        let x = wa.x + Math.round((wa.width - natW) / 2);
        let y = wa.y + Math.round((wa.height - natH) / 2);
        if (position === 'bottom-center')
            y = wa.y + wa.height - natH - MARGIN_PX * scale;
        else if (position === 'top-right') {
            x = wa.x + wa.width - natW - EDGE_PX * scale;
            y = wa.y + EDGE_PX * scale;
        }
        this.actor.set_position(x, y);
    }

    private _armTimer(ms: number): void {
        this._clearTimer();
        this._hideId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            this._hideId = 0;
            this.hide();
            return GLib.SOURCE_REMOVE;
        });
    }

    private _clearTimer(): void {
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = 0;
    }

    private _monitor(index: number): typeof Main.layoutManager.primaryMonitor {
        const lm = Main.layoutManager;
        return (index >= 0 ? lm.monitors[index] : undefined) ?? lm.currentMonitor ?? lm.primaryMonitor;
    }

    private _render(req: OsdRequest, opts: OsdOptions, ease: boolean): void {
        this._icon.gicon = req.icon ?? new Gio.ThemedIcon({name: 'audio-volume-high-symbolic'});
        const hasLevel = req.level !== null;
        const hc = St.Settings.get().high_contrast;
        // no level: the icon alone, or a short state text from the shell (never a device name:
        // the shell's label is only used when there is no level to show)
        this._track.visible = hasLevel;
        this._text.visible = !hasLevel && Boolean(req.label);
        this._text.text = !hasLevel ? (req.label ?? '') : '';
        if (req.level === null) {
            this._number.visible = false;
            this._overText.visible = false;
            return;
        }
        const max = Math.max(1, req.maxLevel);
        const level = Math.min(Math.max(0, req.level), max);
        const scale = St.ThemeContext.get_for_stage(global.stage).scale_factor;
        const w = TRACK_WIDTH[this._skin] * scale;
        this._track.set_width(w);
        const base = Math.round(w * Math.min(level, 1) / max);
        const hundred = Math.round(w / max);
        const over = level > 1;
        this._fill.remove_all_transitions();
        if (ease)
            this._fill.ease({width: base, duration: 100, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        else
            this._fill.set_width(base);
        this._over.set_position(hundred, 0);
        this._over.set_width(over ? Math.round(w * level / max) - hundred : 0);
        this._over.visible = over;
        this._marker.set_position(Math.max(0, hundred - 1), 0);
        this._marker.visible = max > 1;
        this._number.text = String(Math.round(level * 100));
        this._number.visible = opts.showPercentage || hc || over;
        this._overText.visible = over;
    }
}
