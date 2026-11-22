// SPDX-License-Identifier: GPL-3.0-or-later
// Paste-on-select: put the chosen item on the clipboard, then synthesize the paste key in the window
// that gets the focus back once our panel has closed.
//
// Adapted from Clipboard Indicator (clipboard-indicator@tudmotu.com, keyboard.js and
// extension.js #pasteItem / #updateClipboard), Copyright (c) 2014 Yotam Bar-On and contributors,
// MIT licence (https://github.com/Tudmotu/gnome-shell-extension-clipboard-indicator).
// Changes: TypeScript, one class, configurable delay, timers cleaned up on destroy.
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import {inputMethod} from '../shellTypes.js';

export class Paster {
    private _device: Clutter.VirtualInputDevice;
    private _purpose: Clutter.InputContentPurpose = Clutter.InputContentPurpose.NORMAL;
    private _timeouts = new Set<number>();

    constructor() {
        const seat = global.stage.context.get_backend().get_default_seat();
        this._device = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
        // terminals paste with Ctrl+Shift+Insert, everything else with Shift+Insert
        const im = inputMethod();
        im.connectObject('notify::content-purpose', () => {
            this._purpose = im.content_purpose;
        }, this);
    }

    private _key(key: number, pressed: boolean): void {
        this._device.notify_keyval(Clutter.get_current_event_time() * 1000, key,
            pressed ? Clutter.KeyState.PRESSED : Clutter.KeyState.RELEASED);
    }

    private _later(ms: number, f: () => void): void {
        const id = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            this._timeouts.delete(id);
            f();
            return GLib.SOURCE_REMOVE;
        });
        this._timeouts.add(id);
    }

    setClipboard(mime: string, data: Uint8Array): void {
        const cb = St.Clipboard.get_default();
        if (mime.startsWith('text/'))
            cb.set_text(St.ClipboardType.CLIPBOARD, new TextDecoder().decode(data));
        else
            cb.set_content(St.ClipboardType.CLIPBOARD, mime, new GLib.Bytes(data));
    }

    /** Set the clipboard and, if paste is wanted, press the paste keys after `delay` ms. */
    paste(mime: string, data: Uint8Array, synthesize: boolean, delay = 120): void {
        this.setClipboard(mime, data);
        if (!synthesize)
            return;
        this._later(delay, () => {
            const keys = this._purpose === Clutter.InputContentPurpose.TERMINAL
                ? [Clutter.KEY_Control_L, Clutter.KEY_Shift_L, Clutter.KEY_Insert]
                : [Clutter.KEY_Shift_L, Clutter.KEY_Insert];
            for (const k of keys)
                this._key(k, true);
            for (const k of [...keys].reverse())
                this._key(k, false);
        });
    }

    destroy(): void {
        for (const id of this._timeouts)
            GLib.source_remove(id);
        this._timeouts.clear();
        inputMethod().disconnectObject(this);
        this._device.run_dispose();
    }
}
