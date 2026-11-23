// SPDX-License-Identifier: GPL-3.0-or-later
// Emoji and symbol picker feature of the bridge (docs/spec/features/emoji.md):
// global keyboard shortcut, deskd client, clipboard-safe insertion, and skin-aware presentation
// (unified tabs inside WinFlyout for `windows11`, floating EmojiPanel for `macos` and `default`).
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import type {ClipboardFeature} from '../clipboard/feature.js';
import {logError} from '../clipboard/panel.js';
import {EmojiClient, type EmojiItem} from './client.js';
import {EmojiInserter} from './insert.js';
import {EmojiPanel, type EmojiPickHost} from './panel.js';

const SHORTCUT_KEY = 'emoji-shortcut';

export class EmojiFeature implements EmojiPickHost {
    readonly client: EmojiClient;
    private _settings: Gio.Settings;
    private _clipboard: ClipboardFeature | null;
    private _inserter: EmojiInserter;
    private _panel: EmojiPanel | null = null;
    private _skin = '';
    private _bound = false;
    private _settingsIds: number[] = [];
    private _destroyed = false;

    constructor(settings: Gio.Settings, skin: string, clipboard: ClipboardFeature | null) {
        this._settings = settings;
        this._clipboard = clipboard;
        this.client = new EmojiClient();
        this._inserter = new EmojiInserter(() => {
            this._clipboard?.expectOwnContent();
        });
        this._settingsIds.push(settings.connect(`changed::${SHORTCUT_KEY}`, () => { this._bind(); }));
        this._settingsIds.push(settings.connect('changed::enabled', () => {
            this._bind();
            this._syncFlyoutHost();
        }));
        this.setSkin(skin);
        this._bind();
    }

    private get _enabled(): boolean {
        return this._settings.get_boolean('enabled');
    }

    setSkin(skin: string): void {
        if (skin === this._skin && (skin === 'windows11' || this._panel))
            return;
        this._panel?.destroy();
        this._panel = null;
        this._skin = skin;
        if (skin !== 'windows11')
            this._panel = new EmojiPanel(this, skin === 'macos' ? 'macos' : 'default');
        this._syncFlyoutHost();
    }

    private _syncFlyoutHost(): void {
        this._clipboard?.setEmojiHost(this._enabled ? this : null);
    }

    toggle(): void {
        if (!this._enabled)
            return;
        if (this._skin === 'windows11' && this._clipboard?.openTab('emoji'))
            return;
        this._panel?.toggle();
    }

    private _shouldClose(keepOpen: boolean): boolean {
        if (keepOpen)
            return false;
        const mode = this._settings.get_string('close-after-insert');
        if (mode === 'always')
            return true;
        if (mode === 'never')
            return false;
        // 'skin': windows11 stays open for multiple picks; macos and default close
        return this._skin !== 'windows11';
    }

    pickItem(item: EmojiItem, keepOpen: boolean): void {
        if (this._destroyed)
            return;
        const tone = this._settings.get_string('skin-tone');
        const closeNow = this._shouldClose(keepOpen);
        if (closeNow) {
            if (this._skin === 'windows11')
                this._clipboard?.closePanel();
            else
                this._panel?.close();
        }
        this.client.pick(item.id, tone)
            .then(glyph => {
                if (!this._destroyed)
                    this._inserter.insert(glyph);
            })
            .catch(logError);
    }

    private _bind(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        if (!this._enabled)
            return;
        Main.wm.addKeybinding(SHORTCUT_KEY, this._settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW, () => { this.toggle(); });
        this._bound = true;
    }

    destroy(): void {
        this._destroyed = true;
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        for (const id of this._settingsIds)
            this._settings.disconnect(id);
        this._settingsIds = [];
        this._clipboard?.setEmojiHost(null);
        this._clipboard = null;
        this._panel?.destroy();
        this._panel = null;
        this._inserter.destroy();
        this.client.destroy();
    }
}
