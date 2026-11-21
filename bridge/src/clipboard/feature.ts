// SPDX-License-Identifier: GPL-3.0-or-later
// Clipboard history feature of the bridge: capture (watcher), the deskd client, the skinned panel,
// the keyboard shortcut and paste-on-select. The history itself lives in deskd.
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {ClipboardClient, type ClipItem} from '../dbus.js';
import {ListPanel} from './listPanel.js';
import {type ClipboardPanel, type PanelHost, logError} from './panel.js';
import {Paster} from './paste.js';
import {ClipboardWatcher} from './watcher.js';
import {WinFlyout} from './winFlyout.js';

export const SHORTCUT_KEY = 'toggle-shortcut';

export class ClipboardFeature implements PanelHost {
    readonly client: ClipboardClient;
    readonly settings: Gio.Settings;
    private _watcher: ClipboardWatcher;
    private _paster: Paster;
    private _panel: ClipboardPanel | null = null;
    private _skin = '';
    private _bound = false;
    private _settingsIds: number[] = [];
    private _listeners = new Set<() => void>();

    constructor(extDir: string, settings: Gio.Settings, skin: string, private _openPrefs: () => void) {
        this.settings = settings;
        this.client = new ClipboardClient(extDir, () => this._changed());
        this._watcher = new ClipboardWatcher(this.client, settings);
        this._paster = new Paster();
        this._settingsIds.push(settings.connect(`changed::${SHORTCUT_KEY}`, () => this._bind()));
        this._settingsIds.push(settings.connect('changed::enabled', () => this._bind()));
        this.setSkin(skin);
        this._bind();
    }

    /** Called when deskd reports a change; panels refresh, menus update. */
    onChanged(f: () => void): () => void {
        this._listeners.add(f);
        return () => this._listeners.delete(f);
    }

    private _changed(): void {
        if (this._panel?.isOpen)
            void this._panel.refresh();
        for (const f of this._listeners)
            f();
    }

    setSkin(skin: string): void {
        if (skin === this._skin && this._panel)
            return;
        this._panel?.destroy();
        this._skin = skin;
        this._panel = skin === 'windows11' ? new WinFlyout(this)
            : new ListPanel(this, skin === 'macos' ? 'macos' : 'default');
    }

    get skin(): string {
        return this._skin;
    }

    toggle(): void {
        this._panel?.toggle();
    }

    private _bind(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        if (!this.settings.get_boolean('enabled'))
            return;
        Main.wm.addKeybinding(SHORTCUT_KEY, this.settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW, () => this.toggle());
        this._bound = true;
    }

    activate(item: ClipItem): void {
        const paste = this.settings.get_boolean('paste-on-select');
        this.client.activate(item.id)
            .then(([mime, data]) => this._paster.paste(mime, data, paste))
            .catch(logError);
    }

    openSettings(): void {
        this._openPrefs();
    }

    destroy(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        for (const id of this._settingsIds)
            this.settings.disconnect(id);
        this._settingsIds = [];
        this._listeners.clear();
        this._panel?.destroy();
        this._panel = null;
        this._watcher.destroy();
        this._paster.destroy();
        this.client.destroy();
    }
}
