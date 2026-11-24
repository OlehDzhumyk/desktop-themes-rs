// SPDX-License-Identifier: GPL-3.0-or-later
// The visible countdown before a delayed capture (AC-SHOT-03). No modal grab: the user can open a
// menu meanwhile, which is the reason for the delay. Esc is taken as a global accelerator only while
// the countdown runs, so Esc cancels it whichever window has the focus; the grab ends with it.
import Meta from 'gi://Meta';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_, fmt} from '../i18n.js';
import type {DeskToast} from '../ui/toast.js';

export class Countdown {
    private _toast: DeskToast;
    private _skin: string;
    private _left: number;
    private _tickId = 0;
    private _escAction = 0;
    private _escName = '';
    private _accelId = 0;
    private _onDone: ((go: boolean) => void) | null;

    /** `onDone(true)` after `seconds`, `onDone(false)` on Esc or cancel(); it runs once. */
    constructor(toast: DeskToast, skin: string, seconds: number, onDone: (go: boolean) => void) {
        this._toast = toast;
        this._skin = skin;
        this._left = seconds;
        this._onDone = onDone;
    }

    start(): void {
        this._grabEscape();
        this._paint();
        this._tickId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
            this._left -= 1;
            if (this._left > 0) {
                this._paint();
                return GLib.SOURCE_CONTINUE;
            }
            this._tickId = 0;
            this._finish(true);
            return GLib.SOURCE_REMOVE;
        });
    }

    private _paint(): void {
        this._toast.busy(fmt(_('Screenshot in {seconds} s. Esc cancels.'), {seconds: this._left}), this._skin);
    }

    private _grabEscape(): void {
        const action = global.display.grab_accelerator('Escape', Meta.KeyBindingFlags.NONE);
        // 0 is Meta.KeyBindingAction.NONE: another client holds Esc; the shortcut again still cancels
        if (action === 0)
            return;
        this._escAction = action;
        this._escName = Meta.external_binding_name_for_action(action);
        Main.wm.allowKeybinding(this._escName, Shell.ActionMode.ALL);
        this._accelId = global.display.connect('accelerator-activated', (_d: Meta.Display, id: number) => {
            if (id === this._escAction)
                this._finish(false);
        });
    }

    private _ungrabEscape(): void {
        if (this._accelId)
            global.display.disconnect(this._accelId);
        this._accelId = 0;
        if (this._escAction) {
            Main.wm.allowKeybinding(this._escName, Shell.ActionMode.NONE);
            global.display.ungrab_accelerator(this._escAction);
        }
        this._escAction = 0;
        this._escName = '';
    }

    /** Stop without capturing (the shortcut again, a lock, destroy). */
    cancel(): void {
        this._finish(false);
    }

    private _finish(go: boolean): void {
        if (this._tickId)
            GLib.source_remove(this._tickId);
        this._tickId = 0;
        this._ungrabEscape();
        // the countdown must not be in the picture
        this._toast.hide();
        const done = this._onDone;
        this._onDone = null;
        done?.(go);
    }
}
