// SPDX-License-Identifier: GPL-3.0-or-later
// Search-and-list clipboard panel. Skin "macos": a Spotlight-like panel centred on the screen
// (search field, list, preview of the selected item, keyboard hints), opened with Shift+Super+V.
// Skin "default": the same list as a compact popup at the pointer, like a menu.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import type {ClipItem} from '../dbus.js';
import {_, fmt} from '../i18n.js';
import {
    ClipboardPanel, HORIZONTAL, VERTICAL, ago, iconButton, imageWidget, itemAccessibleName, label, oneLine,
    type PanelHost,
} from './panel.js';

export class ListPanel extends ClipboardPanel {
    private _entry: St.Entry;
    private _scroll: St.ScrollView;
    private _list: St.BoxLayout;
    private _preview: St.BoxLayout | null = null;
    private _empty: St.Label;
    private _searchId = 0;
    private _full: boolean;

    constructor(host: PanelHost, skin: 'macos' | 'default') {
        super(host, skin, skin === 'macos' ? 'center' : 'pointer');
        this._full = skin === 'macos';
        // the default skin takes its colours from the active shell theme (popup menu classes)
        if (!this._full)
            this.box.add_style_class_name('popup-menu-content');

        this._entry = new St.Entry({
            style_class: 'desk-clip-search',
            hint_text: this._full ? _('Search the clipboard') : _('Search'),
            can_focus: true,
            x_expand: true,
        });
        this._entry.clutter_text.accessible_name = _('Search the clipboard');
        this._entry.set_primary_icon(new St.Icon({icon_name: 'edit-find-symbolic', style_class: 'desk-clip-search-icon'}));
        this._entry.clutter_text.connect('text-changed', () => { this._queueSearch(); });
        // Enter in the search field (the input method may consume the key event itself)
        this._entry.clutter_text.connect('activate', () => { this.activateSelected(); });
        this._entry.clutter_text.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            // arrows, Enter and shortcuts go to the list even while typing
            return this.onKey(ev);
        });
        const top = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-clip-header'});
        top.add_child(this._entry);
        if (this._full) {
            top.add_child(iconButton('emblem-system-symbolic', _('Clipboard settings'),
                'desk-clip-flat-button', () => {
                    this.close();
                    this.host.openSettings();
                }));
        }
        this.box.add_child(top);

        const body = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-clip-body', y_expand: true});
        this._list = new St.BoxLayout({...VERTICAL, style_class: 'desk-clip-list'});
        this._list.accessible_role = Atk.Role.LIST;
        this._list.accessible_name = _('Clipboard history');
        this._scroll = new St.ScrollView({
            style_class: 'desk-clip-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            overlay_scrollbars: true,
            x_expand: true,
            child: this._list,
        });
        body.add_child(this._scroll);
        if (this._full) {
            this._preview = new St.BoxLayout({...VERTICAL, style_class: 'desk-clip-preview'});
            body.add_child(this._preview);
        }
        this.box.add_child(body);
        this._empty = new St.Label({style_class: 'desk-clip-empty', text: '', visible: false});
        this._list.add_child(this._empty);

        if (this._full) {
            const hints = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-clip-hints'});
            const keys: [string, string][] = [['↵', _('paste')], ['Ctrl+P', _('pin')], ['Ctrl+⌫', _('delete')], ['Esc', _('close')]];
            for (const [k, t] of keys) {
                hints.add_child(new St.Label({text: k, style_class: 'desk-clip-key'}));
                hints.add_child(new St.Label({text: t, style_class: 'desk-clip-hint'}));
            }
            this.box.add_child(hints);
        }
    }

    protected override query(): string {
        return this._entry.get_text();
    }

    protected override limit(): number {
        return this._full ? 0 : 12;
    }

    protected override onOpen(): void {
        this._entry.set_text('');
    }

    protected override focusInitial(): void {
        this._entry.grab_key_focus();
    }

    private _queueSearch(): void {
        if (this._searchId)
            GLib.source_remove(this._searchId);
        this._searchId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 80, () => {
            this._searchId = 0;
            this.selected = 0;
            void this.refresh();
            return GLib.SOURCE_REMOVE;
        });
    }

    protected override handleKey(_ev: Clutter.Event, sym: number, ctrl: boolean): boolean {
        const it = this.items[this.selected];
        if (ctrl && (sym === Clutter.KEY_p || sym === Clutter.KEY_P) && it) {
            void this.togglePin(it);
            return true;
        }
        if (ctrl && (sym === Clutter.KEY_BackSpace || sym === Clutter.KEY_Delete) && it) {
            void this.remove(it);
            return true;
        }
        return false;
    }

    protected override render(): void {
        for (const r of this.rows)
            r.destroy();
        this.rows = [];
        const q = this.query().trim();
        this._empty.visible = this.items.length === 0;
        this._empty.text = q ? fmt(_('Nothing found for “{query}”'), {query: q})
            : this.host.client.private ? _('Private mode: nothing is recorded')
                : _('The history is empty. Copy something (Ctrl+C) and it shows up here.');
        this.items.forEach((it, i) => {
            const row = this._row(it, i);
            this._list.add_child(row);
            this.rows.push(row);
        });
        this._updatePreview();
    }

    protected override highlight(): void {
        super.highlight();
        this._updatePreview();
    }

    protected override scrollTo(row: St.Widget): void {
        const adj = this._scroll.vadjustment;
        const box = row.get_allocation_box();
        if (box.y1 < adj.value)
            adj.value = box.y1;
        else if (box.y2 > adj.value + adj.page_size)
            adj.value = box.y2 - adj.page_size;
    }

    private _row(it: ClipItem, index: number): St.Widget {
        const row = new St.Button({
            style_class: this._full ? 'desk-clip-row' : 'desk-clip-row popup-menu-item',
            can_focus: false, x_expand: true, reactive: true,
        });
        row.accessible_role = Atk.Role.LIST_ITEM;
        row.accessible_name = itemAccessibleName(it);
        const inner = new St.BoxLayout({...HORIZONTAL, x_expand: true});
        if (it.kind === 'image')
            inner.add_child(imageWidget(it, 'desk-clip-thumb', 28));
        else
            inner.add_child(new St.Icon({icon_name: 'text-x-generic-symbolic', style_class: 'desk-clip-kind'}));
        inner.add_child(label(oneLine(it.text, 160), 'desk-clip-text'));
        if (it.pinned)
            inner.add_child(new St.Icon({icon_name: 'view-pin-symbolic', style_class: 'desk-clip-pinned', accessible_name: _('pinned')}));
        if (this._full)
            inner.add_child(new St.Label({text: ago(it.time), style_class: 'desk-clip-time', y_align: Clutter.ActorAlign.CENTER}));
        row.set_child(inner);
        row.connect('clicked', () => { this.activate(it); });
        row.connect('notify::hover', () => {
            if (row.hover && this.selected !== index) {
                this.selected = index;
                this.highlight();
            }
        });
        return row;
    }

    private _updatePreview(): void {
        if (!this._preview)
            return;
        this._preview.destroy_all_children();
        const it = this.items[this.selected];
        if (!it) {
            this._preview.hide();
            return;
        }
        this._preview.show();
        if (it.kind === 'image') {
            this._preview.add_child(imageWidget(it, 'desk-clip-preview-image', 220));
        } else {
            const text = it.text.length > 1200 ? `${it.text.slice(0, 1200)}…` : it.text;
            const l = new St.Label({text, style_class: 'desk-clip-preview-text', y_expand: true});
            l.clutter_text.line_wrap = true;
            this._preview.add_child(l);
        }
        const meta = it.kind === 'image' ? it.text
            : fmt(_('{count} chars · {when}'), {count: it.text.length, when: ago(it.time)}) +
                (it.pinned ? ` · ${_('pinned')}` : '');
        this._preview.add_child(new St.Label({text: meta, style_class: 'desk-clip-preview-meta'}));
    }

    override destroy(): void {
        if (this._searchId)
            GLib.source_remove(this._searchId);
        this._searchId = 0;
        super.destroy();
    }
}
