// SPDX-License-Identifier: GPL-3.0-or-later
// Base of the clipboard panels: a modal surface over the current monitor with a list of items that
// deskd returns, keyboard navigation and activation (paste). Skins subclass it for layout and style.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Pango from 'gi://Pango';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import type {ClipItem, ClipboardClient} from '../dbus.js';

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

/** Relative time in Ukrainian, short. */
export function ago(unix: number): string {
    const s = Math.max(0, Math.floor(Date.now() / 1000) - unix);
    if (s < 60)
        return 'щойно';
    if (s < 3600)
        return `${Math.floor(s / 60)} хв тому`;
    if (s < 86400)
        return `${Math.floor(s / 3600)} год тому`;
    return `${Math.floor(s / 86400)} д тому`;
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

export function iconButton(icon: string, accessible: string, styleClass: string, onClick: () => void): St.Button {
    const b = new St.Button({
        style_class: styleClass,
        can_focus: true,
        accessible_name: accessible,
        child: new St.Icon({icon_name: icon, style_class: 'desk-clip-button-icon'}),
    });
    b.connect('clicked', () => onClick());
    return b;
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

    protected constructor(protected host: PanelHost, protected skin: string, protected anchor: Anchor) {
        this.actor = new St.Widget({
            reactive: true,
            visible: false,
            style_class: `desk-clip-backdrop desk-skin-${skin}`,
        });
        this.box = new St.BoxLayout({...VERTICAL, style_class: `desk-clip-panel desk-skin-${skin}`, reactive: true});
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
        Main.layoutManager.addTopChrome(this.actor);
    }

    get isOpen(): boolean {
        return this.actor.visible;
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
        const anchor = this._anchorPoint();
        this.actor.show();
        this._grab = Main.pushModal(this.actor, {actionMode: Shell.ActionMode.POPUP});
        if (!this._grab) {
            this.actor.hide();
            return;
        }
        this.selected = 0;
        this.onOpen();
        void this.refresh().then(() => this._place(anchor));
        this._place(anchor);
        this.focusInitial();
    }

    /** Called before the first refresh (reset search etc.). */
    protected onOpen(): void {}

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
            if (serial !== this._refreshSerial)
                return;
            this.items = items;
        } catch (e) {
            if (serial !== this._refreshSerial)
                return;
            this.items = [];
            console.warn(`desk-bridge: deskd List failed: ${e}`);
        }
        if (this.selected >= this.items.length)
            this.selected = Math.max(0, this.items.length - 1);
        this.render();
        this.highlight();
    }

    protected highlight(): void {
        this.rows.forEach((r, i) => {
            if (i === this.selected)
                r.add_style_pseudo_class('selected');
            else
                r.remove_style_pseudo_class('selected');
        });
        const row = this.rows[this.selected];
        if (row)
            this.scrollTo(row);
    }

    /** Scroll a row into view (skins with a St.ScrollView override). */
    protected scrollTo(_row: St.Widget): void {}

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
        case Clutter.KEY_Return:
        case Clutter.KEY_KP_Enter:
            this.activateSelected();
            return Clutter.EVENT_STOP;
        }
        return Clutter.EVENT_PROPAGATE;
    }

    /** Point the panel is placed at: text caret (input method), pointer, or monitor geometry. */
    private _anchorPoint(): [number, number] {
        const mon = Main.layoutManager.currentMonitor!;
        if (this.anchor === 'caret') {
            // GNOME's input method keeps the caret rectangle the focused client reported
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            const r = (Main.inputMethod as any)?._cursorRect;
            if (r && r.x >= mon.x && r.y >= mon.y && r.x < mon.x + mon.width && r.y < mon.y + mon.height &&
                (r.x || r.y))
                return [r.x, r.y + (r.height || 16) + 6];
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
        this.close();
        Main.layoutManager.removeChrome(this.actor);
        this.actor.destroy();
    }
}

export function logError(e: unknown): void {
    console.warn(`desk-bridge: ${e}`);
}

/** Run `f` once the main loop is idle (after the current event is done). */
export function idle(f: () => void): void {
    GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
        f();
        return GLib.SOURCE_REMOVE;
    });
}
