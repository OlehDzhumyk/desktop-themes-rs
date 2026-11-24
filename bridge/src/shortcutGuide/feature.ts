// SPDX-License-Identifier: GPL-3.0-or-later
// Shortcut Guide feature (docs/spec/features/shortcut-guide.md).
//
// How a held Super is seen. Mutter gives key events to the focused client before the stage, so a
// `captured-event` handler on global.stage never sees them while an app has the focus. Mutter's
// own key path is used instead (src/core/keybindings.c):
//   - press: the overlay key is not consumed on press, so an accelerator grabbed for `Super_L` /
//     `Super_R` (mask 0) fires `accelerator-activated`: the hold timer starts.
//   - another shortcut while Super is down: mutter asks the shell `filter-keybinding` for it before
//     it runs: the timer stops (AC-GUIDE-03), or a guide shown by the hold closes and the shortcut
//     still runs.
//   - release of Super alone: mutter asks `filter-keybinding` for the binding of the overlay combo
//     (our grab) before it emits `overlay-key`. Returning true there is the clean way to keep the
//     overview closed after the guide was shown (AC-GUIDE-01); after a tap it returns false and the
//     overview opens as usual (AC-GUIDE-02).
//   - other releases (after Super+<unbound key>) are not reported: when the timer fires it checks
//     the modifier state once and opens only while Super alone is still down.
// Once shown, the overlay holds a grab (overlay.ts), so it sees the release itself as well.
// Also binds `shortcut-guide-shortcut` in Shell.ActionMode.NORMAL (sticky searchable mode).
import Clutter from 'gi://Clutter';
import type Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {type Skin} from '../accentPicker.js';
import {actionMode, sessionLocked} from '../shellTypes.js';
import {ShortcutGuideOverlay} from './overlay.js';
import {type SchemaOpener, readLiveShortcuts} from './reader.js';

const SHORTCUT_KEY = 'shortcut-guide-shortcut';
const SUPER_KEYS = ['Super_L', 'Super_R'] as const;
/** Mutter reports a grab of the overlay key itself with this action, not with the grab's own. */
const OVERLAY_ACTION: number = Meta.KeyBindingAction.OVERLAY_KEY;
const MIN_DELAY_MS = 400;
const MAX_DELAY_MS = 2000;
const SUPER_MASK = Clutter.ModifierType.MOD4_MASK | Clutter.ModifierType.SUPER_MASK;
const OTHER_MASK = Clutter.ModifierType.CONTROL_MASK | Clutter.ModifierType.MOD1_MASK
    | Clutter.ModifierType.SHIFT_MASK | Clutter.ModifierType.BUTTON1_MASK
    | Clutter.ModifierType.BUTTON2_MASK | Clutter.ModifierType.BUTTON3_MASK;

interface SuperGrab {
    readonly action: number;
    readonly name: string;
}

export class ShortcutGuideFeature {
    private _settings: Gio.Settings;
    private _openSchema: SchemaOpener;
    private _skin: Skin;
    private _overlay = new ShortcutGuideOverlay();
    private _enabledId = 0;
    private _sessionModeId = 0;
    private _accelId = 0;
    private _filterId = 0;
    private _holdTimerId = 0;
    private _grabs: SuperGrab[] = [];
    private _superHeld = false;
    private _shownByHold = false;
    private _bound = false;

    constructor(settings: Gio.Settings, skin: Skin, openSchema: SchemaOpener) {
        this._settings = settings;
        this._skin = skin;
        this._openSchema = openSchema;
        try {
            this._enabledId = settings.connect('changed::enabled', () => { this._syncEnabled(); });
            this._sessionModeId = Main.sessionMode.connect('updated', () => {
                if (sessionLocked())
                    this._abort();
            });
            this._syncEnabled();
        } catch (e) {
            this.destroy();
            throw e;
        }
    }

    setSkin(skin: Skin): void {
        if (skin === this._skin)
            return;
        this._skin = skin;
        this._abort();
    }

    destroy(): void {
        this._stop();
        if (this._sessionModeId)
            Main.sessionMode.disconnect(this._sessionModeId);
        this._sessionModeId = 0;
        if (this._enabledId)
            this._settings.disconnect(this._enabledId);
        this._enabledId = 0;
        this._overlay.destroy();
    }

    private _syncEnabled(): void {
        if (this._settings.get_boolean('enabled'))
            this._start();
        else
            this._stop();
    }

    private _start(): void {
        if (!this._bound) {
            Main.wm.addKeybinding(SHORTCUT_KEY, this._settings, Meta.KeyBindingFlags.IGNORE_AUTOREPEAT,
                Shell.ActionMode.NORMAL, () => { this._openSticky(); });
            this._bound = true;
        }
        if (this._grabs.length > 0)
            return;
        for (const accel of SUPER_KEYS) {
            const action = global.display.grab_accelerator(accel, Meta.KeyBindingFlags.NONE);
            // 0 (Meta.KeyBindingAction.NONE): another client holds this key
            if (action === 0)
                continue;
            const name = Meta.external_binding_name_for_action(action);
            // every mode: the overlay key's release asks the shell about this binding, and a
            // filtered binding would keep Super from closing the overview or a menu
            Main.wm.allowKeybinding(name, Shell.ActionMode.ALL);
            this._grabs.push({action, name});
        }
        if (this._grabs.length === 0) {
            console.warn('desk-bridge: the Super key is taken; the shortcut guide opens by its shortcut only');
            return;
        }
        this._accelId = global.display.connect('accelerator-activated',
            (_d: Meta.Display, action: number) => { this._onAccelerator(action); });
        this._filterId = global.window_manager.connect('filter-keybinding',
            (_wm: Shell.WM, binding: Meta.KeyBinding) => this._onFilter(binding));
    }

    private _stop(): void {
        this._abort();
        if (this._accelId)
            global.display.disconnect(this._accelId);
        this._accelId = 0;
        if (this._filterId)
            global.window_manager.disconnect(this._filterId);
        this._filterId = 0;
        for (const grab of this._grabs) {
            Main.wm.allowKeybinding(grab.name, Shell.ActionMode.NONE);
            global.display.ungrab_accelerator(grab.action);
        }
        this._grabs = [];
        if (this._bound)
            Main.wm.removeKeybinding(SHORTCUT_KEY);
        this._bound = false;
    }

    private _isOurs(name: string): boolean {
        return this._grabs.some(g => g.name === name);
    }

    /** Mutter: the overlay key itself reports Meta.KeyBindingAction.OVERLAY_KEY, Super_R its own action. */
    private _onAccelerator(action: number): void {
        if (action !== OVERLAY_ACTION && !this._grabs.some(g => g.action === action))
            return;
        if (actionMode() !== Shell.ActionMode.NORMAL || sessionLocked() || this._overlay.isOpen)
            return;
        this._superHeld = true;
        this._startHoldTimer();
    }

    /** Called by mutter for every binding before it runs; true keeps that binding from running. */
    private _onFilter(binding: Meta.KeyBinding): boolean {
        if (!this._isOurs(binding.get_name())) {
            // another shortcut while Super is down: no guide, and a guide shown by the hold
            // makes way for the shortcut (it still runs)
            if (this._superHeld || this._holdTimerId)
                this._abort();
            return false;
        }
        if (!this._superHeld)
            return false; // the press; `accelerator-activated` follows
        // the release of Super alone, just before mutter emits `overlay-key`
        const shown = this._shownByHold;
        this._superHeld = false;
        this._shownByHold = false;
        this._clearHoldTimer();
        if (!shown)
            return false;
        if (!this._overlay.isModal)
            this._overlay.close();
        return true;
    }

    private _holdDelayMs(): number {
        const raw = this._settings.get_uint('hold-delay');
        return Math.min(MAX_DELAY_MS, Math.max(MIN_DELAY_MS, raw));
    }

    private _startHoldTimer(): void {
        this._clearHoldTimer();
        this._holdTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, this._holdDelayMs(), () => {
            this._holdTimerId = 0;
            // one check, not a poll: a release after Super+<unbound key> is not reported
            const [, , mods] = global.get_pointer();
            const superAlone = (mods & SUPER_MASK) !== 0 && (mods & OTHER_MASK) === 0;
            if (!this._superHeld || !superAlone || actionMode() !== Shell.ActionMode.NORMAL
                || sessionLocked()) {
                this._superHeld = false;
                return GLib.SOURCE_REMOVE;
            }
            this._shownByHold = this._open(this._settings.get_boolean('sticky') ? 'sticky' : 'hold');
            if (!this._shownByHold)
                this._superHeld = false;
            return GLib.SOURCE_REMOVE;
        });
    }

    private _clearHoldTimer(): void {
        if (this._holdTimerId)
            GLib.source_remove(this._holdTimerId);
        this._holdTimerId = 0;
    }

    private _abort(): void {
        this._clearHoldTimer();
        this._superHeld = false;
        this._shownByHold = false;
        this._overlay.close();
    }

    private _openSticky(): void {
        if (sessionLocked() || this._overlay.isOpen)
            return;
        this._clearHoldTimer();
        this._superHeld = false;
        this._shownByHold = false;
        this._open('sticky');
    }

    private _open(mode: 'hold' | 'sticky'): boolean {
        return this._overlay.open({
            skin: this._skin,
            mode,
            showOwner: this._settings.get_boolean('show-owner'),
            snapshot: readLiveShortcuts(this._openSchema, this._skin),
            onClosed: () => {
                // the overlay saw the release itself (or Esc, a click, another key)
                this._superHeld = false;
                this._shownByHold = false;
            },
        });
    }
}
