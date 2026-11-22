// SPDX-License-Identifier: GPL-3.0-or-later
// Quick settings items of the bridge: “Theme” (theme switcher, formerly the themes-quick extension)
// and, for the default skin, “Clipboard” (recent items, private mode, clear).
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {QuickMenuToggle, SystemIndicator} from 'resource:///org/gnome/shell/ui/quickSettings.js';

import {confirm} from './confirmDialog.js';
import {ThemesClient, type ThemeRow} from './dbus.js';
import {_, fmt} from './i18n.js';
import type {ClipboardFeature} from './clipboard/feature.js';
import {itemAccessibleName, oneLine} from './clipboard/panel.js';

const THEME_ICON = 'preferences-desktop-appearance-symbolic';
const CLIP_ICON = 'edit-paste-symbolic';
const MODULES = ['look', 'behavior', 'sounds', 'apps'];

/** Launch the window (themes gui; “Робочий стіл” in the bridge), optionally on a page. */
export function launchGui(page?: string): void {
    try {
        if (!page) {
            const app = GioUnix.DesktopAppInfo.new('io.github.OlehDzhumyk.Themes.desktop');
            if (app) {
                app.launch([], global.create_app_launch_context(0, -1));
                return;
            }
        }
        const argv = [`${GLib.get_home_dir()}/.local/bin/themes`, 'gui'];
        if (page)
            argv.push('--page', page);
        Gio.Subprocess.new(argv, Gio.SubprocessFlags.NONE);
    } catch (e) {
        Main.notifyError(_('Desktop'), String(e));
    }
}

const ThemesToggle = GObject.registerClass(
class ThemesToggle extends QuickMenuToggle {
    declare private _section: PopupMenu.PopupMenuSection;
    declare private _client: ThemesClient;
    declare private _busy: boolean;

    constructor() {
        super({title: _('Theme'), iconName: THEME_ICON, toggleMode: false});
        this.menu.setHeader(THEME_ICON, _('Theme'), '');
        this._section = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._section);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this.menu.addAction(_('Open “Desktop”…'), () => {
            launchGui();
            Main.panel.closeQuickSettings();
        });
        this.connect('clicked', () => this.menu.open());
        this._busy = false;
        this._client = new ThemesClient(() => void this._refresh());
        this.menu.connect('open-state-changed', (_m: unknown, open: boolean) => {
            if (open)
                void this._refresh();
            return undefined;
        });
        void this._refresh();
    }

    async _refresh(): Promise<void> {
        try {
            const [themes, current] = await this._client.listAndCurrent();
            this._render(themes, current);
        } catch (e) {
            this.subtitle = _('service unavailable');
            this._section.removeAll();
            this._section.addMenuItem(new PopupMenu.PopupMenuItem(
                fmt(_('Error: {error}'), {error: (e as Error).message}), {reactive: false}));
        }
    }

    _render(themes: ThemeRow[], current: Record<string, string>): void {
        const look = current.look ?? '';
        const uniform = MODULES.every(m => (current[m] ?? '') === look);
        const nameOf = (id: string) => themes.find(t => t[0] === id)?.[1] ?? id;
        this.checked = look !== '' && look !== 'fedora';
        this.subtitle = look ? (uniform ? nameOf(look) : fmt(_('{name} (mixed)'), {name: nameOf(look)})) : '';
        this.menu.setHeader(THEME_ICON, _('Theme'), this.subtitle ?? '');
        this._section.removeAll();
        for (const [id, name, , modules] of themes) {
            const item = new PopupMenu.PopupMenuItem(name);
            const active = MODULES.filter(m => current[m] === id);
            const full = active.length === MODULES.length;
            item.setOrnament(full ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NO_DOT);
            if (active.length && !full)
                item.label.text = `${name} · ${active.length}/${MODULES.length}`;
            const state = full ? _('active') : active.length ? fmt(_('{count} of {total} modules active'),
                {count: active.length, total: MODULES.length}) : '';
            item.accessible_name = fmt(_('{name}: modules {modules}'), {name, modules: modules.join(', ')}) +
                (state ? `, ${state}` : '');
            item.connect('activate', () => void this._apply(id, name));
            this._section.addMenuItem(item);
        }
    }

    async _apply(id: string, name: string): Promise<void> {
        if (this._busy)
            return;
        this._busy = true;
        const warning = (await this._client.experimental())[id];
        if (warning !== undefined) {
            Main.panel.closeQuickSettings();
            const ok = await confirm(fmt(_('Apply the experimental theme “{name}”?'), {name}), warning,
                _('Apply anyway'));
            if (!ok) {
                this._busy = false;
                return;
            }
        }
        this.subtitle = fmt(_('applying “{name}”…'), {name});
        try {
            const [relogin, message, warnings] = await this._client.apply(id);
            if (relogin)
                Main.notify(_('Theme'), message);
            else if (warnings.length)
                Main.notify(_('Theme'), `${message}\n${warnings.join('\n')}`);
        } catch (e) {
            Main.notifyError(_('Theme'), fmt(_('Could not apply “{name}”: {error}'), {name, error: (e as Error).message}));
        } finally {
            this._busy = false;
            void this._refresh();
        }
    }

    override destroy(): void {
        this._client.destroy();
        super.destroy();
    }
});

const ClipboardToggle = GObject.registerClass(
class ClipboardToggle extends QuickMenuToggle {
    declare private _feature: ClipboardFeature;
    declare private _section: PopupMenu.PopupMenuSection;
    declare private _privateItem: PopupMenu.PopupSwitchMenuItem;
    declare private _unlisten: () => void;

    constructor(feature: ClipboardFeature) {
        super({title: _('Clipboard'), iconName: CLIP_ICON, toggleMode: false});
        this._feature = feature;
        this.menu.setHeader(CLIP_ICON, _('Clipboard'), '');
        this._section = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._section);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._privateItem = new PopupMenu.PopupSwitchMenuItem(_('Private mode'), false);
        this._privateItem.connect('toggled', (_i: unknown, state: boolean) => {
            this._feature.client.private = state;
        });
        this.menu.addMenuItem(this._privateItem);
        this.menu.addAction(_('Clear history'), () => {
            void this._feature.client.clear(true).catch(e => console.warn(e));
        });
        this.menu.addAction(_('Clipboard settings…'), () => {
            launchGui('clipboard');
            Main.panel.closeQuickSettings();
        });
        this.connect('clicked', () => this.menu.open());
        this.menu.connect('open-state-changed', (_m: unknown, open: boolean) => {
            if (open)
                void this._refresh();
            return undefined;
        });
        this._unlisten = feature.onChanged(() => {
            if (this.menu.isOpen)
                void this._refresh();
            else
                this._sync();
        });
        this._sync();
    }

    _sync(): void {
        const priv = this._feature.client.private;
        this.checked = !priv;
        this.subtitle = priv ? _('private mode') : '';
        this._privateItem.setToggleState(priv);
    }

    async _refresh(): Promise<void> {
        this._sync();
        this._section.removeAll();
        let items;
        try {
            items = await this._feature.client.list('', 8);
        } catch {
            this._section.addMenuItem(new PopupMenu.PopupMenuItem(_('deskd is unavailable'), {reactive: false}));
            return;
        }
        if (!items.length)
            this._section.addMenuItem(new PopupMenu.PopupMenuItem(_('The history is empty'), {reactive: false}));
        for (const it of items) {
            const mi = new PopupMenu.PopupMenuItem(oneLine(it.text, 60));
            if (it.pinned)
                mi.setOrnament(PopupMenu.Ornament.CHECK);
            mi.accessible_name = itemAccessibleName(it);
            mi.connect('activate', () => {
                Main.panel.closeQuickSettings();
                this._feature.activate(it);
            });
            this._section.addMenuItem(mi);
        }
    }

    override destroy(): void {
        this._unlisten();
        super.destroy();
    }
});

export const DeskIndicator = GObject.registerClass(
class DeskIndicator extends SystemIndicator {
    declare private _clipToggle: InstanceType<typeof ClipboardToggle> | null;

    constructor() {
        super();
        this.quickSettingsItems.push(new ThemesToggle());
        this._clipToggle = null;
    }

    /** The clipboard toggle exists only with the default skin. */
    setClipboard(feature: ClipboardFeature | null): void {
        if (this._clipToggle) {
            this.quickSettingsItems = this.quickSettingsItems.filter(i => i !== this._clipToggle);
            this._clipToggle.destroy();
            this._clipToggle = null;
        }
        if (feature) {
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            this._clipToggle = new (ClipboardToggle as any)(feature) as InstanceType<typeof ClipboardToggle>;
            this.quickSettingsItems.push(this._clipToggle);
            // items added after addExternalIndicator() must be placed by hand
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            const qs = Main.panel.statusArea.quickSettings as any;
            qs.menu?.addItem?.(this._clipToggle);
        }
    }

    override destroy(): void {
        this.quickSettingsItems.forEach(i => i.destroy());
        super.destroy();
    }
});
