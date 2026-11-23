// SPDX-License-Identifier: GPL-3.0-or-later
// Keyboard-first emoji and symbol picker UI:
// - EmojiGrid: reusable search + category strip + grid of buttons (used both in EmojiPanel and
//   inside the Windows 11 unified flyout's Emoji and Symbols tabs).
// - EmojiPanel: modal floating picker for the `macos` and `default` skins, anchored near the
//   text caret (pointer fallback).
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {HORIZONTAL, VERTICAL, animationsEnabled} from '../clipboard/panel.js';
import {_, currentLanguage, fmt} from '../i18n.js';
import {errorText, inputMethod, pushModal} from '../shellTypes.js';
import type {EmojiClient, EmojiItem} from './client.js';

type Monitor = NonNullable<typeof Main.layoutManager.currentMonitor>;

export interface CategoryDef {
    id: string;
    glyph: string;
    label: () => string;
}

export const EMOJI_CATEGORIES: readonly CategoryDef[] = [
    {id: 'recents', glyph: '🕒', label: () => _('Recents')},
    {id: 'smileys', glyph: '😀', label: () => _('Smileys')},
    {id: 'people', glyph: '👋', label: () => _('People')},
    {id: 'animals', glyph: '🐶', label: () => _('Animals and nature')},
    {id: 'food', glyph: '🍕', label: () => _('Food and drink')},
    {id: 'travel', glyph: '🚀', label: () => _('Travel and places')},
    {id: 'activities', glyph: '🎉', label: () => _('Activities')},
    {id: 'objects', glyph: '💡', label: () => _('Objects')},
    {id: 'symbols', glyph: '❤️', label: () => _('Emoji symbols')},
    {id: 'flags', glyph: '🇺🇦', label: () => _('Flags')},
];

export const SYMBOL_CATEGORIES: readonly CategoryDef[] = [
    {id: 'arrows', glyph: '→', label: () => _('Arrows')},
    {id: 'math', glyph: '±', label: () => _('Math')},
    {id: 'currency', glyph: '₴', label: () => _('Currency')},
    {id: 'punctuation', glyph: '—', label: () => _('Punctuation')},
    {id: 'text-faces', glyph: 'ツ', label: () => _('Text faces')},
];

export const ALL_CATEGORIES: readonly CategoryDef[] = [
    ...EMOJI_CATEGORIES,
    ...SYMBOL_CATEGORIES,
];

export interface EmojiPickHost {
    client: EmojiClient;
    pickItem(item: EmojiItem, keepOpen: boolean): void;
}

/**
 * Reusable search + category bar + scrollable grid of emoji/symbol buttons.
 * Handles arrow-key navigation, Ctrl+1..9 category switching, and Enter / Shift+Enter insertion.
 */
export class EmojiGrid {
    readonly actor: St.BoxLayout;
    private _search: St.Entry;
    private _catsBox: St.BoxLayout;
    private _catButtons = new Map<string, St.Button>();
    private _scroll: St.ScrollView;
    private _rowsBox: St.BoxLayout;
    private _empty: St.Label;
    private _items: EmojiItem[] = [];
    private _cells: St.Button[] = [];
    private _selected = 0;
    private _activeCat: string;
    private _serial = 0;
    private _gone = false;
    private _columns: number;

    constructor(
        private _host: EmojiPickHost,
        private _categories: readonly CategoryDef[],
        columns = 8,
    ) {
        this._columns = columns;
        this._activeCat = _categories[0]?.id ?? 'smileys';
        this.actor = new St.BoxLayout({...VERTICAL, style_class: 'desk-emoji-body'});

        this._search = new St.Entry({
            style_class: 'desk-emoji-search',
            hint_text: _('Search emoji and symbols…'),
            can_focus: true,
            x_expand: true,
        });
        this._search.accessible_name = _('Search emoji and symbols');
        this._search.set_primary_icon(new St.Icon({
            icon_name: 'edit-find-symbolic',
            style_class: 'desk-clip-search-icon',
        }));
        this._search.clutter_text.connect('text-changed', () => {
            if (!this._gone)
                void this.refresh();
        });
        this.actor.add_child(this._search);

        this._catsBox = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-emoji-cats'});
        this._catsBox.accessible_role = Atk.Role.PAGE_TAB_LIST;
        for (const cat of this._categories) {
            const btn = new St.Button({
                style_class: 'desk-emoji-cat',
                label: cat.glyph,
                can_focus: true,
                accessible_name: cat.label(),
            });
            btn.accessible_role = Atk.Role.PAGE_TAB;
            btn.connect('clicked', () => {
                this._search.set_text('');
                this._activeCat = cat.id;
                this._updateCatHighlight();
                void this.refresh();
            });
            this._catButtons.set(cat.id, btn);
            this._catsBox.add_child(btn);
        }
        this.actor.add_child(this._catsBox);

        this._rowsBox = new St.BoxLayout({...VERTICAL, style_class: 'desk-emoji-grid'});
        this._rowsBox.accessible_role = Atk.Role.LIST;
        this._rowsBox.accessible_name = _('Emoji and symbols');
        this._scroll = new St.ScrollView({
            style_class: 'desk-emoji-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            overlay_scrollbars: true,
            child: this._rowsBox,
        });
        this.actor.add_child(this._scroll);

        this._empty = new St.Label({style_class: 'desk-emoji-empty', visible: false});
        this.actor.add_child(this._empty);

        this.actor.connect('destroy', () => {
            this._gone = true;
        });
        this._updateCatHighlight();
    }

    reset(): void {
        if (this._gone)
            return;
        this._search.set_text('');
        this._selected = 0;
        void this.refresh(true);
    }

    focusSearch(): void {
        if (!this._gone)
            this._search.grab_key_focus();
    }

    private _isGone(): boolean {
        return this._gone;
    }

    async refresh(initial = false): Promise<void> {
        if (this._isGone())
            return;
        const serial = ++this._serial;
        const query = this._search.get_text().trim();
        const lang = currentLanguage();
        let items: EmojiItem[];
        try {
            if (query.length > 0) {
                items = await this._host.client.search(query, lang, 120);
            } else if (this._activeCat === 'recents') {
                items = await this._host.client.recents();
                if (initial && items.length === 0) {
                    const fallback = this._categories.find(c => c.id !== 'recents');
                    if (fallback) {
                        this._activeCat = fallback.id;
                        this._updateCatHighlight();
                        items = await this._host.client.category(this._activeCat, 0);
                    }
                }
            } else {
                items = await this._host.client.category(this._activeCat, 0);
            }
        } catch (e) {
            if (serial !== this._serial || this._isGone())
                return;
            items = [];
            console.warn(`desk-bridge: deskd emoji query failed: ${errorText(e)}`);
        }
        if (serial !== this._serial || this._isGone())
            return;
        this._items = items;
        if (this._selected >= this._items.length)
            this._selected = Math.max(0, this._items.length - 1);
        this._render(query);
    }

    handleKey(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const state = ev.get_state();
        const ctrl = (state & Clutter.ModifierType.CONTROL_MASK) !== 0;
        const shift = (state & Clutter.ModifierType.SHIFT_MASK) !== 0;

        if (ctrl && sym >= Clutter.KEY_1 && sym <= Clutter.KEY_9) {
            const cat = this._categories[sym - Clutter.KEY_1];
            if (cat) {
                this._search.set_text('');
                this._activeCat = cat.id;
                this._updateCatHighlight();
                void this.refresh();
                return true;
            }
        }

        const cols = this._effectiveColumns();
        switch (sym) {
        case Clutter.KEY_Left:
            if (this._searchHasTextFocus())
                return false;
            this._move(-1);
            return true;
        case Clutter.KEY_Right:
            if (this._searchHasTextFocus())
                return false;
            this._move(1);
            return true;
        case Clutter.KEY_Up:
            this._move(-cols);
            return true;
        case Clutter.KEY_Down:
            this._move(cols);
            return true;
        case Clutter.KEY_Page_Up:
            this._move(-cols * 3);
            return true;
        case Clutter.KEY_Page_Down:
            this._move(cols * 3);
            return true;
        case Clutter.KEY_Home:
            if (this._searchHasTextFocus())
                return false;
            this._move(-this._items.length);
            return true;
        case Clutter.KEY_End:
            if (this._searchHasTextFocus())
                return false;
            this._move(this._items.length);
            return true;
        case Clutter.KEY_Return:
        case Clutter.KEY_KP_Enter: {
            const it = this._items[this._selected];
            if (it) {
                this._host.pickItem(it, shift);
                return true;
            }
            return false;
        }
        default:
            return false;
        }
    }

    private _searchHasTextFocus(): boolean {
        return global.stage.get_key_focus() === this._search.clutter_text &&
            this._search.get_text().length > 0;
    }

    private _effectiveColumns(): number {
        return this._activeCat === 'text-faces' && this._search.get_text().trim().length === 0
            ? Math.min(3, this._columns)
            : this._columns;
    }

    private _updateCatHighlight(): void {
        for (const [id, btn] of this._catButtons) {
            if (id === this._activeCat) {
                btn.add_style_class_name('active');
                btn.add_accessible_state(Atk.StateType.SELECTED);
            } else {
                btn.remove_style_class_name('active');
                btn.remove_accessible_state(Atk.StateType.SELECTED);
            }
        }
    }

    private _render(query: string): void {
        this._rowsBox.destroy_all_children();
        this._cells = [];
        const hasItems = this._items.length > 0;
        this._scroll.visible = hasItems;
        this._empty.visible = !hasItems;
        if (!hasItems) {
            this._empty.text = query.length > 0
                ? fmt(_('Nothing found for “{query}”'), {query})
                : _('No recent picks yet');
            return;
        }
        const cols = this._effectiveColumns();
        const wide = this._activeCat === 'text-faces' && query.length === 0;
        for (let i = 0; i < this._items.length; i += cols) {
            const row = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-emoji-row'});
            for (let j = 0; j < cols && i + j < this._items.length; j++) {
                const idx = i + j;
                const it = this._items[idx];
                if (!it)
                    continue;
                const cell = new St.Button({
                    style_class: wide || it.category === 'text-faces'
                        ? 'desk-emoji-cell desk-emoji-cell-wide'
                        : 'desk-emoji-cell',
                    label: it.glyph,
                    can_focus: true,
                    reactive: true,
                    accessible_name: it.name,
                });
                cell.connect('clicked', () => {
                    this._host.pickItem(it, false);
                });
                cell.connect('notify::hover', () => {
                    if (cell.hover && this._selected !== idx) {
                        this._selected = idx;
                        this._highlight();
                    }
                });
                row.add_child(cell);
                this._cells.push(cell);
            }
            this._rowsBox.add_child(row);
        }
        this._highlight();
    }

    private _move(delta: number): void {
        if (this._items.length === 0)
            return;
        this._selected = Math.min(this._items.length - 1, Math.max(0, this._selected + delta));
        this._highlight();
    }

    private _highlight(): void {
        this._cells.forEach((c, i) => {
            if (i === this._selected) {
                c.add_style_pseudo_class('selected');
                c.add_accessible_state(Atk.StateType.SELECTED);
            } else {
                c.remove_style_pseudo_class('selected');
                c.remove_accessible_state(Atk.StateType.SELECTED);
            }
        });
        const cell = this._cells[this._selected];
        if (cell) {
            const parent = cell.get_parent();
            if (parent) {
                const adj = this._scroll.vadjustment;
                const box = parent.get_allocation_box();
                if (box.y1 < adj.value)
                    adj.value = box.y1;
                else if (box.y2 > adj.value + adj.page_size)
                    adj.value = box.y2 - adj.page_size;
            }
        }
    }

    destroy(): void {
        if (!this._gone)
            this.actor.destroy();
    }
}

/**
 * Floating modal picker for the `macos` and `default` skins, anchored near the text caret
 * (pointer fallback).
 */
export class EmojiPanel {
    readonly actor: St.Widget;
    private _box: St.BoxLayout;
    private _grid: EmojiGrid;
    private _grab: Clutter.Grab | null = null;
    private _closing = false;
    private _gone = false;

    constructor(host: EmojiPickHost, skin: 'macos' | 'default') {
        this.actor = new St.Widget({
            reactive: true,
            visible: false,
            style_class: `desk-clip-backdrop desk-skin-${skin}`,
        });
        this._box = new St.BoxLayout({
            ...VERTICAL,
            style_class: `desk-emoji-panel desk-skin-${skin}${skin === 'default' ? ' popup-menu-content' : ''}`,
            reactive: true,
        });
        this._box.accessible_role = Atk.Role.DIALOG;
        this._box.accessible_name = _('Emoji and symbols');
        this._grid = new EmojiGrid(host, ALL_CATEGORIES, 8);
        this._box.add_child(this._grid.actor);
        this.actor.add_child(this._box);

        this.actor.connect('button-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            const [x, y] = ev.get_coords();
            const [bx, by] = this._box.get_transformed_position();
            const [bw, bh] = this._box.get_transformed_size();
            if (x < bx || y < by || x > bx + bw || y > by + bh) {
                this.close();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
        this.actor.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._onKey(ev));
        this.actor.connect('destroy', () => {
            this._gone = true;
            this._grab = null;
        });
        Main.layoutManager.addTopChrome(this.actor);
    }

    get isOpen(): boolean {
        return !this._gone && this.actor.visible;
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
        this._syncSystemClasses();
        this._grid.reset();
        this._place(anchor);
        this._grid.focusSearch();
        this._box.remove_all_transitions();
        if (animationsEnabled()) {
            this._box.opacity = 0;
            this._box.ease({opacity: 255, duration: 90, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        } else {
            this._box.opacity = 255;
        }
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

    private _onKey(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        if (sym === Clutter.KEY_Escape) {
            this.close();
            return Clutter.EVENT_STOP;
        }
        if (sym === Clutter.KEY_Tab || sym === Clutter.KEY_ISO_Left_Tab) {
            const back = sym === Clutter.KEY_ISO_Left_Tab ||
                (ev.get_state() & Clutter.ModifierType.SHIFT_MASK) !== 0;
            const from = global.stage.get_key_focus();
            this._box.navigate_focus(from, back ? St.DirectionType.TAB_BACKWARD : St.DirectionType.TAB_FORWARD, true);
            return Clutter.EVENT_STOP;
        }
        if (this._grid.handleKey(ev))
            return Clutter.EVENT_STOP;
        return Clutter.EVENT_PROPAGATE;
    }

    private _syncSystemClasses(): void {
        const st = St.Settings.get();
        const set = (cls: string, on: boolean) => {
            if (on)
                this._box.add_style_class_name(cls);
            else
                this._box.remove_style_class_name(cls);
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

    private _anchorPoint(mon: Monitor): [number, number] {
        const r = inputMethod()._cursorRect;
        if (r && r.x >= mon.x && r.y >= mon.y && r.x < mon.x + mon.width && r.y < mon.y + mon.height &&
            (r.x !== 0 || r.y !== 0))
            return [r.x, r.y + (r.height > 0 ? r.height : 16) + 6];
        const [x, y] = global.get_pointer();
        return [x, y + 8];
    }

    private _place([ax, ay]: [number, number]): void {
        const mon = Main.layoutManager.currentMonitor;
        if (!mon)
            return;
        const [, natW] = this._box.get_preferred_width(-1);
        const [, natH] = this._box.get_preferred_height(natW);
        const work = Main.layoutManager.getWorkAreaForMonitor(mon.index);
        const margin = 12;
        let x = ax - natW / 2;
        let y = ay;
        if (y + natH > work.y + work.height - margin)
            y = Math.max(work.y + margin, ay - natH - 40);
        x = Math.min(Math.max(x, work.x + margin), work.x + work.width - natW - margin);
        y = Math.min(Math.max(y, work.y + margin), work.y + work.height - natH - margin);
        this._box.set_position(Math.round(x - mon.x), Math.round(y - mon.y));
    }

    destroy(): void {
        if (this._gone)
            return;
        this.close();
        Main.layoutManager.removeChrome(this.actor);
        this.actor.destroy();
    }
}
