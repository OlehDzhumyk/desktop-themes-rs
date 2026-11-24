// SPDX-License-Identifier: GPL-3.0-or-later
// The zone preview (docs/spec/features/window-layouts.md): a translucent rectangle on the screen
// where the window will go. It never takes input (reactive: false), has an outline with at least
// 3:1 contrast (a light line inside a dark one, so it shows on any wallpaper), and fades in
// within 100 ms; under reduced motion it appears at once (AC-WIN-06).
import Clutter from 'gi://Clutter';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {animationsEnabled, syncSystemClasses} from '../ui/primitives.js';
import type {Rect} from './client.js';

const FADE_MS = 80;

export class ZonePreview {
    private _actor: St.Widget;
    private _shown: Rect | null = null;
    private _gone = false;

    /** Created before the picker's layer, so it sits under the picker and over the windows. */
    constructor(skin: string) {
        this._actor = new St.Widget({
            style_class: `desk-layouts-preview desk-skin-${skin}`,
            reactive: false,
            visible: false,
        });
        syncSystemClasses(this._actor);
        this._actor.connect('destroy', () => { this._gone = true; });
        Main.layoutManager.addTopChrome(this._actor);
    }

    show(rect: Rect): void {
        if (this._gone)
            return;
        const s = this._shown;
        if (s && this._actor.visible && s.x === rect.x && s.y === rect.y && s.w === rect.w && s.h === rect.h)
            return;
        this._shown = rect;
        const a = this._actor;
        a.remove_all_transitions();
        a.set_position(rect.x, rect.y);
        a.set_size(rect.w, rect.h);
        a.show();
        if (animationsEnabled()) {
            a.opacity = 0;
            a.ease({opacity: 255, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        } else {
            a.opacity = 255;
        }
    }

    hide(): void {
        if (this._gone)
            return;
        this._shown = null;
        this._actor.remove_all_transitions();
        this._actor.hide();
    }

    destroy(): void {
        if (this._gone)
            return;
        this._actor.remove_all_transitions();
        Main.layoutManager.removeChrome(this._actor);
        this._actor.destroy();
    }
}
