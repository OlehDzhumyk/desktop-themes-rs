// SPDX-License-Identifier: GPL-3.0-or-later
// Skin "windows11": the Win+V flyout. Opens next to the text caret (or the pointer when the focused
// app reports no caret), tabs on top (“Clipboard”; “Emoji” and “Symbols” are reserved for a
// later stage and shown disabled), “Clear all”, item cards with a pin button and a “…” button
// that reveals pin/delete actions, like Windows 11 does.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import St from 'gi://St';

import type {ClipItem} from '../dbus.js';
import {_} from '../i18n.js';
import {
    ClipboardPanel, HORIZONTAL, VERTICAL, iconButton, imageWidget, itemAccessibleName, label, type PanelHost,
} from './panel.js';

export class WinFlyout extends ClipboardPanel {
    private _scroll: St.ScrollView;
    private _list: St.BoxLayout;
    private _clearBtn: St.Button;
    private _status: St.Label;
    private _expanded = -1;

    constructor(host: PanelHost) {
        super(host, 'windows11', 'caret');

        const tabs = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-clip-tabs'});
        const tab = (text: string, active: boolean, tip: string) => {
            const b = new St.Button({
                style_class: `desk-clip-tab${active ? ' active' : ''}`,
                label: text,
                reactive: active,
                can_focus: active,
                accessible_name: tip,
            });
            b.accessible_role = Atk.Role.PAGE_TAB;
            if (active)
                b.add_accessible_state(Atk.StateType.SELECTED);
            if (!active)
                b.add_style_pseudo_class('insensitive');
            tabs.add_child(b);
        };
        tab(_('Clipboard'), true, _('Clipboard'));
        tab(_('Emoji'), false, _('Emoji: coming later'));
        tab(_('Symbols'), false, _('Symbols: coming later'));
        this.box.add_child(tabs);

        const head = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-clip-headline'});
        head.add_child(new St.Label({text: _('Clipboard'), style_class: 'desk-clip-title', x_expand: true,
            y_align: Clutter.ActorAlign.CENTER}));
        this._clearBtn = new St.Button({style_class: 'desk-clip-clear', label: _('Clear all'), can_focus: true});
        this._clearBtn.connect('clicked', () => {
            void this.host.client.clear(true).then(() => this.refresh()).catch(e => console.warn(e));
        });
        head.add_child(this._clearBtn);
        this.box.add_child(head);

        this._list = new St.BoxLayout({...VERTICAL, style_class: 'desk-clip-cards'});
        this._list.accessible_role = Atk.Role.LIST;
        this._list.accessible_name = _('Clipboard history');
        this._scroll = new St.ScrollView({
            style_class: 'desk-clip-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            overlay_scrollbars: true,
            child: this._list,
        });
        this.box.add_child(this._scroll);
        this._status = new St.Label({style_class: 'desk-clip-empty', visible: false});
        this.box.add_child(this._status);
    }

    protected override onOpen(): void {
        this._expanded = -1;
    }

    protected override focusInitial(): void {
        this.actor.grab_key_focus();
    }

    protected override handleKey(_ev: Clutter.Event, sym: number, ctrl: boolean): boolean {
        const it = this.items[this.selected];
        if (!it)
            return false;
        if (sym === Clutter.KEY_Delete) {
            void this.remove(it);
            return true;
        }
        if (ctrl && (sym === Clutter.KEY_p || sym === Clutter.KEY_P)) {
            void this.togglePin(it);
            return true;
        }
        return false;
    }

    protected override scrollTo(row: St.Widget): void {
        const adj = this._scroll.vadjustment;
        const box = row.get_allocation_box();
        if (box.y1 < adj.value)
            adj.value = box.y1;
        else if (box.y2 > adj.value + adj.page_size)
            adj.value = box.y2 - adj.page_size;
    }

    protected override render(): void {
        for (const r of this.rows)
            r.destroy();
        this.rows = [];
        const priv = this.host.client.private;
        this._clearBtn.visible = this.items.some(i => !i.pinned);
        this._status.visible = this.items.length === 0;
        this._status.text = priv
            ? _('Private mode is on: copies are not kept.')
            : _('Copied items will show up here.\nPress Ctrl+C to copy.');
        this._scroll.visible = this.items.length > 0;
        this.items.forEach((it, i) => {
            const card = this._card(it, i);
            this._list.add_child(card);
            this.rows.push(card);
        });
    }

    private _card(it: ClipItem, index: number): St.Widget {
        const card = new St.Button({style_class: 'desk-clip-card', x_expand: true, can_focus: false, reactive: true});
        card.accessible_role = Atk.Role.LIST_ITEM;
        card.accessible_name = itemAccessibleName(it);
        const inner = new St.BoxLayout({...HORIZONTAL, x_expand: true});
        const content = it.kind === 'image'
            ? imageWidget(it, 'desk-clip-card-image', 96)
            : label(it.text.length > 400 ? `${it.text.slice(0, 400)}…` : it.text, 'desk-clip-card-text', 3);
        content.x_expand = true;
        inner.add_child(content);

        const side = new St.BoxLayout({...VERTICAL, style_class: 'desk-clip-card-side', y_align: Clutter.ActorAlign.START});
        if (this._expanded === index) {
            side.add_child(iconButton('user-trash-symbolic', _('Delete'), 'desk-clip-card-button',
                () => void this.remove(it)));
            side.add_child(iconButton(it.pinned ? 'view-pin-symbolic' : 'view-pin-symbolic',
                it.pinned ? _('Unpin') : _('Pin'),
                `desk-clip-card-button${it.pinned ? ' checked' : ''}`, () => void this.togglePin(it)));
        } else {
            side.add_child(iconButton('view-more-horizontal-symbolic', _('More'), 'desk-clip-card-button', () => {
                this._expanded = index;
                this.render();
                this.highlight();
            }));
            const pin = iconButton('view-pin-symbolic', it.pinned ? _('Unpin') : _('Pin'),
                `desk-clip-card-button pin${it.pinned ? ' checked' : ''}`, () => void this.togglePin(it));
            side.add_child(pin);
        }
        inner.add_child(side);
        card.set_child(inner);
        card.connect('clicked', () => this.activate(it));
        card.connect('notify::hover', () => {
            if (card.hover && this.selected !== index) {
                this.selected = index;
                this.highlight();
            }
        });
        return card;
    }
}
