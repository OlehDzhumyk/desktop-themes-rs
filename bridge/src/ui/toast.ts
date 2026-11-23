// SPDX-License-Identifier: GPL-3.0-or-later
// The shared toast: a short, skin-styled message near the bottom of the current monitor
// («Copied #A1B2C3», «Recognising text…»). It never takes input (reactive: false), is announced to
// screen readers as a notification, and a new message replaces the one on screen. `busy` adds a
// spinner (a static label under reduced motion) and stays until `show` or `hide`; `show` hides
// itself after a delay.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Spinner} from 'resource:///org/gnome/shell/ui/animation.js';

import {animationsEnabled, syncSystemClasses} from './primitives.js';

/** How long a message stays on screen when the caller gives no duration. */
const SHOW_MS = 1600;
const FADE_MS = 80;

export interface DeskToastOptions {
    /** Extra style class for a variant of the look (for example `desk-toast-pill`). */
    variant?: string;
    /** Fade in and out (only when animations are enabled). */
    fade?: boolean;
    /** Where the bottom edge sits, as a fraction of the monitor height. */
    bottom?: number;
}

export class DeskToast {
    private _actor: St.BoxLayout;
    private _label: St.Label;
    private _spinner: Spinner | null = null;
    private _variant: string;
    private _fade: boolean;
    private _bottom: number;
    private _hideId = 0;
    /** The actor was destroyed: by destroy(), or by the shell tearing the stage down. */
    private _gone = false;

    constructor(options: DeskToastOptions = {}) {
        this._variant = options.variant ?? '';
        this._fade = options.fade ?? false;
        this._bottom = options.bottom ?? 0.9;
        this._actor = new St.BoxLayout({
            style_class: 'desk-toast',
            reactive: false,
            visible: false,
            accessible_role: Atk.Role.NOTIFICATION,
        });
        this._label = new St.Label({style_class: 'desk-toast-label', y_align: Clutter.ActorAlign.CENTER});
        this._label.clutter_text.line_wrap = true;
        this._actor.add_child(this._label);
        this._actor.label_actor = this._label;
        this._actor.connect('destroy', () => { this._gone = true; });
        Main.layoutManager.addTopChrome(this._actor);
    }

    /** A message that hides itself after `durationMs`; it replaces a waiting state. */
    show(text: string, skin: string, durationMs = SHOW_MS): void {
        if (this._gone)
            return;
        this._cancelHide();
        this._spinner?.stop();
        this._present(text, skin);
        this._hideId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, durationMs, () => {
            this._hideId = 0;
            this.hide();
            return GLib.SOURCE_REMOVE;
        });
    }

    /** The waiting state: shown until [show] replaces it or [hide] removes it. */
    busy(text: string, skin: string): void {
        if (this._gone)
            return;
        this._cancelHide();
        if (animationsEnabled() && !this._spinner) {
            this._spinner = new Spinner(16, {animate: true, hideOnStop: true});
            this._actor.insert_child_at_index(this._spinner, 0);
        }
        this._spinner?.play();
        this._present(text, skin);
    }

    hide(): void {
        if (this._gone)
            return;
        this._cancelHide();
        this._spinner?.stop();
        const a = this._actor;
        if (!this._fade || !animationsEnabled() || !a.visible) {
            a.remove_all_transitions();
            a.hide();
            return;
        }
        a.ease({
            opacity: 0, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onComplete: () => {
                if (!this._gone)
                    a.hide();
            },
        });
    }

    private _present(text: string, skin: string): void {
        const a = this._actor;
        a.style_class = `desk-toast${this._variant ? ` ${this._variant}` : ''} desk-skin-${skin}`;
        syncSystemClasses(a);
        this._label.text = text;
        a.accessible_name = text;
        a.remove_all_transitions();
        a.show();
        const m = Main.layoutManager.currentMonitor;
        if (m) {
            a.set_width(-1);
            const [, natW] = a.get_preferred_width(-1);
            const width = Math.min(natW, Math.round(m.width * 0.8));
            a.set_width(width);
            const [, natH] = a.get_preferred_height(width);
            a.set_position(Math.round(m.x + (m.width - width) / 2), Math.round(m.y + m.height * this._bottom - natH));
        }
        if (this._fade && animationsEnabled()) {
            a.opacity = 0;
            a.ease({opacity: 255, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        } else {
            a.opacity = 255;
        }
    }

    private _cancelHide(): void {
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = 0;
    }

    destroy(): void {
        this._cancelHide();
        if (this._gone)
            return;
        this._spinner?.stop();
        this._spinner = null;
        this._actor.remove_all_transitions();
        Main.layoutManager.removeChrome(this._actor);
        this._actor.destroy();
    }
}
