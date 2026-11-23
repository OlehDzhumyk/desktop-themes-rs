// SPDX-License-Identifier: GPL-3.0-or-later
// Skin "windows11": the unified Win+V / Win+. flyout. Opens next to the text caret (or the pointer
// when the focused app reports no caret), with live tabs on top (“Clipboard”, “Emoji”, “Symbols”),
// “Clear all”, and clipboard item cards with pin/delete actions, like Windows 11 does.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import St from 'gi://St';

import type {ClipItem} from '../dbus.js';
import {
    EMOJI_CATEGORIES, EmojiGrid, type EmojiPickHost, SYMBOL_CATEGORIES,
} from '../emoji/panel.js';
import {_} from '../i18n.js';
import {
    ClipboardPanel, HORIZONTAL, VERTICAL, iconButton, imageWidget, itemAccessibleName, label, pinIcon,
    type PanelHost,
} from './panel.js';

export type FlyoutTab = 'clipboard' | 'emoji' | 'symbols';

export class WinFlyout extends ClipboardPanel {
    private _tabButtons: Record<FlyoutTab, St.Button>;
    private _activeTab: FlyoutTab = 'clipboard';
    private _clipSection: St.BoxLayout;
    private _emojiSection: St.BoxLayout;
    private _symbolsSection: St.BoxLayout;
    private _emojiGrid: EmojiGrid | null = null;
    private _symbolsGrid: EmojiGrid | null = null;
    private _emojiHost: EmojiPickHost | null = null;
    private _scroll: St.ScrollView;
    private _list: St.BoxLayout;
    private _clearBtn: St.Button;
    private _status: St.Label;
    /** Id of the card whose «…» actions are shown (an id, so it does not move to another item). */
    private _expanded: ClipItem['id'] | null = null;

    constructor(host: PanelHost) {
        super(host, 'windows11', 'caret');

        const tabs = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-clip-tabs'});
        tabs.accessible_role = Atk.Role.PAGE_TAB_LIST;
        const makeTab = (id: FlyoutTab, text: string): St.Button => {
            const b = new St.Button({
                style_class: `desk-clip-tab${id === 'clipboard' ? ' active' : ''}`,
                label: text,
                reactive: id === 'clipboard',
                can_focus: id === 'clipboard',
                accessible_name: text,
            });
            b.accessible_role = Atk.Role.PAGE_TAB;
            if (id === 'clipboard')
                b.add_accessible_state(Atk.StateType.SELECTED);
            else
                b.add_style_pseudo_class('insensitive');
            b.connect('clicked', () => { this._selectTab(id); });
            tabs.add_child(b);
            return b;
        };
        this._tabButtons = {
            clipboard: makeTab('clipboard', _('Clipboard')),
            emoji: makeTab('emoji', _('Emoji')),
            symbols: makeTab('symbols', _('Symbols')),
        };
        this.box.add_child(tabs);

        this._clipSection = new St.BoxLayout({...VERTICAL});
        const head = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-clip-headline'});
        head.add_child(new St.Label({text: _('Clipboard'), style_class: 'desk-clip-title', x_expand: true,
            y_align: Clutter.ActorAlign.CENTER}));
        this._clearBtn = new St.Button({style_class: 'desk-clip-clear', label: _('Clear all'), can_focus: true});
        this._clearBtn.connect('clicked', () => {
            void this.host.client.clear(true).then(() => this.refresh()).catch((e: unknown) => { console.warn(e); });
        });
        head.add_child(this._clearBtn);
        this._clipSection.add_child(head);

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
        this._clipSection.add_child(this._scroll);
        this._status = new St.Label({style_class: 'desk-clip-empty', visible: false});
        this._clipSection.add_child(this._status);
        this.box.add_child(this._clipSection);

        this._emojiSection = new St.BoxLayout({...VERTICAL, visible: false});
        this.box.add_child(this._emojiSection);
        this._symbolsSection = new St.BoxLayout({...VERTICAL, visible: false});
        this.box.add_child(this._symbolsSection);
    }

    /** Connect or disconnect the emoji engine that backs the Emoji and Symbols tabs. */
    setEmojiHost(host: EmojiPickHost | null): void {
        if (this._emojiHost === host)
            return;
        this._emojiGrid?.destroy();
        this._emojiGrid = null;
        this._symbolsGrid?.destroy();
        this._symbolsGrid = null;
        this._emojiHost = host;

        const enabled = host !== null;
        for (const id of ['emoji', 'symbols'] as const) {
            const btn = this._tabButtons[id];
            btn.reactive = enabled;
            btn.can_focus = enabled;
            if (enabled)
                btn.remove_style_pseudo_class('insensitive');
            else
                btn.add_style_pseudo_class('insensitive');
        }
        if (host) {
            this._emojiGrid = new EmojiGrid(host, EMOJI_CATEGORIES, 7);
            this._emojiSection.add_child(this._emojiGrid.actor);
            this._symbolsGrid = new EmojiGrid(host, SYMBOL_CATEGORIES, 6);
            this._symbolsSection.add_child(this._symbolsGrid.actor);
        } else if (this._activeTab !== 'clipboard') {
            this._selectTab('clipboard');
        }
    }

    /** Open the flyout on a specific tab (or close it if already open on that tab). */
    openTab(tab: FlyoutTab): void {
        if (tab !== 'clipboard' && !this._emojiHost)
            return;
        if (this.isOpen && this._activeTab === tab) {
            this.close();
            return;
        }
        this._selectTab(tab);
        if (!this.isOpen)
            this.open();
        else
            this.focusInitial();
    }

    private _selectTab(tab: FlyoutTab): void {
        if (tab !== 'clipboard' && !this._emojiHost)
            return;
        this._activeTab = tab;
        for (const [id, btn] of Object.entries(this._tabButtons) as [FlyoutTab, St.Button][]) {
            if (id === tab) {
                btn.add_style_class_name('active');
                btn.add_accessible_state(Atk.StateType.SELECTED);
            } else {
                btn.remove_style_class_name('active');
                btn.remove_accessible_state(Atk.StateType.SELECTED);
            }
        }
        this._clipSection.visible = tab === 'clipboard';
        this._emojiSection.visible = tab === 'emoji';
        this._symbolsSection.visible = tab === 'symbols';
        if (tab === 'emoji') {
            this._emojiGrid?.reset();
            this._emojiGrid?.focusSearch();
        } else if (tab === 'symbols') {
            this._symbolsGrid?.reset();
            this._symbolsGrid?.focusSearch();
        } else {
            this.actor.grab_key_focus();
        }
    }

    protected override onOpen(): void {
        this._expanded = null;
        if (this._activeTab === 'emoji')
            this._emojiGrid?.reset();
        else if (this._activeTab === 'symbols')
            this._symbolsGrid?.reset();
    }

    protected override focusInitial(): void {
        if (this._activeTab === 'emoji')
            this._emojiGrid?.focusSearch();
        else if (this._activeTab === 'symbols')
            this._symbolsGrid?.focusSearch();
        else
            this.actor.grab_key_focus();
    }

    protected override handleKey(ev: Clutter.Event, sym: number, ctrl: boolean): boolean {
        if (this._activeTab === 'emoji')
            return this._emojiGrid?.handleKey(ev) ?? false;
        if (this._activeTab === 'symbols')
            return this._symbolsGrid?.handleKey(ev) ?? false;
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
        const card = new St.Button({style_class: 'desk-clip-card', x_expand: true, can_focus: true, reactive: true});
        card.accessible_role = Atk.Role.LIST_ITEM;
        card.accessible_name = itemAccessibleName(it);
        const inner = new St.BoxLayout({...HORIZONTAL, x_expand: true});
        const content = it.kind === 'image'
            ? imageWidget(it, 'desk-clip-card-image', 96)
            : label(it.text.length > 400 ? `${it.text.slice(0, 400)}…` : it.text, 'desk-clip-card-text', 3);
        content.x_expand = true;
        inner.add_child(content);

        const side = new St.BoxLayout({...VERTICAL, style_class: 'desk-clip-card-side', y_align: Clutter.ActorAlign.START});
        if (this._expanded === it.id) {
            side.add_child(iconButton('user-trash-symbolic', _('Delete'), 'desk-clip-card-button',
                () => void this.remove(it)));
            side.add_child(iconButton(pinIcon(it.pinned),
                it.pinned ? _('Unpin') : _('Pin'),
                `desk-clip-card-button${it.pinned ? ' checked' : ''}`, () => void this.togglePin(it)));
        } else {
            side.add_child(iconButton('view-more-horizontal-symbolic', _('More'), 'desk-clip-card-button', () => {
                this._expanded = it.id;
                this.render();
                this.highlight();
            }));
            const pin = iconButton(pinIcon(it.pinned), it.pinned ? _('Unpin') : _('Pin'),
                `desk-clip-card-button pin${it.pinned ? ' checked' : ''}`, () => void this.togglePin(it));
            side.add_child(pin);
        }
        inner.add_child(side);
        card.set_child(inner);
        card.connect('clicked', () => { this.activate(it); });
        // Tab / focus moves the selection too, so Enter, Delete and Ctrl+P act on the focused card
        card.connect('key-focus-in', () => {
            if (this.selected !== index) {
                this.selected = index;
                this.highlight();
            }
        });
        card.connect('notify::hover', () => {
            if (card.hover && this.selected !== index) {
                this.selected = index;
                this.highlight();
            }
        });
        return card;
    }

    override destroy(): void {
        this._emojiGrid = null;
        this._symbolsGrid = null;
        this._emojiHost = null;
        super.destroy();
    }
}
