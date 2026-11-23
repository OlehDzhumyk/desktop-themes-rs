// SPDX-License-Identifier: GPL-3.0-or-later
// The small status toast of the text extractor: «Recognising text…» with a spinner (a static
// label under reduced motion), then the result. It never takes input (reactive: false) and is
// announced to screen readers as a notification.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Spinner} from 'resource:///org/gnome/shell/ui/animation.js';

import {animationsEnabled} from '../ui/primitives.js';

/** How long a result stays on screen. */
const RESULT_SECONDS = 4;

export class OcrToast {
    private _actor: St.BoxLayout;
    private _spinner: Spinner | null = null;
    private _label: St.Label;
    private _hideId = 0;
    private _destroyed = false;

    constructor(skin: string) {
        this._actor = new St.BoxLayout({
            style_class: `desk-ocr-toast desk-skin-${skin}${St.Settings.get().high_contrast ? ' desk-hc' : ''}`,
            reactive: false,
            visible: false,
            accessible_role: Atk.Role.NOTIFICATION,
        });
        this._label = new St.Label({style_class: 'desk-ocr-toast-label', y_align: Clutter.ActorAlign.CENTER});
        this._label.clutter_text.line_wrap = true;
        this._actor.add_child(this._label);
        this._actor.label_actor = this._label;
        Main.layoutManager.addTopChrome(this._actor);
    }

    /** The waiting state: shown until [show] replaces it or [hide] removes it. */
    busy(text: string): void {
        this._cancelHide();
        if (animationsEnabled() && !this._spinner) {
            this._spinner = new Spinner(16, {animate: true, hideOnStop: true});
            this._actor.insert_child_at_index(this._spinner, 0);
        }
        this._spinner?.play();
        this._present(text);
    }

    /** A result: replaces the waiting state and hides itself after a few seconds. */
    show(text: string): void {
        this._cancelHide();
        this._spinner?.stop();
        this._present(text);
        this._hideId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, RESULT_SECONDS, () => {
            this._hideId = 0;
            this.hide();
            return GLib.SOURCE_REMOVE;
        });
    }

    hide(): void {
        this._cancelHide();
        this._spinner?.stop();
        this._actor.hide();
    }

    private _present(text: string): void {
        this._label.text = text;
        this._actor.accessible_name = text;
        this._actor.show();
        const m = Main.layoutManager.currentMonitor;
        if (m) {
            this._actor.set_width(-1);
            const [, natW] = this._actor.get_preferred_width(-1);
            const width = Math.min(natW, Math.round(m.width * 0.8));
            this._actor.set_width(width);
            const [, natH] = this._actor.get_preferred_height(width);
            this._actor.set_position(Math.round(m.x + (m.width - width) / 2),
                Math.round(m.y + m.height - natH - m.height / 10));
        }
    }

    private _cancelHide(): void {
        if (this._hideId) {
            GLib.source_remove(this._hideId);
            this._hideId = 0;
        }
    }

    destroy(): void {
        if (this._destroyed)
            return;
        this._destroyed = true;
        this._cancelHide();
        this._spinner?.stop();
        this._spinner = null;
        Main.layoutManager.removeChrome(this._actor);
        this._actor.destroy();
    }
}
