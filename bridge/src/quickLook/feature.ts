// SPDX-License-Identifier: GPL-3.0-or-later
// Quick Look feature of the bridge (docs/spec/features/quick-look.md): binds `quick-look-shortcut`
// of io.github.OlehDzhumyk.Desk.QuickLook in normal mode only (never in the overview or on the
// lock screen) and starts the previewer, `themes quick-look -- <paths…>`, as a separate process, so
// a crashing decoder never takes the shell with it (AC-QL-09).
//
// Which files: Files (Nautilus) has no API that gives its selection, and taking over Sushi's
// org.gnome.NautilusPreviewer name would break Sushi (the `fedora` theme keeps Sushi and binds no
// shortcut). Until the Files hand-over exists, the files come from the clipboard: the local files
// that were copied (Ctrl+C in Files offers `text/uri-list` and `x-special/gnome-copied-files`).
// The clipboard is read only when the shortcut is pressed; no file name is logged.
//
// The override `quick-look-toggle` of io.github.OlehDzhumyk.Desk.Shortcuts needs no handler here:
// themes-core writes it into `quick-look-shortcut` (crates/themes-core/src/shortcuts.rs), as for
// every other Desk shortcut. Plain Space is refused: shell keybindings are global and would eat
// every typed space.
import Gio from 'gi://Gio';
import type GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_, fmt} from '../i18n.js';
import {runThemes} from '../quickSettings.js';
import {errorText} from '../shellTypes.js';
import {DeskToast} from '../ui/toast.js';

const SHORTCUT_KEY = 'quick-look-shortcut';
/** Clipboard types that carry copied files, best first. */
const URI_TYPES = ['text/uri-list', 'x-special/gnome-copied-files'] as const;
/** The previewer's selection is capped (the spec's edge case: 1000 files stays O(1)). */
const MAX_FILES = 1000;
/** How long a message stays on screen. */
const TOAST_MS = 4000;
const SIGTERM = 15;

/** An accelerator that is Space without a modifier. */
function isBareSpace(accel: string): boolean {
    return accel.trim().toLowerCase() === 'space';
}

/**
 * The local paths in clipboard text of `text/uri-list` (RFC 2483: one URI per line, `#` comments)
 * or `x-special/gnome-copied-files` (a first line `copy` or `cut`, then one URI per line).
 * Returns the paths and whether URIs of other locations (remote, trash) were left out.
 */
function localPaths(text: string): {paths: string[]; skipped: boolean} {
    const paths: string[] = [];
    let skipped = false;
    for (const raw of text.split(/\r?\n/)) {
        const line = raw.trim();
        if (!line || line.startsWith('#') || line === 'copy' || line === 'cut')
            continue;
        let path: string | null = null;
        try {
            const file = Gio.File.new_for_uri(line);
            if (file.get_uri_scheme() === 'file')
                path = file.get_path();
        } catch {
            path = null;
        }
        if (path === null) {
            skipped = true;
            continue;
        }
        if (paths.length < MAX_FILES && !paths.includes(path))
            paths.push(path);
    }
    return {paths, skipped};
}

/** The clipboard content of `type` as text, or null. */
function clipboardText(type: string): Promise<string | null> {
    return new Promise(resolve => {
        St.Clipboard.get_default().get_content(St.ClipboardType.CLIPBOARD, type,
            (_c: St.Clipboard, bytes: GLib.Bytes | null) => {
                const data = bytes && bytes.get_size() > 0 ? bytes.get_data() : null;
                resolve(data ? new TextDecoder().decode(data) : null);
            });
    });
}

export class QuickLookFeature {
    private _settings: Gio.Settings;
    private _skin: string;
    private _toast = new DeskToast();
    private _settingsIds: number[] = [];
    private _bound = false;
    /** The previewer this feature started and still waits for; the shortcut closes it again. */
    private _child: Gio.Subprocess | null = null;
    private _cancellable = new Gio.Cancellable();
    /** Bumped by each press and by destroy(): a clipboard read that finishes late is dropped. */
    private _generation = 0;
    private _destroyed = false;

    constructor(settings: Gio.Settings, skin: string) {
        this._settings = settings;
        this._skin = skin;
        try {
            for (const key of [SHORTCUT_KEY, 'enabled'])
                this._settingsIds.push(settings.connect(`changed::${key}`, () => { this._bind(); }));
            this._bind();
        } catch (e) {
            // a half-built feature must not leave handlers or keybindings behind
            this.destroy();
            throw e;
        }
    }

    setSkin(skin: string): void {
        if (skin === this._skin)
            return;
        this._toast.hide();
        this._skin = skin;
    }

    private _unbind(): void {
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
    }

    private _bind(): void {
        this._unbind();
        if (!this._settings.get_boolean('enabled'))
            return;
        if (this._settings.get_strv(SHORTCUT_KEY).some(isBareSpace)) {
            console.warn('desk-bridge: Quick Look: plain Space is never bound (shell shortcuts are global)');
            return;
        }
        Main.wm.addKeybinding(SHORTCUT_KEY, this._settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
            Shell.ActionMode.NORMAL, () => { this.toggle(); });
        this._bound = true;
    }

    /** The shortcut: closes the previewer started here, or opens one on the copied files. */
    toggle(): void {
        if (this._destroyed || !this._settings.get_boolean('enabled'))
            return;
        if (this._child) {
            this._child.send_signal(SIGTERM);
            return;
        }
        const generation = ++this._generation;
        void this._open(generation);
    }

    private async _open(generation: number): Promise<void> {
        const offered: string[] = global.display.get_selection()
            .get_mimetypes(Meta.SelectionType.SELECTION_CLIPBOARD);
        let found = {paths: [] as string[], skipped: false};
        for (const type of URI_TYPES) {
            if (!offered.includes(type))
                continue;
            const text = await clipboardText(type);
            if (generation !== this._generation || this._destroyed)
                return;
            if (text) {
                found = localPaths(text);
                break;
            }
        }
        if (found.paths.length === 0) {
            this._toast.show(found.skipped
                ? _('Quick Look shows only files on this computer.')
                : _('No files to preview. Copy files in Files, then press the shortcut again.'),
            this._skin, TOAST_MS);
            return;
        }
        this._spawn(found.paths);
    }

    private _spawn(paths: string[]): void {
        let child: Gio.Subprocess;
        try {
            // `--`: a file named like an option stays a path
            child = runThemes(['quick-look', '--', ...paths]);
        } catch (e) {
            console.warn(`desk-bridge: Quick Look could not start: ${errorText(e)}`);
            this._toast.show(fmt(_('Could not open Quick Look: {error}'), {error: errorText(e)}),
                this._skin, TOAST_MS);
            return;
        }
        this._child = child;
        child.wait_async(this._cancellable, (_c: Gio.Subprocess | null, res: Gio.AsyncResult) => {
            try {
                child.wait_finish(res);
            } catch {
                // cancelled by destroy(): the previewer stays open, it is the user's window
            }
            if (this._child === child)
                this._child = null;
        });
    }

    destroy(): void {
        this._destroyed = true;
        this._generation++;
        this._cancellable.cancel();
        this._child = null;
        this._unbind();
        for (const id of this._settingsIds)
            this._settings.disconnect(id);
        this._settingsIds = [];
        this._toast.destroy();
    }
}
