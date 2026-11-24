// SPDX-License-Identifier: GPL-3.0-or-later
// Search feature of the bridge (docs/spec/features/search.md, stage 4): binds `search-shortcut` of
// io.github.OlehDzhumyk.Desk.Search in normal mode only (never in the overview or on the lock
// screen), opens the skinned search box (panel.ts), sends the text to deskd (client.ts) and
// performs the chosen result. deskd ranks; the bridge only draws and acts.
//
// The override `search-toggle` of io.github.OlehDzhumyk.Desk.Shortcuts needs no handler here:
// themes-core writes it into `search-shortcut`, as for every other Desk shortcut. The default is
// empty until the own launcher replaces spotlight@nin in the macOS look.
import Gio from 'gi://Gio';
import GioUnix from 'gi://GioUnix';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import type {ClipboardFeature} from '../clipboard/feature.js';
import {EmojiInserter} from '../emoji/insert.js';
import {_, fmt} from '../i18n.js';
import {lockScreen, logOut, restart, shutDown, suspend} from '../logoMenu/actions.js';
import {errorText} from '../shellTypes.js';
import {DeskToast} from '../ui/toast.js';
import {QueryScheduler, SearchClient, type SearchItem} from './client.js';
import {SearchPanel} from './panel.js';

const SHORTCUT_KEY = 'search-shortcut';
const INTERFACE_SCHEMA = 'org.gnome.desktop.interface';
/** deskd sends only the query of a web result; the bridge picks the page that opens it. */
const WEB_SEARCH_URL = 'https://duckduckgo.com/?q=';

function launchContext(): Gio.AppLaunchContext {
    return global.create_app_launch_context(0, -1);
}

/** A file result's target as a file:// URI, or null for anything else. */
function fileUri(target: string): string | null {
    try {
        const file = target.startsWith('/') ? Gio.File.new_for_path(target) : Gio.File.new_for_uri(target);
        return file.get_uri_scheme() === 'file' ? file.get_uri() : null;
    } catch {
        return null;
    }
}

export class SearchFeature {
    private _settings: Gio.Settings;
    private _clipboard: ClipboardFeature | null;
    private _client: SearchClient;
    private _scheduler: QueryScheduler;
    private _inserter: EmojiInserter;
    private _toast: DeskToast | null = null;
    private _panel: SearchPanel | null = null;
    /** A closed panel that is still fading out; destroyed with the feature. */
    private _fading: SearchPanel | null = null;
    private _skin: string;
    private _bound = false;
    /** The keybinding action of `search-shortcut` (0 when not bound): pressed in the open panel, it closes. */
    private _action = 0;
    private _settingsIds: number[] = [];
    private _destroyed = false;

    constructor(settings: Gio.Settings, skin: string, clipboard: ClipboardFeature | null) {
        this._settings = settings;
        this._skin = skin;
        this._clipboard = clipboard;
        this._client = new SearchClient();
        this._scheduler = new QueryScheduler(this._client, (text, items) => {
            this._panel?.setResults(text, items);
        });
        this._inserter = new EmojiInserter(() => { this._clipboard?.expectOwnContent(); });
        this._settingsIds.push(settings.connect(`changed::${SHORTCUT_KEY}`, () => { this._bind(); }));
        this._settingsIds.push(settings.connect('changed::enabled', () => {
            if (!this._enabled)
                this._closePanel();
            this._bind();
        }));
        this._settingsIds.push(settings.connect('changed::max-results', () => {
            if (this._panel)
                this._scheduler.request(this._panel.text, this._limit);
        }));
        this._bind();
    }

    private get _enabled(): boolean {
        return this._settings.get_boolean('enabled');
    }

    private get _limit(): number {
        return this._settings.get_uint('max-results');
    }

    setSkin(skin: string): void {
        if (skin === this._skin)
            return;
        this._skin = skin;
        // an open panel keeps the old look until the next open
        this._closePanel();
    }

    toggle(): void {
        if (this._destroyed || !this._enabled)
            return;
        if (this._panel) {
            this._closePanel();
            return;
        }
        this._dropFading();
        const panel = new SearchPanel(this._skin, {
            query: text => { this._scheduler.request(text, this._limit); },
            // the modal grab stops the keybinding; the panel asks whether a key press is one of its
            // accelerators (every one in the list) so the second press closes (AC-SRCH-01)
            isToggle: (keycode, state) => this._action !== 0
                && global.display.get_keybinding_action(keycode, state) === this._action,
            activate: item => {
                this._panel = null;
                this._scheduler.cancel();
                this._activate(item);
            },
            closed: () => {
                this._fading = this._panel;
                this._panel = null;
                this._scheduler.cancel();
            },
        });
        this._panel = panel;
        if (!panel.open()) {
            this._panel = null;
            this._scheduler.cancel();
        }
    }

    private _closePanel(): void {
        this._scheduler.cancel();
        this._panel?.destroy();
        this._panel = null;
        this._dropFading();
    }

    private _dropFading(): void {
        this._fading?.destroy();
        this._fading = null;
    }

    private _activate(item: SearchItem): void {
        try {
            this._perform(item);
        } catch (e) {
            console.warn(`desk-bridge: search: cannot open ${item.kind} result: ${errorText(e)}`);
            Main.notifyError(_('Search'), errorText(e));
            return;
        }
        // best effort: a usage count that is not written changes only the ranking
        this._client.recordLaunch(item.id).catch(() => { /* ignored */ });
    }

    private _perform(item: SearchItem): void {
        switch (item.kind) {
        case 'app':
            this._launchApp(item.activate);
            return;
        case 'calculator':
            St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, item.activate);
            this._showToast(fmt(_('Copied {value}'), {value: item.activate}));
            return;
        case 'action':
            this._runAction(item.activate);
            return;
        case 'file': {
            const uri = fileUri(item.activate);
            if (!uri)
                throw new Error(`not a local file: ${item.id}`);
            Gio.AppInfo.launch_default_for_uri(uri, launchContext());
            return;
        }
        case 'clipboard': {
            const id = /^\d+$/.test(item.activate) ? Number(item.activate) : NaN;
            if (this._clipboard && Number.isSafeInteger(id)) {
                this._clipboard.activateId(id);
                return;
            }
            St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, item.activate);
            this._showToast(_('Copied to the clipboard'));
            return;
        }
        case 'emoji':
            this._inserter.insert(item.activate);
            return;
        case 'web':
            if (item.activate.trim().length === 0)
                return;
            Gio.AppInfo.launch_default_for_uri(`${WEB_SEARCH_URL}${encodeURIComponent(item.activate)}`,
                launchContext());
            return;
        }
    }

    private _launchApp(desktopId: string): void {
        const app = Shell.AppSystem.get_default().lookup_app(desktopId) as Shell.App | null;
        if (app) {
            app.activate();
            return;
        }
        const info = GioUnix.DesktopAppInfo.new(desktopId) as GioUnix.DesktopAppInfo | null;
        if (!info)
            throw new Error(`no application ${desktopId}`);
        info.launch([], launchContext());
    }

    /** The system actions of crates/desk-search/src/actions.rs. Restart, power off and log out
     * go through the session manager, which asks first. */
    private _runAction(id: string): void {
        switch (id) {
        case 'lock':
            lockScreen();
            return;
        case 'suspend':
            suspend();
            return;
        case 'logout':
            logOut();
            return;
        case 'restart':
            restart();
            return;
        case 'power-off':
            shutDown();
            return;
        case 'dark-mode': {
            const iface = new Gio.Settings({schema_id: INTERFACE_SCHEMA});
            const dark = iface.get_string('color-scheme') === 'prefer-dark';
            iface.set_string('color-scheme', dark ? 'default' : 'prefer-dark');
            this._showToast(dark ? _('Dark style is off') : _('Dark style is on'));
            return;
        }
        case 'clipboard-private': {
            const clip = this._clipboard;
            if (!clip)
                throw new Error('the clipboard history is not available');
            const on = !clip.client.private;
            clip.client.private = on;
            this._showToast(on ? _('Clipboard private mode is on') : _('Clipboard private mode is off'));
            return;
        }
        default:
            throw new Error(`unknown system action ${id}`);
        }
    }

    private _showToast(text: string): void {
        this._toast ??= new DeskToast({variant: 'desk-toast-pill', fade: true});
        this._toast.show(text, this._skin);
    }

    private _bind(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        this._action = 0;
        if (!this._enabled)
            return;
        this._action = Main.wm.addKeybinding(SHORTCUT_KEY, this._settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL, () => { this.toggle(); });
        this._bound = true;
    }

    destroy(): void {
        this._destroyed = true;
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
        this._action = 0;
        for (const id of this._settingsIds)
            this._settings.disconnect(id);
        this._settingsIds = [];
        this._closePanel();
        this._scheduler.destroy();
        this._client.destroy();
        this._inserter.destroy();
        this._toast?.destroy();
        this._toast = null;
        this._clipboard = null;
    }
}
