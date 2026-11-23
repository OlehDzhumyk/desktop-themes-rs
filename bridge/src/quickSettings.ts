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
import {errorText} from './shellTypes.js';
import type {ClipboardFeature} from './clipboard/feature.js';
import {itemAccessibleName, oneLine} from './clipboard/panel.js';

const THEME_ICON = 'preferences-desktop-appearance-symbolic';
const CLIP_ICON = 'edit-paste-symbolic';
const MODULES = ['look', 'behavior', 'sounds', 'apps'];

/** The typings say `new()` always finds the file; at run time it returns null when it does not. */
function desktopApp(id: string): GioUnix.DesktopAppInfo | null {
    return GioUnix.DesktopAppInfo.new(id);
}

/** Launch the window (themes gui; “Робочий стіл” in the bridge), optionally on a page. */
export function launchGui(page?: string): void {
    try {
        if (!page) {
            const app = desktopApp('io.github.OlehDzhumyk.Themes.desktop');
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

/**
 * GNOME Shell 51 puts a quick settings item's menu into the quick settings overlay
 * (QuickSettingsMenu._completeAddItem) and does not destroy it with the item, so a removed toggle
 * would leave its menu, items and their signal handlers on the stage. Call from the toggle's
 * 'destroy' handler.
 */
function destroyMenu(toggle: QuickMenuToggle): void {
    toggle.menu.destroy();
}

const ThemesToggle = GObject.registerClass(
class ThemesToggle extends QuickMenuToggle {
    declare private _section: PopupMenu.PopupMenuSection;
    declare private _client: ThemesClient | null;
    declare private _busy: boolean;
    /** Destroyed, by disable() or by the shell tearing the stage down at logout (from C, so no
     * JS destroy() override runs then); async work that finishes later must not touch it. */
    declare private _destroyed: boolean;

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
        this.connect('clicked', () => { this.menu.open(); });
        this._busy = false;
        this._destroyed = false;
        this._client = null;
        this.connect('destroy', () => {
            this._destroyed = true;
            this._client?.destroy();
            destroyMenu(this);
        });
        this.menu.connect('open-state-changed', (_m: unknown, open: boolean) => {
            if (open)
                void this._refresh();
            return undefined;
        });
    }

    /** The service client needs the extension directory (its interface XML), handed over right
     * after construction (see ClipboardToggle.attach). */
    attach(extDir: string): void {
        this._client = new ThemesClient(extDir, () => void this._refresh());
        void this._refresh();
    }

    /** Read through a method: TypeScript cannot see the change across an await. */
    _gone(): boolean {
        return this._destroyed;
    }

    async _refresh(): Promise<void> {
        if (!this._client)
            return;
        try {
            const [themes, current] = await this._client.listAndCurrent();
            if (!this._gone())
                this._render(themes, current);
        } catch (e) {
            if (this._gone())
                return;
            this.subtitle = _('service unavailable');
            this._section.removeAll();
            this._section.addMenuItem(new PopupMenu.PopupMenuItem(
                fmt(_('Error: {error}'), {error: errorText(e)}), {reactive: false}));
        }
    }

    _render(themes: ThemeRow[], current: Record<string, string>): void {
        const look = current['look'] ?? '';
        const uniform = MODULES.every(m => (current[m] ?? '') === look);
        const nameOf = (id: string) => themes.find(t => t[0] === id)?.[1] ?? id;
        this.checked = look !== '' && look !== 'fedora';
        this.subtitle = look ? (uniform ? nameOf(look) : fmt(_('{name} (mixed)'), {name: nameOf(look)})) : '';
        this.menu.setHeader(THEME_ICON, _('Theme'), this.subtitle);
        this._section.removeAll();
        for (const [id, name, , modules] of themes) {
            const item = new PopupMenu.PopupMenuItem(name);
            const active = MODULES.filter(m => current[m] === id);
            const full = active.length === MODULES.length;
            item.setOrnament(full ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NO_DOT);
            if (active.length && !full)
                item.label.text = `${name} · ${String(active.length)}/${String(MODULES.length)}`;
            const state = full ? _('active') : active.length ? fmt(_('{count} of {total} modules active'),
                {count: active.length, total: MODULES.length}) : '';
            item.accessible_name = fmt(_('{name}: modules {modules}'), {name, modules: modules.join(', ')}) +
                (state ? `, ${state}` : '');
            item.connect('activate', () => void this._apply(id, name));
            this._section.addMenuItem(item);
        }
    }

    async _apply(id: string, name: string): Promise<void> {
        const client = this._client;
        if (this._busy || !client)
            return;
        this._busy = true;
        // fail closed: when the service cannot say whether the theme is experimental or sets
        // unapproved command keys, nothing is applied (ADR 0006, S-5)
        let warning: string | undefined;
        let pending: [string, string][];
        try {
            const [exp, cmds] = await Promise.all([client.experimental(), client.pendingCommands(id)]);
            warning = exp[id];
            pending = cmds;
        } catch (e) {
            this._busy = false;
            if (!this._gone())
                Main.notifyError(_('Theme'), fmt(_('Could not apply “{name}”: {error}'), {name, error: errorText(e)}));
            return;
        }
        if (this._gone())
            return;
        if (warning !== undefined) {
            Main.panel.closeQuickSettings();
            const ok = await confirm(fmt(_('Apply the experimental theme “{name}”?'), {name}), warning,
                _('Apply anyway'));
            if (!ok || this._gone()) {
                this._busy = false;
                return;
            }
        }
        if (pending.length > 0) {
            Main.panel.closeQuickSettings();
            const list = pending.map(([k, v]) => `${k} = ${v}`).join('\n');
            const ok = await confirm(
                fmt(_('Allow “{name}” to run commands?'), {name}),
                fmt(_('This theme sets keys that make the desktop run programs:\n\n{list}'), {list}),
                _('Allow and apply'),
            );
            if (!ok || this._gone()) {
                this._busy = false;
                return;
            }
        }
        this.subtitle = fmt(_('applying “{name}”…'), {name});
        try {
            const [relogin, message, warnings] = await client.apply(id, warning !== undefined || pending.length > 0);
            if (relogin)
                Main.notify(_('Theme'), message);
            else if (warnings.length)
                Main.notify(_('Theme'), `${message}\n${warnings.join('\n')}`);
        } catch (e) {
            Main.notifyError(_('Theme'), fmt(_('Could not apply “{name}”: {error}'), {name, error: errorText(e)}));
        } finally {
            this._busy = false;
            if (!this._gone())
                void this._refresh();
        }
    }
});

// GObject typings describe the constructor of a registered class through _init(), not through a
// custom constructor, so the feature is handed over by attach() right after construction.
const ClipboardToggle = GObject.registerClass(
class ClipboardToggle extends QuickMenuToggle {
    declare private _feature: ClipboardFeature | null;
    declare private _section: PopupMenu.PopupMenuSection;
    declare private _privateItem: PopupMenu.PopupSwitchMenuItem;
    declare private _unlisten: (() => void) | null;
    declare private _destroyed: boolean;

    constructor() {
        super({title: _('Clipboard'), iconName: CLIP_ICON, toggleMode: false});
        this._feature = null;
        this._unlisten = null;
        this._destroyed = false;
        // a signal, not a destroy() override: the shell may destroy the toggle from C at logout
        this.connect('destroy', () => {
            this._destroyed = true;
            this._unlisten?.();
            this._unlisten = null;
            this._feature = null;
            destroyMenu(this);
        });
        this.menu.setHeader(CLIP_ICON, _('Clipboard'), '');
        this._section = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._section);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._privateItem = new PopupMenu.PopupSwitchMenuItem(_('Private mode'), false);
        this._privateItem.connect('toggled', (_i: unknown, state: boolean) => {
            if (this._feature)
                this._feature.client.private = state;
        });
        this.menu.addMenuItem(this._privateItem);
        this.menu.addAction(_('Clear history'), () => {
            void this._feature?.client.clear(true).catch((e: unknown) => { console.warn(e); });
        });
        this.menu.addAction(_('Clipboard settings…'), () => {
            launchGui('clipboard');
            Main.panel.closeQuickSettings();
        });
        this.connect('clicked', () => { this.menu.open(); });
        this.menu.connect('open-state-changed', (_m: unknown, open: boolean) => {
            if (open)
                void this._refresh();
            return undefined;
        });
    }

    /** Connect the toggle to the clipboard feature it shows. */
    attach(feature: ClipboardFeature): void {
        this._feature = feature;
        this._unlisten = feature.onChanged(() => {
            if (this.menu.isOpen)
                void this._refresh();
            else
                this._sync();
        });
        this._sync();
    }

    _sync(): void {
        const priv = this._feature?.client.private ?? false;
        this.checked = !priv;
        this.subtitle = priv ? _('private mode') : '';
        this._privateItem.setToggleState(priv);
    }

    /** Read through a method: TypeScript cannot see the change across an await. */
    _gone(): boolean {
        return this._destroyed;
    }

    async _refresh(): Promise<void> {
        this._sync();
        this._section.removeAll();
        const feature = this._feature;
        if (!feature)
            return;
        let items;
        try {
            items = await feature.client.list('', 8);
        } catch {
            if (this._gone())
                return;
            this._section.addMenuItem(new PopupMenu.PopupMenuItem(_('deskd is unavailable'), {reactive: false}));
            return;
        }
        if (this._gone())
            return;
        if (!items.length)
            this._section.addMenuItem(new PopupMenu.PopupMenuItem(_('The history is empty'), {reactive: false}));
        for (const it of items) {
            const mi = new PopupMenu.PopupMenuItem(oneLine(it.text, 60));
            if (it.pinned)
                mi.setOrnament(PopupMenu.Ornament.CHECK);
            mi.accessible_name = itemAccessibleName(it);
            mi.connect('activate', () => {
                Main.panel.closeQuickSettings();
                feature.activate(it);
            });
            this._section.addMenuItem(mi);
        }
    }
});

export const DeskIndicator = GObject.registerClass(
class DeskIndicator extends SystemIndicator {
    declare private _clipToggle: InstanceType<typeof ClipboardToggle> | null;
    declare private _themes: InstanceType<typeof ThemesToggle>;

    constructor() {
        super();
        this._themes = new ThemesToggle();
        this.quickSettingsItems.push(this._themes);
        this._clipToggle = null;
    }

    /** Hands the extension directory to the items that read interface XML from it. */
    attach(extDir: string): void {
        this._themes.attach(extDir);
    }

    /** The clipboard toggle exists only with the default skin. */
    setClipboard(feature: ClipboardFeature | null): void {
        if (this._clipToggle) {
            this.quickSettingsItems = this.quickSettingsItems.filter(i => i !== this._clipToggle);
            this._clipToggle.destroy();
            this._clipToggle = null;
        }
        if (feature) {
            this._clipToggle = new ClipboardToggle();
            this._clipToggle.attach(feature);
            this.quickSettingsItems.push(this._clipToggle);
            // items added after addExternalIndicator() must be placed by hand
            Main.panel.statusArea.quickSettings?.menu.addItem(this._clipToggle);
        }
    }

    override destroy(): void {
        this.quickSettingsItems.forEach(i => { i.destroy(); });
        super.destroy();
    }
});
