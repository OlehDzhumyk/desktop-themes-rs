// SPDX-License-Identifier: GPL-3.0-or-later
// Snap assist (docs/spec/features/window-layouts.md, AC-WIN-02): after a window is snapped and
// other zones of the layout stay empty, a compact modal strip offers the other normal windows of
// the workspace, one zone at a time. Each choice shows a live miniature of the window and its title
// (the accessible name); 1..9 or a click places that window into the zone that the preview shows,
// Left/Right/Tab move the focus, Enter or Space picks, Esc skips the rest. Titles are used in
// memory only; nothing is stored.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_, fmt} from '../i18n.js';
import {pushModal} from '../shellTypes.js';
import {VERTICAL, label, syncSystemClasses} from '../ui/primitives.js';
import type {Rect} from './client.js';
import type {ZonePreview} from './preview.js';

/** At most this many windows are offered (keys 1..9). */
const MAX_CHOICES = 9;
const THUMB_WIDTH = 160;
const THUMB_HEIGHT = 100;

/** An empty zone: where it is and its number in the layout (1-based). */
export interface EmptyZone {
    rect: Rect;
    number: number;
    count: number;
}

export interface AssistCallbacks {
    /** Place `win` into `zone`; the strip stays open for the next empty zone. */
    place: (win: Meta.Window, zone: EmptyZone) => void;
    /** The strip closed (all zones filled, Esc, no windows left); it is destroyed before this runs. */
    close: () => void;
}

/** A live, scaled miniature of the window (or its app icon when it has no actor). */
function thumbnail(win: Meta.Window): Clutter.Actor {
    const source: Clutter.Actor | null = win.get_compositor_private<Meta.WindowActor | null>();
    const rect = win.get_frame_rect();
    if (source && rect.width > 0 && rect.height > 0) {
        const scale = Math.min(THUMB_WIDTH / rect.width, THUMB_HEIGHT / rect.height, 1);
        return new Clutter.Clone({
            source,
            width: Math.round(rect.width * scale),
            height: Math.round(rect.height * scale),
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
            x_expand: true,
            y_expand: true,
        });
    }
    const app = Shell.WindowTracker.get_default().get_window_app(win);
    return app.create_icon_texture(48);
}

export class SnapAssist {
    private _layer: St.Widget;
    private _strip: St.BoxLayout;
    private _title: St.Label;
    private _row: St.BoxLayout;
    private _buttons: St.Button[] = [];
    private _zones: EmptyZone[];
    private _windows: Meta.Window[];
    private _windowIds = new Map<Meta.Window, number>();
    private _preview: ZonePreview;
    private _area: Rect;
    private _grab: Clutter.Grab | null = null;
    private _cb: AssistCallbacks | null;
    private _destroyed = false;

    /** `preview` belongs to the caller; the assist only shows and hides it. */
    constructor(skin: string, area: Rect, zones: EmptyZone[], windows: Meta.Window[], preview: ZonePreview,
        cb: AssistCallbacks) {
        this._area = area;
        this._zones = [...zones];
        this._windows = windows.slice(0, MAX_CHOICES);
        this._preview = preview;
        this._cb = cb;
        this._layer = new St.Widget({
            style_class: `desk-layouts-assist-layer desk-skin-${skin}`,
            reactive: true,
            x: 0, y: 0,
            width: global.stage.width,
            height: global.stage.height,
            accessible_role: Atk.Role.WINDOW,
            accessible_name: _('Snap assist'),
        });
        this._strip = new St.BoxLayout({
            ...VERTICAL,
            style_class: `desk-layouts-assist desk-skin-${skin}${skin === 'default' ? ' popup-menu-content' : ''}`,
            reactive: true,
            accessible_role: Atk.Role.DIALOG,
            accessible_name: _('Snap assist'),
        });
        syncSystemClasses(this._strip);
        this._title = new St.Label({style_class: 'desk-layouts-title'});
        this._strip.add_child(this._title);
        this._row = new St.BoxLayout({style_class: 'desk-layouts-assist-row'});
        this._strip.add_child(this._row);
        this._strip.add_child(new St.Label({
            text: _('Press 1–9 or click a window; Esc skips'),
            style_class: 'desk-layouts-hint',
        }));
        this._layer.add_child(this._strip);

        // a window that closes meanwhile leaves the list
        for (const w of this._windows) {
            this._windowIds.set(w, w.connect('unmanaging', () => {
                this._forget(w);
                this._refresh();
            }));
        }
        this._layer.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._key(ev));
        this._layer.connect('button-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            const [x, y] = ev.get_coords();
            const [px, py] = this._strip.get_transformed_position();
            const [pw, ph] = this._strip.get_transformed_size();
            if (x < px || x >= px + pw || y < py || y >= py + ph || ev.get_button() === Clutter.BUTTON_SECONDARY)
                this._close();
            return Clutter.EVENT_STOP;
        });
        this._layer.connect('destroy', () => { this._grab = null; });
    }

    /** Show the strip and take the input. False when there is nothing to offer or the grab was refused. */
    open(): boolean {
        if (this._zones.length === 0 || this._windows.length === 0) {
            this.destroy();
            return false;
        }
        Main.layoutManager.addTopChrome(this._layer);
        this._grab = pushModal(this._layer, Shell.ActionMode.POPUP);
        if (!this._grab) {
            this.destroy();
            return false;
        }
        this._refresh();
        return true;
    }

    private _forget(win: Meta.Window): void {
        const id = this._windowIds.get(win);
        if (id)
            win.disconnect(id);
        this._windowIds.delete(win);
        this._windows = this._windows.filter(w => w !== win);
    }

    /** Rebuild the choices for the current zone; closes when no zone or no window is left. */
    private _refresh(): void {
        if (this._destroyed)
            return;
        const zone = this._zones[0];
        if (!zone || this._windows.length === 0) {
            this._close();
            return;
        }
        this._title.text = fmt(_('Choose a window for zone {number} of {count}'),
            {number: zone.number, count: zone.count});
        this._strip.accessible_name = this._title.text;
        this._row.destroy_all_children();
        this._buttons = this._windows.map((win, i) => {
            const box = new St.BoxLayout({...VERTICAL, style_class: 'desk-layouts-assist-box'});
            const thumb = new St.Bin({style_class: 'desk-layouts-assist-thumb', width: THUMB_WIDTH, height: THUMB_HEIGHT,
                child: thumbnail(win)});
            box.add_child(thumb);
            const title = win.get_title() || _('Untitled window');
            const caption = new St.BoxLayout({style_class: 'desk-layouts-assist-caption'});
            caption.add_child(new St.Label({text: String(i + 1), style_class: 'desk-layouts-assist-number',
                y_align: Clutter.ActorAlign.CENTER}));
            caption.add_child(label(title, 'desk-layouts-assist-label'));
            box.add_child(caption);
            const b = new St.Button({
                style_class: 'desk-layouts-assist-choice',
                can_focus: true,
                reactive: true,
                accessible_name: fmt(_('{number}: {title}'), {number: i + 1, title}),
                child: box,
            });
            b.connect('clicked', () => { this._pick(i); });
            this._row.add_child(b);
            return b;
        });
        this._place();
        this._preview.show(zone.rect);
        this._buttons[0]?.grab_key_focus();
    }

    private _place(): void {
        const a = this._area;
        this._strip.set_width(-1);
        const [, natW] = this._strip.get_preferred_width(-1);
        const width = Math.min(natW, a.w - 24);
        this._strip.set_width(width);
        const [, natH] = this._strip.get_preferred_height(width);
        this._strip.set_position(Math.round(a.x + (a.w - width) / 2), Math.round(a.y + (a.h - natH) / 2));
    }

    private _pick(i: number): void {
        const win = this._windows[i];
        const zone = this._zones.shift();
        if (!win || !zone)
            return;
        this._forget(win);
        this._cb?.place(win, zone);
        this._refresh();
    }

    private _key(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const digit = sym >= Clutter.KEY_1 && sym <= Clutter.KEY_9 ? sym - Clutter.KEY_1
            : sym >= Clutter.KEY_KP_1 && sym <= Clutter.KEY_KP_9 ? sym - Clutter.KEY_KP_1 : -1;
        if (digit >= 0) {
            if (digit < this._windows.length)
                this._pick(digit);
            return Clutter.EVENT_STOP;
        }
        const focus = global.stage.key_focus;
        const shift = (ev.get_state() & Clutter.ModifierType.SHIFT_MASK) !== 0;
        switch (sym) {
        case Clutter.KEY_Escape:
            this._close();
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Return:
        case Clutter.KEY_KP_Enter:
        case Clutter.KEY_ISO_Enter:
        case Clutter.KEY_space: {
            const i = this._buttons.findIndex(b => b === focus);
            this._pick(i < 0 ? 0 : i);
            return Clutter.EVENT_STOP;
        }
        case Clutter.KEY_Tab:
            this._row.navigate_focus(focus, shift ? St.DirectionType.TAB_BACKWARD : St.DirectionType.TAB_FORWARD, true);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_ISO_Left_Tab:
        case Clutter.KEY_Left:
        case Clutter.KEY_Up:
            this._row.navigate_focus(focus, St.DirectionType.TAB_BACKWARD, true);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Right:
        case Clutter.KEY_Down:
            this._row.navigate_focus(focus, St.DirectionType.TAB_FORWARD, true);
            return Clutter.EVENT_STOP;
        default:
            return Clutter.EVENT_PROPAGATE;
        }
    }

    private _close(): void {
        const cb = this._cb;
        this._cb = null;
        this.destroy();
        cb?.close();
    }

    destroy(): void {
        if (this._destroyed)
            return;
        this._destroyed = true;
        this._cb = null;
        for (const [win, id] of this._windowIds)
            win.disconnect(id);
        this._windowIds.clear();
        this._windows = [];
        this._preview.hide();
        if (this._grab) {
            Main.popModal(this._grab);
            this._grab = null;
        }
        if (this._layer.get_parent())
            Main.layoutManager.removeChrome(this._layer);
        this._layer.destroy();
        this._buttons = [];
    }
}
