// SPDX-License-Identifier: GPL-3.0-or-later
// The layout picker (docs/spec/features/window-layouts.md): a modal popover with a miniature of
// every layout; each miniature shows its zones as buttons. windows11: a compact Fluent popover at
// the top of the monitor (no names under the miniatures); macos: a translucent Move & Resize
// popover at the top; default: a shell-styled popover in the middle of the monitor.
//
// Keyboard (two levels): on the layout level the arrow keys move between the miniatures; Enter,
// Space or Tab go into the zones of the focused layout. On the zone level the arrow keys move
// between the zones (past the last zone to the next layout), Enter or Space places the window,
// Tab goes back to the layouts. Esc, a right click or a click outside closes. Hovering or
// focusing a zone asks for the preview; the accessible name says «Layout <name>, zone <n> of <m>».
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_, fmt} from '../i18n.js';
import {pushModal} from '../shellTypes.js';
import {VERTICAL, syncSystemClasses} from '../ui/primitives.js';
import type {LayoutDef, Rect} from './client.js';

/** Miniatures in one row of the popover. */
const COLUMNS = 3;
/** Width of a miniature in logical pixels; the height follows the monitor's aspect. */
const MINI_WIDTH = 88;
/** Space between zones inside a miniature. */
const MINI_GAP = 3;

/** The name to show: the built-in layouts have translated names, custom ones their own. */
function layoutName(layout: LayoutDef): string {
    if (!layout.builtin)
        return layout.name;
    const names: Record<string, string> = {
        halves: _('Halves'),
        thirds: _('Thirds'),
        'two-thirds': _('Two-thirds'),
        quarters: _('Quarters'),
        focus: _('Focus'),
        stack: _('Stack'),
    };
    return names[layout.id] ?? layout.name;
}

/** The zones as shown on this monitor: columns become rows on a portrait monitor (as deskd does). */
function shownZones(layout: LayoutDef, portrait: boolean): Rect[] {
    return portrait ? layout.zones.map(z => ({x: z.y, y: z.x, w: z.h, h: z.w})) : layout.zones;
}

type Direction = 'left' | 'right' | 'up' | 'down';

/** The index of the nearest zone in `dir` from zone `from`, or -1 when there is none. */
function neighbour(zones: Rect[], from: number, dir: Direction): number {
    const cur = zones[from];
    if (!cur)
        return -1;
    const cx = cur.x + cur.w / 2, cy = cur.y + cur.h / 2;
    let best = -1;
    let bestScore = Infinity;
    zones.forEach((z, i) => {
        if (i === from)
            return;
        const dx = z.x + z.w / 2 - cx, dy = z.y + z.h / 2 - cy;
        const along = dir === 'left' ? -dx : dir === 'right' ? dx : dir === 'up' ? -dy : dy;
        const across = dir === 'left' || dir === 'right' ? Math.abs(dy) : Math.abs(dx);
        if (along <= 0.001)
            return;
        const score = along + 2 * across;
        if (score < bestScore) {
            bestScore = score;
            best = i;
        }
    });
    return best;
}

export interface PickerCallbacks {
    /** A zone is hovered or focused (zone index), or none is any more (null). */
    hover: (layoutId: string, zone: number | null) => void;
    /** A zone was chosen; the picker is destroyed before this runs. */
    choose: (layoutId: string, zone: number) => void;
    /** Closed without a choice; the picker is destroyed before this runs. */
    close: () => void;
}

interface Card {
    layout: LayoutDef;
    zones: Rect[];
    actor: St.BoxLayout;
    buttons: St.Button[];
}

export class LayoutPicker {
    private _layer: St.Widget;
    private _popover: St.BoxLayout;
    private _cards: Card[] = [];
    private _skin: string;
    private _area: Rect;
    private _layoutIdx = 0;
    /** -1: the layout level; otherwise the focused zone. */
    private _zoneIdx = -1;
    private _grab: Clutter.Grab | null = null;
    private _cb: PickerCallbacks | null;
    private _destroyed = false;

    constructor(skin: string, layouts: LayoutDef[], activeId: string, area: Rect, cb: PickerCallbacks) {
        this._skin = skin;
        this._area = area;
        this._cb = cb;
        this._layer = new St.Widget({
            style_class: `desk-layouts-layer desk-skin-${skin}`,
            reactive: true,
            x: 0, y: 0,
            width: global.stage.width,
            height: global.stage.height,
            accessible_role: Atk.Role.WINDOW,
            accessible_name: _('Window layouts'),
        });
        const title = skin === 'macos' ? _('Move & Resize') : skin === 'windows11' ? _('Snap layouts') : _('Window layouts');
        this._popover = new St.BoxLayout({
            ...VERTICAL,
            style_class: `desk-layouts-picker desk-skin-${skin}${skin === 'default' ? ' popup-menu-content' : ''}`,
            reactive: true,
            accessible_role: Atk.Role.DIALOG,
            accessible_name: title,
        });
        syncSystemClasses(this._popover);
        this._layer.add_child(this._popover);
        if (skin !== 'windows11')
            this._popover.add_child(new St.Label({text: title, style_class: 'desk-layouts-title'}));

        const portrait = area.h > area.w;
        const miniHeight = Math.max(40, Math.min(140, Math.round(MINI_WIDTH * area.h / Math.max(1, area.w))));
        const grid = new St.BoxLayout({...VERTICAL, style_class: 'desk-layouts-grid'});
        this._popover.add_child(grid);
        let row: St.BoxLayout | null = null;
        layouts.forEach((layout, li) => {
            if (li % COLUMNS === 0) {
                row = new St.BoxLayout({style_class: 'desk-layouts-row'});
                grid.add_child(row);
            }
            const card = this._card(layout, li, portrait, miniHeight);
            if (layout.id === activeId) {
                this._layoutIdx = li;
                card.actor.add_style_pseudo_class('checked');
            }
            row?.add_child(card.actor);
            this._cards.push(card);
        });
        if (skin !== 'windows11') {
            this._popover.add_child(new St.Label({
                text: _('Arrow keys choose, Enter places the window, Esc closes'),
                style_class: 'desk-layouts-hint',
            }));
        }

        this._layer.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._key(ev));
        this._layer.connect('button-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            // a click outside the popover (or a right click anywhere) closes
            const [x, y] = ev.get_coords();
            const [px, py] = this._popover.get_transformed_position();
            const [pw, ph] = this._popover.get_transformed_size();
            const inside = x >= px && x < px + pw && y >= py && y < py + ph;
            if (!inside || ev.get_button() === Clutter.BUTTON_SECONDARY)
                this._close();
            return Clutter.EVENT_STOP;
        });
        this._layer.connect('destroy', () => { this._grab = null; });
    }

    private _card(layout: LayoutDef, li: number, portrait: boolean, miniHeight: number): Card {
        const name = layoutName(layout);
        const actor = new St.BoxLayout({
            ...VERTICAL,
            style_class: 'desk-layouts-card',
            can_focus: true,
            reactive: true,
            track_hover: true,
            accessible_role: Atk.Role.LIST_ITEM,
            accessible_name: fmt(_('Layout {name}'), {name}),
        });
        const mini = new St.Widget({style_class: 'desk-layouts-mini', width: MINI_WIDTH, height: miniHeight});
        actor.add_child(mini);
        if (this._skin !== 'windows11')
            actor.add_child(new St.Label({text: name, style_class: 'desk-layouts-card-label', x_expand: true}));
        const zones = shownZones(layout, portrait);
        const buttons = zones.map((z, zi) => {
            const x = Math.round(z.x * MINI_WIDTH) + MINI_GAP / 2;
            const y = Math.round(z.y * miniHeight) + MINI_GAP / 2;
            const b = new St.Button({
                style_class: 'desk-layouts-zone',
                can_focus: true,
                reactive: true,
                track_hover: true,
                x: Math.round(x), y: Math.round(y),
                width: Math.max(4, Math.round(z.w * MINI_WIDTH) - MINI_GAP),
                height: Math.max(4, Math.round(z.h * miniHeight) - MINI_GAP),
                accessible_name: fmt(_('Layout {name}, zone {number} of {count}'),
                    {name, number: zi + 1, count: zones.length}),
            });
            b.connect('clicked', () => { this._choose(li, zi); });
            b.connect('notify::hover', () => {
                if (b.hover)
                    this._cb?.hover(layout.id, zi);
                else if (!this._focusedZone())
                    this._cb?.hover(layout.id, null);
            });
            b.connect('key-focus-in', () => {
                this._layoutIdx = li;
                this._zoneIdx = zi;
                this._cb?.hover(layout.id, zi);
            });
            mini.add_child(b);
            return b;
        });
        actor.connect('key-focus-in', () => {
            this._layoutIdx = li;
            this._zoneIdx = -1;
            this._cb?.hover(layout.id, null);
        });
        return {layout, zones, actor, buttons};
    }

    /** The zone button that has the key focus, if any. */
    private _focusedZone(): St.Button | null {
        const focus = global.stage.key_focus;
        for (const c of this._cards) {
            const b = c.buttons.find(btn => btn === focus);
            if (b)
                return b;
        }
        return null;
    }

    /** Show the popover and take the input. False when the shell refused the grab (nothing shown). */
    open(): boolean {
        if (this._cards.length === 0) {
            this.destroy();
            return false;
        }
        Main.layoutManager.addTopChrome(this._layer);
        this._place();
        this._grab = pushModal(this._layer, Shell.ActionMode.POPUP);
        if (!this._grab) {
            this.destroy();
            return false;
        }
        this._focus(this._layoutIdx, -1);
        return true;
    }

    private _place(): void {
        const a = this._area;
        const [, natW] = this._popover.get_preferred_width(-1);
        const [, natH] = this._popover.get_preferred_height(natW);
        const x = Math.round(a.x + (a.w - natW) / 2);
        const y = this._skin === 'default' ? Math.round(a.y + (a.h - natH) / 2) : a.y + 12;
        this._popover.set_position(Math.max(a.x, x), Math.max(a.y, y));
    }

    private _focus(li: number, zi: number): void {
        const card = this._cards[li];
        if (!card)
            return;
        this._layoutIdx = li;
        this._zoneIdx = zi;
        const target = zi >= 0 ? card.buttons[zi] : card.actor;
        target?.grab_key_focus();
    }

    private _move(dir: Direction): void {
        const n = this._cards.length;
        const li = this._layoutIdx;
        if (this._zoneIdx < 0) {
            const step = dir === 'left' ? -1 : dir === 'right' ? 1 : dir === 'up' ? -COLUMNS : COLUMNS;
            const next = li + step;
            if (next >= 0 && next < n)
                this._focus(next, -1);
            return;
        }
        const card = this._cards[li];
        if (!card)
            return;
        const zi = neighbour(card.zones, this._zoneIdx, dir);
        if (zi >= 0) {
            this._focus(li, zi);
            return;
        }
        // past the edge of a layout: into the next or the previous layout
        const forward = dir === 'right' || dir === 'down';
        const next = li + (forward ? 1 : -1);
        const nextCard = this._cards[next];
        if (nextCard)
            this._focus(next, forward ? 0 : nextCard.buttons.length - 1);
    }

    private _key(ev: Clutter.Event): boolean {
        switch (ev.get_key_symbol()) {
        case Clutter.KEY_Escape:
            this._close();
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Tab:
        case Clutter.KEY_ISO_Left_Tab:
            this._focus(this._layoutIdx, this._zoneIdx < 0 ? 0 : -1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Return:
        case Clutter.KEY_KP_Enter:
        case Clutter.KEY_ISO_Enter:
        case Clutter.KEY_space:
            if (this._zoneIdx < 0)
                this._focus(this._layoutIdx, 0);
            else
                this._choose(this._layoutIdx, this._zoneIdx);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Left:
            this._move('left');
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Right:
            this._move('right');
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Up:
            this._move('up');
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Down:
            this._move('down');
            return Clutter.EVENT_STOP;
        default:
            return Clutter.EVENT_PROPAGATE;
        }
    }

    private _choose(li: number, zi: number): void {
        const card = this._cards[li];
        const cb = this._cb;
        this._cb = null;
        this.destroy();
        if (card)
            cb?.choose(card.layout.id, zi);
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
        if (this._grab) {
            Main.popModal(this._grab);
            this._grab = null;
        }
        if (this._layer.get_parent())
            Main.layoutManager.removeChrome(this._layer);
        this._layer.destroy();
        this._cards = [];
    }
}
