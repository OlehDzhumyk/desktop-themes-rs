// SPDX-License-Identifier: GPL-3.0-or-later
// «Тема» у швидких налаштуваннях: список тем, застосування через D-Bus-службу io.github.OlehDzhumyk.Themes.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {QuickMenuToggle, SystemIndicator} from 'resource:///org/gnome/shell/ui/quickSettings.js';

const ICON = 'preferences-desktop-appearance-symbolic';
const IFACE = `<node><interface name="io.github.OlehDzhumyk.Themes">
  <method name="List"><arg type="a(sssas)" direction="out"/></method>
  <method name="Current"><arg type="a{ss}" direction="out"/></method>
  <method name="Apply"><arg type="s" direction="in"/><arg type="as" direction="in"/>
    <arg type="b" direction="out"/><arg type="s" direction="out"/><arg type="as" direction="out"/></method>
  <signal name="Changed"><arg type="a{ss}"/></signal>
</interface></node>`;
const ThemesProxy = Gio.DBusProxy.makeProxyWrapper(IFACE);
const MODULES = ['look', 'behavior', 'sounds', 'apps'];

const ThemesToggle = GObject.registerClass(
class ThemesToggle extends QuickMenuToggle {
    constructor() {
        super({title: 'Тема', iconName: ICON, toggleMode: false});
        this.menu.setHeader(ICON, 'Тема', '');
        this._section = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._section);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this.menu.addAction('Відкрити «Теми»…', () => this._launchGui());
        this.connect('clicked', () => this.menu.open());
        this._busy = false;
        this._proxy = new ThemesProxy(Gio.DBus.session, 'io.github.OlehDzhumyk.Themes', '/io/github/OlehDzhumyk/Themes', null, null,
            Gio.DBusProxyFlags.DO_NOT_LOAD_PROPERTIES | Gio.DBusProxyFlags.DO_NOT_AUTO_START_AT_CONSTRUCTION);
        this._changedId = this._proxy.connectSignal('Changed', () => this._refresh());
        this._openId = this.menu.connect('open-state-changed', (_m, open) => {
            if (open)
                this._refresh();
        });
        this._refresh();
    }

    async _refresh() {
        try {
            const [[themes], [current]] = await Promise.all([this._proxy.ListAsync(), this._proxy.CurrentAsync()]);
            this._render(themes, current);
        } catch (e) {
            this.subtitle = 'служба недоступна';
            this._section.removeAll();
            this._section.addMenuItem(new PopupMenu.PopupMenuItem(`Помилка: ${e.message}`, {reactive: false}));
        }
    }

    _render(themes, current) {
        const look = current.look ?? '';
        const uniform = MODULES.every(m => (current[m] ?? '') === look);
        const nameOf = id => themes.find(t => t[0] === id)?.[1] ?? id;
        this.checked = look !== '' && look !== 'fedora';
        this.subtitle = look ? (uniform ? nameOf(look) : `${nameOf(look)} (змішано)`) : '';
        this.menu.setHeader(ICON, 'Тема', this.subtitle);
        this._section.removeAll();
        for (const [id, name, , modules] of themes) {
            const item = new PopupMenu.PopupMenuItem(name);
            const active = MODULES.filter(m => current[m] === id);
            const full = active.length === MODULES.length;
            const ornament = PopupMenu.Ornament.CHECK ?? PopupMenu.Ornament.DOT;
            item.setOrnament(full ? ornament : PopupMenu.Ornament.NO_DOT ?? PopupMenu.Ornament.NONE);
            if (active.length && !full)
                item.label.text = `${name} · ${active.length}/${MODULES.length}`;
            item.accessible_name = `${name}: модулі ${modules.join(', ')}`;
            item.connect('activate', () => this._apply(id, name));
            this._section.addMenuItem(item);
        }
    }

    async _apply(id, name) {
        if (this._busy)
            return;
        this._busy = true;
        this.subtitle = `застосування «${name}»…`;
        try {
            const [relogin, message, warnings] = await this._proxy.ApplyAsync(id, []);
            if (relogin)
                Main.notify('Тема', `${message}`);
            else if (warnings.length)
                Main.notify('Тема', `${message}\n${warnings.join('\n')}`);
        } catch (e) {
            Main.notifyError('Тема', `Не вдалося застосувати «${name}»: ${e.message}`);
        } finally {
            this._busy = false;
            this._refresh();
        }
    }

    _launchGui() {
        try {
            const app = Gio.DesktopAppInfo.new('io.github.OlehDzhumyk.Themes.desktop');
            if (app)
                app.launch([], global.create_app_launch_context(0, -1));
            else
                Gio.Subprocess.new([`${GLib.get_home_dir()}/.local/bin/themes`, 'gui'], Gio.SubprocessFlags.NONE);
        } catch (e) {
            Main.notifyError('Тема', e.message);
        }
        Main.panel.closeQuickSettings?.();
    }

    destroy() {
        if (this._changedId)
            this._proxy.disconnectSignal(this._changedId);
        this._changedId = 0;
        super.destroy();
    }
});

const ThemesIndicator = GObject.registerClass(
class ThemesIndicator extends SystemIndicator {
    constructor() {
        super();
        this.quickSettingsItems.push(new ThemesToggle());
    }
});

export default class ThemesQuickExtension extends Extension {
    enable() {
        this._indicator = new ThemesIndicator();
        Main.panel.statusArea.quickSettings.addExternalIndicator(this._indicator);
    }

    disable() {
        this._indicator.quickSettingsItems.forEach(i => i.destroy());
        this._indicator.destroy();
        this._indicator = null;
    }
}
