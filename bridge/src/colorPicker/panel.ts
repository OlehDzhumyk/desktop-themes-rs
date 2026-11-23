// SPDX-License-Identifier: GPL-3.0-or-later
// History of picked colours (docs/spec/features/color-picker.md, «History panel»): swatches with
// the value and the nearest colour name, click or Enter copies, Delete removes, a format selector,
// «Clear», and the WCAG contrast ratio of the two newest picks. One modal panel for all skins; the
// stylesheet gives it the skin's look (windows11: a flyout at the bottom right; macos, default: a
// popover at the pointer).
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {HORIZONTAL, VERTICAL, animationsEnabled, iconButton, label, syncSystemClasses} from '../ui/primitives.js';
import {_, fmt} from '../i18n.js';
import {errorText, pushModal} from '../shellTypes.js';
import {type ColorFormat, type ColorItem, type ColorPickerClient, FORMATS, asFormat} from './client.js';
import {formatLabel} from './overlay.js';

export interface HistoryHost {
    readonly client: ColorPickerClient;
    readonly settings: Gio.Settings;
    /** Put the text on the clipboard and say so. */
    copy(text: string): void;
    /** Start the eyedropper. */
    startPicking(): void;
}

/** Long, translated names of the formats (the same texts as the settings page). */
function formatName(f: ColorFormat): string {
    switch (f) {
    case 'hex': return _('HEX (#RRGGBB)');
    case 'rgb': return _('RGB (rgb(r, g, b))');
    case 'hsl': return _('HSL (hsl(h, s%, l%))');
    case 'hsv': return _('HSV (hsv(h, s%, v%))');
    case 'name': return _('CSS name when exact, otherwise HEX');
    case 'gtk': return _('GTK @define-color line');
    case 'decimal': return _('Decimal (r,g,b)');
    }
}

/** The WCAG 2.x level a contrast ratio passes, for normal text. */
function wcagLevel(ratio: number): string {
    if (ratio >= 7)
        return _('passes AAA');
    if (ratio >= 4.5)
        return _('passes AA');
    if (ratio >= 3)
        return _('passes AA for large text only');
    return _('fails AA');
}

export class HistoryPanel {
    readonly actor: St.Widget;
    private _box: St.BoxLayout;
    private _formatButtons = new Map<ColorFormat, St.Button>();
    private _scroll: St.ScrollView;
    private _list: St.BoxLayout;
    private _empty: St.Label;
    private _contrast: St.Label;
    private _clear: St.Button;
    private _items: ColorItem[] = [];
    private _rows: St.Button[] = [];
    private _selected = 0;
    private _grab: Clutter.Grab | null = null;
    private _serial = 0;
    private _formatId: number;
    private _gone = false;

    constructor(private _host: HistoryHost, private _skin: string) {
        this.actor = new St.Widget({reactive: true, visible: false, style_class: `desk-clip-backdrop desk-skin-${_skin}`});
        this._box = new St.BoxLayout({
            ...VERTICAL,
            style_class: `desk-color-panel desk-skin-${_skin}${_skin === 'default' ? ' popup-menu-content' : ''}`,
            reactive: true,
        });
        this._box.accessible_role = Atk.Role.DIALOG;
        this._box.accessible_name = _('Picked colours');

        const header = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-color-header'});
        header.add_child(label(_('Picked colours'), 'desk-color-title'));
        header.add_child(iconButton('color-select-symbolic', _('Pick a colour from the screen'),
            'desk-color-flat-button', () => {
                this.close();
                this._host.startPicking();
            }));
        this._box.add_child(header);

        const formats = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-color-formats'});
        formats.accessible_role = Atk.Role.TOOL_BAR;
        formats.accessible_name = _('Format');
        for (const f of FORMATS) {
            const b = new St.Button({
                style_class: 'desk-color-format',
                label: formatLabel(f),
                can_focus: true,
                toggle_mode: true,
                accessible_name: formatName(f),
            });
            b.connect('clicked', () => { this._host.settings.set_string('format', f); });
            this._formatButtons.set(f, b);
            formats.add_child(b);
        }
        this._box.add_child(formats);

        this._list = new St.BoxLayout({...VERTICAL, style_class: 'desk-color-list'});
        this._list.accessible_role = Atk.Role.LIST;
        this._list.accessible_name = _('Picked colours');
        this._scroll = new St.ScrollView({
            style_class: 'desk-color-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            overlay_scrollbars: true,
            child: this._list,
        });
        this._box.add_child(this._scroll);
        this._empty = new St.Label({style_class: 'desk-color-empty', visible: false});
        this._box.add_child(this._empty);

        const footer = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-color-footer'});
        this._contrast = label('', 'desk-color-contrast');
        footer.add_child(this._contrast);
        this._clear = new St.Button({style_class: 'desk-color-clear', label: _('Clear'), can_focus: true});
        this._clear.connect('clicked', () => {
            this._host.client.clear().catch((e: unknown) => {
                console.warn(`desk-bridge: colour picker Clear: ${errorText(e)}`);
            });
        });
        footer.add_child(this._clear);
        this._box.add_child(footer);
        this.actor.add_child(this._box);

        this._formatId = _host.settings.connect('changed::format', () => { this._syncFormat(); });
        this._syncFormat();

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

    private _alive(): boolean {
        return !this._gone;
    }

    get isOpen(): boolean {
        return !this._gone && this.actor.visible;
    }

    open(): void {
        if (this.isOpen)
            return;
        const mon = Main.layoutManager.currentMonitor;
        if (!mon)
            return;
        this.actor.set_position(mon.x, mon.y);
        this.actor.set_size(mon.width, mon.height);
        const [px, py] = global.get_pointer();
        this.actor.show();
        this._grab = pushModal(this.actor, Shell.ActionMode.POPUP);
        if (!this._grab) {
            this.actor.hide();
            return;
        }
        syncSystemClasses(this._box);
        this._selected = 0;
        this._place(px, py);
        void this.refresh().then(() => {
            if (this.isOpen) {
                this._place(px, py);
                (this._rows[0] ?? this._formatButtons.get(this._format()))?.grab_key_focus();
            }
        });
        this._box.remove_all_transitions();
        if (animationsEnabled()) {
            this._box.opacity = 0;
            this._box.ease({opacity: 255, duration: 80, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        } else {
            this._box.opacity = 255;
        }
    }

    close(): void {
        if (!this.isOpen)
            return;
        if (this._grab)
            Main.popModal(this._grab);
        this._grab = null;
        this.actor.hide();
    }

    private _format(): ColorFormat {
        return asFormat(this._host.settings.get_string('format'));
    }

    private _syncFormat(): void {
        const current = this._format();
        for (const [f, b] of this._formatButtons) {
            b.checked = f === current;
            if (b.checked)
                b.add_accessible_state(Atk.StateType.CHECKED);
            else
                b.remove_accessible_state(Atk.StateType.CHECKED);
        }
    }

    async refresh(): Promise<void> {
        const serial = ++this._serial;
        let items: ColorItem[] = [];
        let ratio = 0;
        let failed = false;
        try {
            [items, ratio] = await Promise.all([this._host.client.list(), this._host.client.contrast()]);
        } catch (e) {
            failed = true;
            console.warn(`desk-bridge: colour picker List: ${errorText(e)}`);
        }
        // a newer refresh ran meanwhile, or the actor is gone (destroy, or the stage at logout)
        if (serial !== this._serial || !this._alive())
            return;
        this._items = items;
        if (this._selected >= items.length)
            this._selected = Math.max(0, items.length - 1);
        this._render(failed, ratio);
    }

    private _render(failed: boolean, ratio: number): void {
        const hadFocus = this._rows.some(r => r === global.stage.get_key_focus());
        this._list.destroy_all_children();
        this._rows = [];
        const has = this._items.length > 0;
        this._scroll.visible = has;
        this._empty.visible = !has;
        this._clear.reactive = has;
        this._clear.can_focus = has;
        if (has)
            this._clear.remove_style_pseudo_class('insensitive');
        else
            this._clear.add_style_pseudo_class('insensitive');
        if (failed)
            this._empty.text = _('deskd is unavailable');
        else if (this._host.settings.get_uint('history-size') === 0)
            this._empty.text = _('The history is off: history size is 0');
        else
            this._empty.text = _('No colours picked yet');
        this._contrast.visible = ratio > 0;
        this._contrast.text = ratio > 0
            ? fmt(_('Contrast of the last two picks: {ratio}:1, {level}'), {ratio: ratio.toFixed(2), level: wcagLevel(ratio)})
            : '';

        this._items.forEach((it, i) => {
            const row = new St.Button({style_class: 'desk-color-row', can_focus: true, x_expand: true});
            row.accessible_role = Atk.Role.LIST_ITEM;
            row.accessible_name = fmt(_('{value}, {name}'), {value: it.formatted, name: it.name});
            const content = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-color-row-box', x_expand: true});
            content.add_child(new St.Widget({
                style_class: 'desk-color-swatch', style: `background-color: ${it.hex};`,
                y_align: Clutter.ActorAlign.CENTER,
            }));
            content.add_child(label(it.formatted, 'desk-color-row-value'));
            const name = label(it.name, 'desk-color-row-name');
            name.x_expand = false;
            content.add_child(name);
            content.add_child(iconButton('edit-delete-symbolic', fmt(_('Remove {value}'), {value: it.formatted}),
                'desk-color-flat-button', () => { this._remove(it); }));
            row.set_child(content);
            row.connect('clicked', () => { this._copy(it); });
            row.connect('key-focus-in', () => {
                this._selected = i;
                this._highlight();
            });
            this._list.add_child(row);
            this._rows.push(row);
        });
        this._highlight();
        if (hadFocus)
            (this._rows[this._selected] ?? this._formatButtons.get(this._format()))?.grab_key_focus();
    }

    private _highlight(): void {
        this._rows.forEach((r, i) => {
            if (i === this._selected) {
                r.add_style_pseudo_class('selected');
                r.add_accessible_state(Atk.StateType.SELECTED);
            } else {
                r.remove_style_pseudo_class('selected');
                r.remove_accessible_state(Atk.StateType.SELECTED);
            }
        });
        const row = this._rows[this._selected];
        if (row) {
            const adj = this._scroll.vadjustment;
            const box = row.get_allocation_box();
            if (box.y1 < adj.value)
                adj.value = box.y1;
            else if (box.y2 > adj.value + adj.page_size)
                adj.value = box.y2 - adj.page_size;
        }
    }

    private _copy(it: ColorItem): void {
        this.close();
        this._host.copy(it.formatted);
    }

    private _remove(it: ColorItem): void {
        this._host.client.remove(it.hex).catch((e: unknown) => {
            console.warn(`desk-bridge: colour picker Remove: ${errorText(e)}`);
        });
    }

    private _move(delta: number): void {
        if (this._rows.length === 0)
            return;
        this._selected = Math.min(this._rows.length - 1, Math.max(0, this._selected + delta));
        this._rows[this._selected]?.grab_key_focus();
        this._highlight();
    }

    private _onKey(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const focus = global.stage.get_key_focus();
        const onRow = this._rows.some(r => r === focus);
        switch (sym) {
        case Clutter.KEY_Escape:
            this.close();
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Up:
            this._move(-1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Down:
            this._move(1);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Delete:
        case Clutter.KEY_KP_Delete: {
            const it = this._items[this._selected];
            if (it)
                this._remove(it);
            return Clutter.EVENT_STOP;
        }
        case Clutter.KEY_Return:
        case Clutter.KEY_KP_Enter: {
            // on a button (format, Clear, pick) Enter presses it; on the list it copies
            const it = this._items[this._selected];
            if (onRow && it) {
                this._copy(it);
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        }
        case Clutter.KEY_Tab:
        case Clutter.KEY_ISO_Left_Tab: {
            const back = sym === Clutter.KEY_ISO_Left_Tab ||
                (ev.get_state() & Clutter.ModifierType.SHIFT_MASK) !== 0;
            this._box.navigate_focus(focus, back ? St.DirectionType.TAB_BACKWARD : St.DirectionType.TAB_FORWARD, true);
            return Clutter.EVENT_STOP;
        }
        default:
            return Clutter.EVENT_PROPAGATE;
        }
    }

    private _place(px: number, py: number): void {
        const mon = Main.layoutManager.currentMonitor;
        if (!mon)
            return;
        const [, natW] = this._box.get_preferred_width(-1);
        const [, natH] = this._box.get_preferred_height(natW);
        const work = Main.layoutManager.getWorkAreaForMonitor(mon.index);
        const margin = 12;
        let x: number;
        let y: number;
        if (this._skin === 'windows11') {
            // a flyout above the bottom right corner, as the Windows 11 tray flyouts
            x = work.x + work.width - natW - margin;
            y = work.y + work.height - natH - margin;
        } else {
            x = px - natW / 2;
            y = py + 12;
            if (y + natH > work.y + work.height - margin)
                y = py - natH - 12;
        }
        x = Math.min(Math.max(x, work.x + margin), work.x + work.width - natW - margin);
        y = Math.min(Math.max(y, work.y + margin), work.y + work.height - natH - margin);
        this._box.set_position(Math.round(x - mon.x), Math.round(y - mon.y));
    }

    destroy(): void {
        if (this._formatId)
            this._host.settings.disconnect(this._formatId);
        this._formatId = 0;
        if (this._gone)
            return;
        this.close();
        Main.layoutManager.removeChrome(this.actor);
        this.actor.destroy();
    }
}
