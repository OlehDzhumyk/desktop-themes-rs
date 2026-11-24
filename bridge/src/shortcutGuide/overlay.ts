// SPDX-License-Identifier: GPL-3.0-or-later
// Skin-aware Shortcut Guide overlay (docs/spec/features/shortcut-guide.md):
//   windows11  dimmed full-screen grid of columns by group, rounded square keycaps, neutral Super
//   macos      centred translucent cheat-sheet panel with search (in sticky mode) and ⌘ ⌥ ⇧ ⌃
//   default    same centred panel with plain-text keycaps
// mode 'hold' (Super held): no search; it holds a grab in Shell.ActionMode.NORMAL only to see the
// release of Super (mutter gives keys to the focused client otherwise), so shortcuts still run;
// any other key, a click or Esc closes it. mode 'sticky' (sticky=true or shortcut-guide-shortcut):
// modal in POPUP mode with live search and keyboard scrolling; Esc closes (AC-GUIDE-05).
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {type Skin} from '../accentPicker.js';
import {_} from '../i18n.js';
import {pushModal} from '../shellTypes.js';
import {HORIZONTAL, VERTICAL, animationsEnabled, label, syncSystemClasses} from '../ui/primitives.js';
import {type ShortcutEntry, type ShortcutGroup, type ShortcutSnapshot} from './reader.js';

const FADE_IN_MS = 100;
const SCROLL_STEP_PX = 48;
const PAGE_STEP_PX = 220;

export interface OverlayOpenOptions {
    readonly skin: Skin;
    readonly mode: 'hold' | 'sticky';
    readonly showOwner: boolean;
    readonly snapshot: ShortcutSnapshot;
    readonly onClosed: () => void;
}

function ownerText(entry: ShortcutEntry): string {
    return entry.owner === 'user' ? _('your setting') : _('theme');
}

function matchesQuery(entry: ShortcutEntry, groupTitle: string, query: string): boolean {
    if (!query)
        return true;
    const needle = query.toLowerCase();
    if (entry.action.toLowerCase().includes(needle))
        return true;
    if (groupTitle.toLowerCase().includes(needle))
        return true;
    if (ownerText(entry).toLowerCase().includes(needle))
        return true;
    for (const combo of entry.combos) {
        if (combo.join('+').toLowerCase().includes(needle))
            return true;
    }
    return false;
}

export class ShortcutGuideOverlay {
    private _backdrop: St.Widget | null = null;
    private _panel: St.BoxLayout | null = null;
    private _searchEntry: St.Entry | null = null;
    private _scrollView: St.ScrollView | null = null;
    private _contentBox: St.BoxLayout | null = null;
    private _grab: Clutter.Grab | null = null;
    private _backdropSignals: number[] = [];
    private _searchChangedId = 0;
    private _onClosed: (() => void) | null = null;
    private _snapshot: ShortcutSnapshot | null = null;
    private _skin: Skin = 'default';
    private _showOwner = true;
    private _sticky = false;
    private _open = false;

    get isOpen(): boolean {
        return this._open;
    }

    get isModal(): boolean {
        return this._open && this._sticky;
    }

    open(opts: OverlayOpenOptions): boolean {
        this.close(true);
        const monitor = Main.layoutManager.currentMonitor ?? Main.layoutManager.primaryMonitor;
        if (!monitor)
            return false;

        this._skin = opts.skin;
        this._showOwner = opts.showOwner;
        const sticky = opts.mode === 'sticky';
        this._sticky = sticky;
        this._snapshot = opts.snapshot;
        this._onClosed = opts.onClosed;

        const isWin11 = opts.skin === 'windows11';
        const backdrop = new St.Widget({
            style_class: `desk-guide-backdrop desk-skin-${opts.skin}`,
            reactive: true,
            can_focus: true,
            visible: false,
            x: monitor.x,
            y: monitor.y,
            width: monitor.width,
            height: monitor.height,
        });
        syncSystemClasses(backdrop);

        const panel = new St.BoxLayout({
            ...VERTICAL,
            style_class: `desk-guide-panel desk-skin-${opts.skin}${isWin11 ? ' desk-guide-fullscreen' : ''}`,
            reactive: true,
            accessible_role: Atk.Role.DIALOG,
            accessible_name: _('Shortcut guide'),
        });
        syncSystemClasses(panel);

        // Header
        const header = new St.BoxLayout({
            ...HORIZONTAL,
            style_class: 'desk-guide-header',
            x_expand: true,
        });
        const title = label(_('Shortcut guide'), 'desk-guide-title');
        header.add_child(title);
        const hintText = sticky
            ? _('Type to filter · Up/Down to scroll · Esc to close')
            : _('Release Super to close');
        const hint = new St.Label({
            text: hintText,
            style_class: 'desk-guide-hint',
            y_align: Clutter.ActorAlign.CENTER,
        });
        header.add_child(hint);
        panel.add_child(header);

        // Search entry (only in modal / sticky mode)
        if (sticky) {
            const entry = new St.Entry({
                style_class: 'desk-guide-search',
                hint_text: _('Search shortcuts…'),
                can_focus: true,
                x_expand: true,
            });
            entry.accessible_name = _('Search shortcuts');
            this._searchEntry = entry;
            this._searchChangedId = entry.clutter_text.connect('text-changed', () => {
                this._renderGroups(entry.get_text().trim());
            });
            panel.add_child(entry);
        }

        // Partial read warning banner (empty/error state)
        if (opts.snapshot.partial) {
            const notice = new St.Label({
                text: _('Some shortcuts are unavailable'),
                style_class: 'desk-guide-notice',
                x_expand: true,
            });
            panel.add_child(notice);
        }

        // Scrollable body
        const scrollView = new St.ScrollView({
            style_class: 'desk-guide-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.AUTOMATIC,
            x_expand: true,
            y_expand: true,
        });
        const contentBox = new St.BoxLayout({
            ...(isWin11 ? HORIZONTAL : VERTICAL),
            style_class: isWin11 ? 'desk-guide-columns' : 'desk-guide-list',
            x_expand: true,
        });
        scrollView.set_child(contentBox);
        panel.add_child(scrollView);

        backdrop.add_child(panel);
        this._backdrop = backdrop;
        this._panel = panel;
        this._scrollView = scrollView;
        this._contentBox = contentBox;

        this._renderGroups('');

        // Size and centre panel inside monitor work area
        const workArea = Main.layoutManager.getWorkAreaForMonitor(monitor.index);
        if (isWin11) {
            const padX = Math.round(workArea.width * 0.04);
            const padY = Math.round(workArea.height * 0.06);
            panel.set_size(
                Math.max(640, workArea.width - padX * 2),
                Math.max(420, workArea.height - padY * 2),
            );
            panel.set_position(
                workArea.x - monitor.x + padX,
                workArea.y - monitor.y + padY,
            );
        } else {
            const panelWidth = Math.min(720, Math.max(420, Math.round(workArea.width * 0.56)));
            const panelHeight = Math.min(640, Math.max(360, Math.round(workArea.height * 0.72)));
            panel.set_size(panelWidth, panelHeight);
            panel.set_position(
                workArea.x - monitor.x + Math.round((workArea.width - panelWidth) / 2),
                workArea.y - monitor.y + Math.round((workArea.height - panelHeight) / 2),
            );
        }

        // GNOME Shell 50: never pass {affectsInputRegion: false} to addTopChrome
        Main.layoutManager.addTopChrome(backdrop);
        backdrop.show();

        // hold: NORMAL keeps every shortcut running; sticky: POPUP, typing goes to the search
        this._grab = pushModal(backdrop, sticky ? Shell.ActionMode.POPUP : Shell.ActionMode.NORMAL);
        if (!this._grab) {
            this._teardownActor();
            return false;
        }
        this._backdropSignals.push(
            backdrop.connect('captured-event', (_actor: Clutter.Actor, ev: Clutter.Event) =>
                this._onCapturedEvent(ev)),
        );
        if (this._searchEntry)
            this._searchEntry.grab_key_focus();
        else
            backdrop.grab_key_focus();

        this._open = true;
        backdrop.remove_all_transitions();
        if (animationsEnabled()) {
            backdrop.opacity = 0;
            backdrop.ease({
                opacity: 255,
                duration: FADE_IN_MS,
                mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            });
        } else {
            backdrop.opacity = 255;
        }
        return true;
    }

    close(silent = false): void {
        if (!this._open && !this._backdrop)
            return;
        const callback = this._onClosed;
        this._open = false;
        this._sticky = false;
        this._onClosed = null;
        this._snapshot = null;
        this._teardownActor();
        if (!silent && callback)
            callback();
    }

    destroy(): void {
        this.close(true);
    }

    private _teardownActor(): void {
        if (this._grab) {
            Main.popModal(this._grab);
            this._grab = null;
        }
        if (this._searchChangedId && this._searchEntry) {
            this._searchEntry.clutter_text.disconnect(this._searchChangedId);
        }
        this._searchChangedId = 0;
        if (this._backdrop) {
            for (const id of this._backdropSignals)
                this._backdrop.disconnect(id);
            this._backdropSignals = [];
            this._backdrop.remove_all_transitions();
            Main.layoutManager.removeChrome(this._backdrop);
            this._backdrop.destroy();
        } else {
            this._backdropSignals = [];
        }
        this._backdrop = null;
        this._panel = null;
        this._searchEntry = null;
        this._scrollView = null;
        this._contentBox = null;
    }

    private _onCapturedEvent(ev: Clutter.Event): boolean {
        const type = ev.type();
        if (type === Clutter.EventType.BUTTON_PRESS || type === Clutter.EventType.TOUCH_BEGIN) {
            if (!this._sticky || !this._insidePanel(ev)) {
                this.close();
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        }
        if (type !== Clutter.EventType.KEY_PRESS && type !== Clutter.EventType.KEY_RELEASE)
            return Clutter.EVENT_PROPAGATE;
        const sym = ev.get_key_symbol();
        const isSuper = sym === Clutter.KEY_Super_L || sym === Clutter.KEY_Super_R;
        if (!this._sticky) {
            // hold: Super's release, Esc or any other key (shortcuts never get here: mutter ran
            // them before the stage) closes it
            if (type === Clutter.EventType.KEY_RELEASE ? isSuper : !isSuper)
                this.close();
            return Clutter.EVENT_STOP;
        }
        if (type !== Clutter.EventType.KEY_PRESS)
            return Clutter.EVENT_PROPAGATE;
        switch (sym) {
        case Clutter.KEY_Escape:
            this.close();
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Down:
            this._scrollBy(SCROLL_STEP_PX);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Up:
            this._scrollBy(-SCROLL_STEP_PX);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Page_Down:
            this._scrollBy(PAGE_STEP_PX);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Page_Up:
            this._scrollBy(-PAGE_STEP_PX);
            return Clutter.EVENT_STOP;
        default:
            return Clutter.EVENT_PROPAGATE;
        }
    }

    private _insidePanel(ev: Clutter.Event): boolean {
        if (!this._panel)
            return false;
        const [clickX, clickY] = ev.get_coords();
        const [panelX, panelY] = this._panel.get_transformed_position();
        const [panelW, panelH] = this._panel.get_transformed_size();
        return clickX >= panelX && clickY >= panelY && clickX <= panelX + panelW && clickY <= panelY + panelH;
    }

    private _scrollBy(delta: number): void {
        const adj = this._scrollView?.vadjustment;
        if (!adj)
            return;
        const maxVal = Math.max(0, adj.upper - adj.page_size);
        adj.value = Math.min(maxVal, Math.max(0, adj.value + delta));
    }

    private _renderGroups(query: string): void {
        if (!this._contentBox || !this._snapshot)
            return;
        this._contentBox.destroy_all_children();

        const filtered: ShortcutGroup[] = [];
        for (const group of this._snapshot.groups) {
            const matching = group.entries.filter(entry => matchesQuery(entry, group.title, query));
            if (matching.length > 0)
                filtered.push({id: group.id, title: group.title, entries: matching});
        }

        if (filtered.length === 0) {
            const empty = new St.Label({
                text: query ? _('No matching shortcuts') : _('No active shortcuts found'),
                style_class: 'desk-guide-empty',
                x_expand: true,
                x_align: Clutter.ActorAlign.CENTER,
                y_align: Clutter.ActorAlign.CENTER,
            });
            this._contentBox.add_child(empty);
            return;
        }

        if (this._skin === 'windows11') {
            this._renderWin11Columns(filtered);
            return;
        }

        for (const group of filtered)
            this._contentBox.add_child(this._buildGroupSection(group));
    }

    private _renderWin11Columns(groups: readonly ShortcutGroup[]): void {
        if (!this._contentBox)
            return;
        const columnCount = Math.min(3, Math.max(1, groups.length));
        const columns: St.BoxLayout[] = [];
        const heights: number[] = [];
        for (let idx = 0; idx < columnCount; idx++) {
            const col = new St.BoxLayout({
                ...VERTICAL,
                style_class: 'desk-guide-column',
                x_expand: true,
            });
            columns.push(col);
            heights.push(0);
            this._contentBox.add_child(col);
        }
        for (const group of groups) {
            let bestCol = 0;
            let minWeight = heights[0] ?? 0;
            for (let idx = 1; idx < columns.length; idx++) {
                const weight = heights[idx] ?? 0;
                if (weight < minWeight) {
                    minWeight = weight;
                    bestCol = idx;
                }
            }
            columns[bestCol]?.add_child(this._buildGroupSection(group));
            heights[bestCol] = (heights[bestCol] ?? 0) + group.entries.length + 2;
        }
    }

    private _buildGroupSection(group: ShortcutGroup): St.BoxLayout {
        const section = new St.BoxLayout({
            ...VERTICAL,
            style_class: 'desk-guide-group',
            x_expand: true,
        });
        const heading = new St.Label({
            text: group.title,
            style_class: 'desk-guide-group-title',
            x_expand: true,
        });
        section.add_child(heading);
        for (const entry of group.entries)
            section.add_child(this._buildRow(entry));
        return section;
    }

    private _buildRow(entry: ShortcutEntry): St.BoxLayout {
        const row = new St.BoxLayout({
            ...HORIZONTAL,
            style_class: 'desk-guide-row',
            x_expand: true,
        });

        const textCol = new St.BoxLayout({
            ...HORIZONTAL,
            style_class: 'desk-guide-row-text',
            x_expand: true,
            y_align: Clutter.ActorAlign.CENTER,
        });
        textCol.add_child(label(entry.action, 'desk-guide-action'));

        if (this._showOwner) {
            const badge = new St.Label({
                text: ownerText(entry),
                style_class: entry.owner === 'user'
                    ? 'desk-guide-owner desk-guide-owner-user'
                    : 'desk-guide-owner',
                y_align: Clutter.ActorAlign.CENTER,
            });
            textCol.add_child(badge);
        }
        row.add_child(textCol);

        const keysBox = new St.BoxLayout({
            ...HORIZONTAL,
            style_class: 'desk-guide-keys',
            y_align: Clutter.ActorAlign.CENTER,
        });
        entry.combos.forEach((combo, comboIdx) => {
            if (comboIdx > 0) {
                keysBox.add_child(new St.Label({
                    text: '/',
                    style_class: 'desk-guide-combo-sep',
                    y_align: Clutter.ActorAlign.CENTER,
                }));
            }
            for (const token of combo) {
                keysBox.add_child(new St.Label({
                    text: token,
                    style_class: 'desk-guide-keycap',
                    y_align: Clutter.ActorAlign.CENTER,
                }));
            }
        });
        row.add_child(keysBox);
        return row;
    }
}
