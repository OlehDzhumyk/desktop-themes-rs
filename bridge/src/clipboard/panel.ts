// SPDX-License-Identifier: GPL-3.0-or-later
// Base of the clipboard panels: a modal surface over the current monitor with a list of items that
// deskd returns, keyboard navigation and activation (paste). Skins subclass it for layout and style.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Pango from 'gi://Pango';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import type {ClipItem, ClipboardClient} from '../dbus.js';
import {_, fmt} from '../i18n.js';
import {errorText, inputMethod, pushModal} from '../shellTypes.js';

type Monitor = NonNullable<typeof Main.layoutManager.currentMonitor>;

export const VERTICAL = {orientation: Clutter.Orientation.VERTICAL};
export const HORIZONTAL = {orientation: Clutter.Orientation.HORIZONTAL};

export interface PanelHost {
    client: ClipboardClient;
    settings: Gio.Settings;
    /** Close the panel, put the item on the clipboard and paste it into the focused window. */
    activate(item: ClipItem): void;
    openSettings(): void;
}

export type Anchor = 'pointer' | 'caret' | 'center' | 'bottom-right';

/** Relative time, short. */
export function ago(unix: number): string {
    const s = Math.max(0, Math.floor(Date.now() / 1000) - unix);
    if (s < 60)
        return _('just now');
    if (s < 3600)
        return fmt(_('{n} min ago'), {n: Math.floor(s / 60)});
    if (s < 86400)
        return fmt(_('{n} h ago'), {n: Math.floor(s / 3600)});
    return fmt(_('{n} d ago'), {n: Math.floor(s / 86400)});
}

/** What a screen reader says for an item: its text (or image description) and state. */
export function itemAccessibleName(item: ClipItem): string {
    const text = oneLine(item.text, 120);
    return item.pinned ? fmt(_('{text}, pinned'), {text}) : text;
}

/** Animations follow org.gnome.desktop.interface enable-animations and the reduced-motion setting. */
export function animationsEnabled(): boolean {
    const s = St.Settings.get();
    return s.enable_animations && s.reduced_motion !== St.ReducedMotion.REDUCE;
}

/**
 * Classes that let the skins follow the system (stylesheet.css):
 * .desk-light (colour scheme is not dark), .desk-accent-<colour> (GNOME accent colour) and
 * .desk-hc (high contrast: outlines, so nothing depends on colour alone).
 */
export function syncSystemClasses(widget: St.Widget): void {
    const st = St.Settings.get();
    const set = (cls: string, on: boolean) => {
        if (on)
            widget.add_style_class_name(cls);
        else
            widget.remove_style_class_name(cls);
    };
    set('desk-hc', st.high_contrast);
    set('desk-light', st.color_scheme !== St.SystemColorScheme.PREFER_DARK);
    const accents: [St.SystemAccentColor, string][] = [
        [St.SystemAccentColor.BLUE, 'blue'], [St.SystemAccentColor.TEAL, 'teal'],
        [St.SystemAccentColor.GREEN, 'green'], [St.SystemAccentColor.YELLOW, 'yellow'],
        [St.SystemAccentColor.ORANGE, 'orange'], [St.SystemAccentColor.RED, 'red'],
        [St.SystemAccentColor.PINK, 'pink'], [St.SystemAccentColor.PURPLE, 'purple'],
        [St.SystemAccentColor.SLATE, 'slate'],
    ];
    for (const [color, name] of accents)
        set(`desk-accent-${name}`, st.accent_color === color);
}

/** One line preview of a text item (whitespace collapsed). */
export function oneLine(text: string, max = 300): string {
    const t = text.replace(/\s+/g, ' ').trim();
    return t.length > max ? `${t.slice(0, max)}…` : t;
}

export function label(text: string, styleClass: string, lines = 1): St.Label {
    const l = new St.Label({text, style_class: styleClass, x_expand: true, y_align: Clutter.ActorAlign.CENTER});
    const ct = l.clutter_text;
    ct.ellipsize = Pango.EllipsizeMode.END;
    if (lines > 1) {
        ct.line_wrap = true;
        ct.line_wrap_mode = Pango.WrapMode.WORD_CHAR;
        // St has no max-lines: cap the height through the style sheet (line-height × lines)
    }
    return l;
}

export function imageWidget(item: ClipItem, styleClass: string, size: number): St.Widget {
    if (item.image) {
        const icon = new St.Icon({
            gicon: new Gio.FileIcon({file: Gio.File.new_for_path(item.image)}),
            icon_size: size,
            style_class: styleClass,
        });
        return icon;
    }
    return new St.Icon({icon_name: 'image-x-generic-symbolic', icon_size: size, style_class: styleClass});
}

/** Pin icon: filled when pinned, an outline (shipped with the extension) when not, so the state is
 * not conveyed by colour alone. */
export function pinIcon(pinned: boolean): Gio.Icon {
    if (pinned)
        return new Gio.ThemedIcon({name: 'view-pin-symbolic'});
    const [file] = GLib.filename_from_uri(import.meta.url);
    const root = Gio.File.new_for_path(file).get_parent()?.get_parent();
    if (!root)
        return new Gio.ThemedIcon({name: 'view-pin-symbolic'});
    return new Gio.FileIcon({file: root.get_child('icons').get_child('desk-pin-outline-symbolic.svg')});
}

export function iconButton(icon: string | Gio.Icon, accessible: string, styleClass: string,
    onClick: () => void): St.Button {
    const b = new St.Button({
        style_class: styleClass,
        can_focus: true,
        accessible_name: accessible,
        child: typeof icon === 'string'
            ? new St.Icon({icon_name: icon, style_class: 'desk-clip-button-icon'})
            : new St.Icon({gicon: icon, style_class: 'desk-clip-button-icon'}),
    });
    b.connect('clicked', () => { onClick(); });
    return b;
}

/**
 * Classes that let the skins follow the system (stylesheet.css):
 * .desk-light (colour scheme is not dark), .desk-accent-<colour> (GNOME accent colour) and
 * .desk-hc (high contrast: outlines, so nothing depends on colour alone).
 */
export function syncSystemClasses(widget: St.Widget): void {
    const st = St.Settings.get();
    const set = (cls: string, on: boolean) => {
        if (on)
            widget.add_style_class_name(cls);
        else
            widget.remove_style_class_name(cls);
    };
    set('desk-hc', st.high_contrast);
    set('desk-light', st.color_scheme !== St.SystemColorScheme.PREFER_DARK);
    const accents: [St.SystemAccentColor, string][] = [
        [St.SystemAccentColor.BLUE, 'blue'], [St.SystemAccentColor.TEAL, 'teal'],
        [St.SystemAccentColor.GREEN, 'green'], [St.SystemAccentColor.YELLOW, 'yellow'],
        [St.SystemAccentColor.ORANGE, 'orange'], [St.SystemAccentColor.RED, 'red'],
        [St.SystemAccentColor.PINK, 'pink'], [St.SystemAccentColor.PURPLE, 'purple'],
        [St.SystemAccentColor.SLATE, 'slate'],
    ];
    for (const [color, name] of accents)
        set(`desk-accent-${name}`, st.accent_color === color);
}

export abstract class ClipboardPanel {
    readonly actor: St.Widget;
    protected box: St.BoxLayout;
    protected items: ClipItem[] = [];
    protected rows: St.Widget[] = [];
    protected selected = 0;
    private _grab: Clutter.Grab | null = null;
    private _refreshSerial = 0;
    private _closing = false;
    /** The actor was destroyed: by destroy(), or by the shell tearing the stage down at logout,
     * while a deskd reply or signal may still arrive. Nothing may touch the actor after that. */
    private _gone = false;

    protected constructor(protected host: PanelHost, protected skin: string, protected anchor: Anchor) {
        this.actor = new St.Widget({
            reactive: true,
            visible: false,
            style_class: `desk-clip-backdrop desk-skin-${skin}`,
        });
        this.box = new St.BoxLayout({...VERTICAL, style_class: `desk-clip-panel desk-skin-${skin}`, reactive: true});
        this.box.accessible_role = Atk.Role.DIALOG;
        this.box.accessible_name = _('Clipboard history');
        this.actor.add_child(this.box);
        this.actor.connect('button-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            const [x, y] = ev.get_coords();
            const [bx, by] = this.box.get_transformed_position();
            const [bw, bh] = this.box.get_transformed_size();
            if (x < bx || y < by || x > bx + bw || y > by + bh) {
                this.close();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
        this.actor.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this.onKey(ev));
        this.actor.connect('destroy', () => {
            this._gone = true;
            this._grab = null;
        });
        Main.layoutManager.addTopChrome(this.actor);
    }

    get isOpen(): boolean {
        return !this._gone && this.actor.visible;
    }

    /** Query string for List() (search field). */
    protected query(): string {
        return '';
    }

    protected limit(): number {
        return 0;
    }

    /** Build the rows for `this.items` into the list container. */
    protected abstract render(): void;

    /** Where the keyboard focus goes when the panel opens. */
    protected abstract focusInitial(): void;

    /** Skin-specific keys; return true when handled. */
    protected handleKey(_ev: Clutter.Event, _sym: number, _ctrl: boolean): boolean {
        return false;
    }

    toggle(): void {
        if (this.isOpen)
            this.close();
        else
            this.open();
    }

    open(): void {
        if (this.isOpen)
            return;
        const mon = Main.layoutManager.currentMonitor;
        if (!mon)
            return;
        this.actor.set_position(mon.x, mon.y);
        this.actor.set_size(mon.width, mon.height);
        const anchor = this._anchorPoint(mon);
        this.actor.show();
        this._grab = pushModal(this.actor, Shell.ActionMode.POPUP);
        if (!this._grab) {
            this.actor.hide();
            return;
        }
        this.selected = 0;
        syncSystemClasses(this.box);
        this.onOpen();
        void this.refresh().then(() => { this._place(anchor); });
        this._place(anchor);
        this.focusInitial();
        // a short fade only when the user has not turned animations off
        this.box.remove_all_transitions();
        if (animationsEnabled()) {
            this.box.opacity = 0;
            this.box.ease({opacity: 255, duration: 120, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        } else {
            this.box.opacity = 255;
        }
    }

    /** Called before the first refresh (reset search etc.). */
    protected onOpen(): void {
        // nothing to reset in the base panel
    }

    close(): void {
        if (!this.isOpen || this._closing)
            return;
        this._closing = true;
        if (this._grab)
            Main.popModal(this._grab);
        this._grab = null;
        this.actor.hide();
        this._closing = false;
    }

    async refresh(): Promise<void> {
        const serial = ++this._refreshSerial;
        try {
            const items = await this.host.client.list(this.query(), this.limit());
            if (serial !== this._refreshSerial || this._gone)
                return;
            this.items = items;
        } catch (e) {
            if (serial !== this._refreshSerial || this._gone)
                return;
            this.items = [];
            console.warn(`desk-bridge: deskd List failed: ${errorText(e)}`);
        }
        if (this.selected >= this.items.length)
            this.selected = Math.max(0, this.items.length - 1);
        this.render();
        this.highlight();
    }

    protected highlight(): void {
        this.rows.forEach((r, i) => {
            if (i === this.selected) {
                r.add_style_pseudo_class('selected');
                r.add_accessible_state(Atk.StateType.SELECTED);
            } else {
                r.remove_style_pseudo_class('selected');
                r.remove_accessible_state(Atk.StateType.SELECTED);
            }
        });
        const row = this.rows[this.selected];
        if (row) {
            this.scrollTo(row);
            // keyboard focus on a row (Tab reached the list): it follows the selection
            const focus = global.stage.get_key_focus();
            if (focus && focus !== row && this.rows.some(r => r === focus) && row.can_focus)
                row.grab_key_focus();
        }
    }

    /** Scroll a row into view (skins with a St.ScrollView override). */
    protected scrollTo(_row: St.Widget): void {
        // skins without a scroll view have nothing to scroll
    }

    protected move(delta: number): void {
        if (!this.items.length)
            return;
        this.selected = Math.min(this.items.length - 1, Math.max(0, this.selected + delta));
        this.highlight();
    }

    protected activateSelected(): void {
        const it = this.items[this.selected];
        if (it)
            this.activate(it);
    }

    protected activate(item: ClipItem): void {
        if (!this.isOpen)
            return;
        this.close();
        this.host.activate(item);
    }

    protected async togglePin(item: ClipItem): Promise<void> {
        await this.host.client.pin(item.id, !item.pinned).catch(logError);
        await this.refresh();
    }

    protected async remove(item: ClipItem): Promise<void> {
        await this.host.client.remove(item.id).catch(logError);
        await this.refresh();
    }

    protected onKey(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const ctrl = (ev.get_state() & Clutter.ModifierType.CONTROL_MASK) !== 0;
        if (sym === Clutter.KEY_Escape) {
            this.close();
            return Clutter.EVENT_STOP;
        }
        if (this.handleKey(ev, sym, ctrl))
            return Clutter.EVENT_STOP;
        switch (sym) {
        case Clutter.KEY_Up:
            this.move(-1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Down:
            this.move(1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Page_Up:
            this.move(-5);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Page_Down:
            this.move(5);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Home:
            this.move(-this.items.length);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_End:
            this.move(this.items.length);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Tab:
        case Clutter.KEY_ISO_Left_Tab: {
            // Tab / Shift+Tab walk the focusable controls of the panel (search, buttons)
            const back = sym === Clutter.KEY_ISO_Left_Tab ||
                (ev.get_state() & Clutter.ModifierType.SHIFT_MASK) !== 0;
            const from = global.stage.get_key_focus();
            this.box.navigate_focus(from, back ? St.DirectionType.TAB_BACKWARD : St.DirectionType.TAB_FORWARD, true);
            return Clutter.EVENT_STOP;
        }
        case Clutter.KEY_Return:
        case Clutter.KEY_KP_Enter:
            this.activateSelected();
            return Clutter.EVENT_STOP;
        }
        return Clutter.EVENT_PROPAGATE;
    }

    /** Point the panel is placed at: text caret (input method), pointer, or monitor geometry. */
    private _anchorPoint(mon: Monitor): [number, number] {
        if (this.anchor === 'caret') {
            // GNOME's input method keeps the caret rectangle the focused client reported
            const r = inputMethod()._cursorRect;
            if (r && r.x >= mon.x && r.y >= mon.y && r.x < mon.x + mon.width && r.y < mon.y + mon.height &&
                (r.x !== 0 || r.y !== 0))
                return [r.x, r.y + (r.height > 0 ? r.height : 16) + 6];
        }
        if (this.anchor === 'caret' || this.anchor === 'pointer') {
            const [x, y] = global.get_pointer();
            return [x, y + 8];
        }
        return [mon.x + mon.width / 2, mon.y + mon.height / 2];
    }

    /** Position the panel box inside the backdrop (monitor-relative). */
    private _place([ax, ay]: [number, number]): void {
        const mon = Main.layoutManager.currentMonitor;
        if (!mon)
            return;
        const [, natW] = this.box.get_preferred_width(-1);
        const [, natH] = this.box.get_preferred_height(natW);
        const work = Main.layoutManager.getWorkAreaForMonitor(mon.index);
        const margin = 12;
        let x: number, y: number;
        switch (this.anchor) {
        case 'center':
            x = mon.x + (mon.width - natW) / 2;
            y = mon.y + Math.round(mon.height * 0.22);
            break;
        case 'bottom-right':
            x = work.x + work.width - natW - margin;
            y = work.y + work.height - natH - margin;
            break;
        default:
            x = ax - natW / 2;
            y = ay;
            if (y + natH > work.y + work.height - margin)
                y = Math.max(work.y + margin, ay - natH - 40);
        }
        x = Math.min(Math.max(x, work.x + margin), work.x + work.width - natW - margin);
        y = Math.min(Math.max(y, work.y + margin), work.y + work.height - natH - margin);
        this.box.set_position(Math.round(x - mon.x), Math.round(y - mon.y));
    }

    destroy(): void {
        if (this._gone)
            return;
        this.close();
        Main.layoutManager.removeChrome(this.actor);
        this.actor.destroy();
    }
}

export function logError(e: unknown): void {
    console.warn(`desk-bridge: ${errorText(e)}`);
}
