// SPDX-License-Identifier: GPL-3.0-or-later
// A short message near the bottom of the current monitor («Copied #A1B2C3»), styled by the skin.
// It takes no input and hides itself; a new message replaces the one on screen.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {animationsEnabled, syncSystemClasses} from '../clipboard/panel.js';

const SHOW_MS = 1600;
const FADE_MS = 80;

export class Toast {
    private _label: St.Label;
    private _timeout = 0;
    private _gone = false;

    constructor() {
        this._label = new St.Label({style_class: 'desk-color-toast', visible: false, reactive: false});
        this._label.accessible_role = Atk.Role.NOTIFICATION;
        this._label.connect('destroy', () => { this._gone = true; });
        Main.layoutManager.addTopChrome(this._label);
    }

    show(text: string, skin: string): void {
        if (this._gone)
            return;
        const mon = Main.layoutManager.currentMonitor;
        if (!mon)
            return;
        const l = this._label;
        l.style_class = `desk-color-toast desk-skin-${skin}`;
        syncSystemClasses(l);
        l.text = text;
        l.remove_all_transitions();
        l.show();
        const [, natW] = l.get_preferred_width(-1);
        const [, natH] = l.get_preferred_height(natW);
        l.set_position(Math.round(mon.x + (mon.width - natW) / 2), Math.round(mon.y + mon.height * 0.82 - natH));
        if (animationsEnabled()) {
            l.opacity = 0;
            l.ease({opacity: 255, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        } else {
            l.opacity = 255;
        }
        this._clearTimeout();
        this._timeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, SHOW_MS, () => {
            this._timeout = 0;
            this._hide();
            return GLib.SOURCE_REMOVE;
        });
    }

    private _hide(): void {
        if (this._gone)
            return;
        const l = this._label;
        if (!animationsEnabled()) {
            l.hide();
            return;
        }
        l.ease({
            opacity: 0, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onComplete: () => {
                if (!this._gone)
                    l.hide();
            },
        });
    }

    private _clearTimeout(): void {
        if (this._timeout)
            GLib.source_remove(this._timeout);
        this._timeout = 0;
    }

    destroy(): void {
        this._clearTimeout();
        if (this._gone)
            return;
        this._label.remove_all_transitions();
        Main.layoutManager.removeChrome(this._label);
        this._label.destroy();
    }
}
