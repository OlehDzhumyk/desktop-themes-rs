// SPDX-License-Identifier: GPL-3.0-or-later
// The search box (docs/spec/features/search.md): a modal overlay on the monitor with the pointer.
// macos: a Spotlight-like wide pill whose top sits at 22% of the monitor height; windows11: a
// Fluent panel in the middle of the monitor; default: a shell-styled (Adwaita) panel in the upper
// third.
//
// Results are grouped: the best result is the top hit, the others follow under the title of their
// kind, in the order deskd ranked them. Keyboard: the field keeps the focus; Up and Down move the
// selection, Tab and Shift+Tab go to the first row of the next or previous group (wrapping),
// Enter opens the selected result, Super+1..9 open the first nine, Esc clears the field and
// closes when it is empty, and the search shortcut pressed again closes. A click outside the panel closes. The panel fades in and out
// in at most 120 ms, without any transition when animations are off or reduced.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_, fmt} from '../i18n.js';
import {pushModal} from '../shellTypes.js';
import {HORIZONTAL, VERTICAL, animationsEnabled, label, syncSystemClasses} from '../ui/primitives.js';
import type {SearchItem, SearchKind} from './client.js';

const FADE_MS = 100;
/** How many rows Super+<n> reaches. */
const NUMBERED = 9;

const FALLBACK_ICONS: Record<SearchKind, string> = {
    calculator: 'accessories-calculator-symbolic',
    app: 'application-x-executable-symbolic',
    action: 'system-run-symbolic',
    file: 'text-x-generic-symbolic',
    clipboard: 'edit-paste-symbolic',
    emoji: 'face-smile-symbolic',
    web: 'web-browser-symbolic',
};

function kindTitle(kind: SearchKind): string {
    switch (kind) {
    case 'calculator':
        return _('Calculator');
    case 'app':
        return _('Applications');
    case 'action':
        return _('System actions');
    case 'file':
        return _('Files');
    case 'clipboard':
        return _('Clipboard');
    case 'emoji':
        return _('Emoji');
    case 'web':
        return _('Web');
    }
}

/** The icon of a result: a theme icon name or an absolute path; the kind's icon otherwise. */
function resultIcon(item: SearchItem): St.Icon {
    const style = 'desk-search-row-icon';
    if (item.icon.startsWith('/'))
        return new St.Icon({gicon: Gio.FileIcon.new(Gio.File.new_for_path(item.icon)), style_class: style});
    if (item.kind === 'app' && item.icon.length > 0)
        return new St.Icon({gicon: Gio.ThemedIcon.new_with_default_fallbacks(item.icon), style_class: style});
    return new St.Icon({icon_name: item.icon || FALLBACK_ICONS[item.kind], style_class: style});
}

/** A group of rows under one title; the top hit is its own group. */
interface Group {
    title: string;
    items: SearchItem[];
}

/** The top hit first, then the other results under their kind, in deskd's order. */
function groupResults(items: SearchItem[]): Group[] {
    const [top, ...rest] = items;
    if (!top)
        return [];
    const groups: Group[] = [{title: _('Top hit'), items: [top]}];
    const byKind = new Map<SearchKind, Group>();
    for (const item of rest) {
        let g = byKind.get(item.kind);
        if (!g) {
            g = {title: kindTitle(item.kind), items: []};
            byKind.set(item.kind, g);
            groups.push(g);
        }
        g.items.push(item);
    }
    return groups;
}

export interface SearchPanelHost {
    /** The text of the field changed (also called with '' when the panel opens). */
    query: (text: string) => void;
    /** Whether a key press (hardware keycode, modifier state) is the search shortcut. */
    isToggle: (keycode: number, state: number) => boolean;
    /** A result was chosen; the panel is destroyed before this runs. */
    activate: (item: SearchItem) => void;
    /** Closed without a choice; the panel is destroyed before this runs. */
    closed: () => void;
}

export class SearchPanel {
    private _layer: St.Widget;
    private _box: St.BoxLayout;
    private _entry: St.Entry;
    private _scroll: St.ScrollView;
    private _list: St.BoxLayout;
    private _empty: St.Label;
    private _skin: string;
    /** The rows in the order shown (the order of Up/Down and Super+<n>). */
    private _rows: {item: SearchItem; button: St.Button}[] = [];
    /** The index of the first row of each group, in order (the stops of Tab). */
    private _groupStarts: number[] = [];
    private _selected = 0;
    private _grab: Clutter.Grab | null = null;
    private _host: SearchPanelHost | null;
    private _destroyed = false;

    constructor(skin: string, host: SearchPanelHost) {
        this._skin = skin;
        this._host = host;
        this._layer = new St.Widget({
            style_class: `desk-search-layer desk-skin-${skin}`,
            reactive: true,
            accessible_role: Atk.Role.WINDOW,
            accessible_name: _('Search'),
        });
        this._box = new St.BoxLayout({
            ...VERTICAL,
            style_class: `desk-search-panel desk-skin-${skin}${skin === 'default' ? ' popup-menu-content' : ''}`,
            reactive: true,
            accessible_role: Atk.Role.DIALOG,
            accessible_name: _('Search'),
        });
        syncSystemClasses(this._box);
        this._layer.add_child(this._box);

        this._entry = new St.Entry({
            style_class: 'desk-search-entry',
            hint_text: _('Type to search'),
            can_focus: true,
            x_expand: true,
        });
        this._entry.accessible_name = _('Search');
        this._entry.set_primary_icon(new St.Icon({icon_name: 'edit-find-symbolic', style_class: 'desk-search-entry-icon'}));
        this._entry.clutter_text.connect('text-changed', () => {
            if (!this._destroyed)
                this._host?.query(this._entry.get_text());
        });
        // before the field's own handling: Enter would only emit «activate» otherwise
        this._entry.clutter_text.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._key(ev));
        this._box.add_child(this._entry);

        this._list = new St.BoxLayout({...VERTICAL, style_class: 'desk-search-results'});
        this._list.accessible_role = Atk.Role.LIST;
        this._list.accessible_name = _('Search results');
        this._scroll = new St.ScrollView({
            style_class: 'desk-search-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            overlay_scrollbars: true,
            visible: false,
            child: this._list,
        });
        this._box.add_child(this._scroll);
        this._empty = new St.Label({style_class: 'desk-search-empty', visible: false});
        this._box.add_child(this._empty);

        this._layer.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._key(ev));
        this._layer.connect('button-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            const [x, y] = ev.get_coords();
            const [bx, by] = this._box.get_transformed_position();
            const [bw, bh] = this._box.get_transformed_size();
            if (x < bx || y < by || x >= bx + bw || y >= by + bh) {
                this._close();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
        this._layer.connect('destroy', () => { this._grab = null; });
    }

    /** Show the panel on the monitor with the pointer and take the input; false when refused. */
    open(): boolean {
        const mon = Main.layoutManager.currentMonitor;
        if (!mon) {
            this.destroy();
            return false;
        }
        this._layer.set_position(mon.x, mon.y);
        this._layer.set_size(mon.width, mon.height);
        Main.layoutManager.addTopChrome(this._layer);
        this._grab = pushModal(this._layer, Shell.ActionMode.POPUP);
        if (!this._grab) {
            this.destroy();
            return false;
        }
        this._place();
        this._entry.grab_key_focus();
        if (animationsEnabled()) {
            this._box.opacity = 0;
            this._box.ease({opacity: 255, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        }
        this._host?.query('');
        return true;
    }

    /** The current text of the field. */
    get text(): string {
        return this._entry.get_text();
    }

    /** Show the results of `text`; ignored when the field no longer holds that text. */
    setResults(text: string, items: SearchItem[]): void {
        if (this._destroyed || text !== this._entry.get_text())
            return;
        this._list.destroy_all_children();
        this._rows = [];
        this._groupStarts = [];
        this._selected = 0;
        const query = text.trim();
        for (const group of groupResults(items)) {
            const box = new St.BoxLayout({...VERTICAL, style_class: 'desk-search-group'});
            box.add_child(new St.Label({text: group.title, style_class: 'desk-search-group-title'}));
            this._groupStarts.push(this._rows.length);
            for (const item of group.items)
                box.add_child(this._row(item, group.title));
            this._list.add_child(box);
        }
        const has = this._rows.length > 0;
        this._scroll.visible = has;
        this._empty.visible = !has && query.length > 0;
        if (!has)
            this._empty.text = fmt(_('Nothing found for “{query}”'), {query});
        this._highlight();
        this._place();
    }

    private _row(item: SearchItem, groupTitle: string): St.Button {
        const index = this._rows.length;
        const content = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-search-row-box', x_expand: true});
        content.add_child(resultIcon(item));
        const texts = new St.BoxLayout({...VERTICAL, style_class: 'desk-search-row-texts', x_expand: true});
        texts.add_child(label(item.title, 'desk-search-row-title'));
        if (item.subtitle)
            texts.add_child(label(item.subtitle, 'desk-search-row-subtitle'));
        content.add_child(texts);
        if (index < NUMBERED) {
            const keys = this._skin === 'macos' ? `⌘${String(index + 1)}` : `Super+${String(index + 1)}`;
            content.add_child(new St.Label({
                text: keys,
                style_class: 'desk-search-row-key',
                y_align: Clutter.ActorAlign.CENTER,
            }));
        }
        const subtitle = item.subtitle ? `, ${item.subtitle}` : '';
        const button = new St.Button({
            style_class: 'desk-search-row',
            child: content,
            reactive: true,
            can_focus: false,
            track_hover: true,
            x_expand: true,
            accessible_role: Atk.Role.LIST_ITEM,
            accessible_name: `${groupTitle}: ${item.title}${subtitle}`,
        });
        button.connect('clicked', () => { this._choose(index); });
        button.connect('notify::hover', () => {
            if (button.hover && this._selected !== index) {
                this._selected = index;
                this._highlight();
            }
        });
        this._rows.push({item, button});
        return button;
    }

    private _highlight(): void {
        this._rows.forEach(({button}, i) => {
            if (i === this._selected) {
                button.add_style_pseudo_class('selected');
                button.add_accessible_state(Atk.StateType.SELECTED);
            } else {
                button.remove_style_pseudo_class('selected');
                button.remove_accessible_state(Atk.StateType.SELECTED);
            }
        });
        const row = this._rows[this._selected]?.button;
        const group = row?.get_parent();
        if (!row || !group)
            return;
        // keep the selected row in view (its box is relative to the group, the group to the list)
        const adj = this._scroll.vadjustment;
        const g = group.get_allocation_box();
        const r = row.get_allocation_box();
        const top = g.y1 + r.y1, bottom = g.y1 + r.y2;
        if (top < adj.value)
            adj.value = top;
        else if (bottom > adj.value + adj.page_size)
            adj.value = bottom - adj.page_size;
    }

    private _move(delta: number): void {
        const n = this._rows.length;
        if (n === 0)
            return;
        this._selected = Math.min(n - 1, Math.max(0, this._selected + delta));
        this._highlight();
    }

    /** Select the first row of the next (1) or previous (-1) group, wrapping around. */
    private _moveGroup(dir: 1 | -1): void {
        const starts = this._groupStarts;
        const n = starts.length;
        if (n === 0)
            return;
        // the group that holds the selection: the last start at or before it
        let current = 0;
        starts.forEach((start, g) => {
            if (start <= this._selected)
                current = g;
        });
        this._selected = starts[(current + dir + n) % n] ?? 0;
        this._highlight();
    }

    private _key(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const state = ev.get_state();
        if (this._host?.isToggle(ev.get_key_code(), state)) {
            this._close();
            return Clutter.EVENT_STOP;
        }
        const superDown = (state & (Clutter.ModifierType.SUPER_MASK | Clutter.ModifierType.MOD4_MASK)) !== 0;
        if (superDown && sym >= Clutter.KEY_1 && sym <= Clutter.KEY_9) {
            const index = sym - Clutter.KEY_1;
            if (index < this._rows.length)
                this._choose(index);
            return Clutter.EVENT_STOP;
        }
        switch (sym) {
        case Clutter.KEY_Escape:
            if (this._entry.get_text().length > 0)
                this._entry.set_text('');
            else
                this._close();
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Up:
        case Clutter.KEY_KP_Up:
            this._move(-1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Down:
        case Clutter.KEY_KP_Down:
            this._move(1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Tab:
        case Clutter.KEY_KP_Tab:
            this._moveGroup((state & Clutter.ModifierType.SHIFT_MASK) !== 0 ? -1 : 1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_ISO_Left_Tab:
            this._moveGroup(-1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Page_Up:
            this._move(-5);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Page_Down:
            this._move(5);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Return:
        case Clutter.KEY_KP_Enter:
        case Clutter.KEY_ISO_Enter:
            if (this._rows.length > 0)
                this._choose(this._selected);
            return Clutter.EVENT_STOP;
        default:
            return Clutter.EVENT_PROPAGATE;
        }
    }

    private _place(): void {
        if (this._destroyed)
            return;
        const mon = Main.layoutManager.currentMonitor;
        if (!mon)
            return;
        const [, natW] = this._box.get_preferred_width(-1);
        const x = Math.round((this._layer.width - natW) / 2);
        let y: number;
        if (this._skin === 'macos') {
            y = Math.round(this._layer.height * 0.22);
        } else if (this._skin === 'windows11') {
            // the middle of the monitor for the empty field; the panel then grows downwards
            const [, natH] = this._box.get_preferred_height(natW);
            y = Math.round(Math.min((this._layer.height - natH) / 2, this._layer.height * 0.3));
        } else {
            y = Math.round(this._layer.height * 0.18);
        }
        this._box.set_position(Math.max(0, x), Math.max(0, y));
    }

    private _choose(index: number): void {
        const row = this._rows[index];
        if (!row)
            return;
        const host = this._host;
        this._host = null;
        this.destroy();
        host?.activate(row.item);
    }

    private _close(): void {
        const host = this._host;
        this._host = null;
        if (!animationsEnabled()) {
            this.destroy();
            host?.closed();
            return;
        }
        // give the input back at once; the actor goes when the fade ends
        this._releaseGrab();
        this._box.ease({
            opacity: 0, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onStopped: () => { this.destroy(); },
        });
        host?.closed();
    }

    private _releaseGrab(): void {
        if (this._grab) {
            Main.popModal(this._grab);
            this._grab = null;
        }
        this._layer.reactive = false;
    }

    destroy(): void {
        if (this._destroyed)
            return;
        this._destroyed = true;
        this._host = null;
        this._releaseGrab();
        this._box.remove_all_transitions();
        if (this._layer.get_parent())
            Main.layoutManager.removeChrome(this._layer);
        this._layer.destroy();
        this._rows = [];
        this._groupStarts = [];
    }
}
