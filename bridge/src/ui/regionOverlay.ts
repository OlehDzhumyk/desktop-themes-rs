// SPDX-License-Identifier: GPL-3.0-or-later
// The shared region selector (text extractor, docs/spec/features/ocr.md; screenshots): a modal,
// dimmed layer over all monitors with a crosshair cursor. Drag with the pointer, or use the keyboard: an arrow key
// shows a rectangle in the middle of the current monitor, arrows move it, Shift+arrows resize it
// (Ctrl: 1 px steps), Enter confirms. Esc or a right click cancels and nothing runs (AC-OCR-07).
// Regions smaller than 8 px on a side are ignored. `captureRegion` takes the PNG of the selection.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {pushModal} from '../shellTypes.js';

/** A region in stage coordinates (logical pixels). */
export interface Region {
    x: number;
    y: number;
    width: number;
    height: number;
}

/** What the caller says to the user: the layer's accessible name and the visible hint. */
export interface RegionOverlayText {
    accessibleName: string;
    hint: string;
}

/** Width and height from the IHDR chunk of a PNG (deskd checks them against the image). */
export function pngSize(png: Uint8Array): [number, number] | null {
    if (png.length < 24)
        return null;
    const view = new DataView(png.buffer, png.byteOffset, png.byteLength);
    return [view.getUint32(16), view.getUint32(20)];
}

/** The PNG of a region of the stage, at physical pixels (two monitors with different scaling). */
export function captureRegion(region: Region): Promise<Uint8Array> {
    const stream = Gio.MemoryOutputStream.new_resizable();
    const shooter = new Shell.Screenshot();
    return new Promise((resolve, reject) => {
        shooter.screenshot_area(region.x, region.y, region.width, region.height, stream,
            (_s: Shell.Screenshot | null, res: Gio.AsyncResult) => {
                try {
                    shooter.screenshot_area_finish(res);
                    stream.close(null);
                    resolve(stream.steal_as_bytes().toArray());
                } catch (e) {
                    reject(e instanceof Error ? e : new Error(String(e)));
                }
            });
    });
}

/** Smaller selections are taken as a click and ignored. */
const MIN_REGION = 8;
const STEP = 10;

export class RegionOverlay {
    private _actor: St.Widget;
    /** top, bottom, left, right: the dimmed parts around the selection */
    private _dims: St.Widget[];
    private _frame: St.Widget;
    private _hint: St.Label;
    private _grab: Clutter.Grab | null = null;
    private _start: [number, number] | null = null;
    private _region: Region | null = null;
    private _onDone: ((region: Region | null) => void) | null;
    private _destroyed = false;

    /** `onDone` runs once: with the region, or with null when the selection was cancelled. */
    constructor(skin: string, text: RegionOverlayText, onDone: (region: Region | null) => void) {
        this._onDone = onDone;
        this._actor = new St.Widget({
            style_class: `desk-region-overlay desk-skin-${skin}${St.Settings.get().high_contrast ? ' desk-hc' : ''}`,
            reactive: true,
            can_focus: true,
            x: 0, y: 0,
            width: global.stage.width,
            height: global.stage.height,
            cursor_type: Clutter.CursorType.CROSSHAIR,
            accessible_role: Atk.Role.WINDOW,
            accessible_name: text.accessibleName,
        });
        this._dims = [0, 1, 2, 3].map(() => new St.Widget({style_class: 'desk-region-dim'}));
        for (const d of this._dims)
            this._actor.add_child(d);
        this._frame = new St.Widget({style_class: 'desk-region-selection', visible: false});
        this._actor.add_child(this._frame);
        this._hint = new St.Label({
            style_class: 'desk-region-hint',
            text: text.hint,
        });
        this._actor.add_child(this._hint);
        this._actor.connect('button-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._press(ev));
        this._actor.connect('motion-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._motion(ev));
        this._actor.connect('button-release-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._release(ev));
        this._actor.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._key(ev));
        this._actor.connect('destroy', () => { this._grab = null; });
    }

    /** Show the layer and take the input. False when the shell refused the grab (nothing shown). */
    open(): boolean {
        Main.layoutManager.addTopChrome(this._actor);
        this._layout();
        this._grab = pushModal(this._actor, Shell.ActionMode.POPUP);
        if (!this._grab) {
            this.destroy();
            return false;
        }
        this._actor.grab_key_focus();
        return true;
    }

    private _finish(region: Region | null): void {
        const done = this._onDone;
        this._onDone = null;
        this.destroy();
        done?.(region);
    }

    private _press(ev: Clutter.Event): boolean {
        if (ev.get_button() === Clutter.BUTTON_SECONDARY) {
            this._finish(null);
            return Clutter.EVENT_STOP;
        }
        if (ev.get_button() !== Clutter.BUTTON_PRIMARY)
            return Clutter.EVENT_STOP;
        const [x, y] = ev.get_coords();
        this._start = [x, y];
        this._setRegion({x, y, width: 0, height: 0});
        return Clutter.EVENT_STOP;
    }

    private _motion(ev: Clutter.Event): boolean {
        if (!this._start)
            return Clutter.EVENT_PROPAGATE;
        const [x, y] = ev.get_coords();
        const [sx, sy] = this._start;
        this._setRegion({
            x: Math.min(sx, x), y: Math.min(sy, y),
            width: Math.abs(x - sx), height: Math.abs(y - sy),
        });
        return Clutter.EVENT_STOP;
    }

    private _release(ev: Clutter.Event): boolean {
        if (!this._start || ev.get_button() !== Clutter.BUTTON_PRIMARY)
            return Clutter.EVENT_STOP;
        this._start = null;
        this._confirm();
        return Clutter.EVENT_STOP;
    }

    /** Take the region if it is large enough; otherwise forget it and keep selecting. */
    private _confirm(): void {
        const r = this._region;
        if (r && r.width >= MIN_REGION && r.height >= MIN_REGION) {
            this._finish({
                x: Math.round(r.x), y: Math.round(r.y),
                width: Math.round(r.width), height: Math.round(r.height),
            });
            return;
        }
        this._setRegion(null);
    }

    private _key(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const state = ev.get_state();
        if (sym === Clutter.KEY_Escape) {
            this._finish(null);
            return Clutter.EVENT_STOP;
        }
        if (sym === Clutter.KEY_Return || sym === Clutter.KEY_KP_Enter || sym === Clutter.KEY_space) {
            this._confirm();
            return Clutter.EVENT_STOP;
        }
        const moves: Record<number, [number, number]> = {
            [Clutter.KEY_Left]: [-1, 0], [Clutter.KEY_Right]: [1, 0],
            [Clutter.KEY_Up]: [0, -1], [Clutter.KEY_Down]: [0, 1],
        };
        const move = moves[sym];
        if (!move)
            return Clutter.EVENT_PROPAGATE;
        const step = state & Clutter.ModifierType.CONTROL_MASK ? 1 : STEP;
        const r = this._region ?? this._keyboardStart();
        const [dx, dy] = [move[0] * step, move[1] * step];
        if (state & Clutter.ModifierType.SHIFT_MASK)
            this._setRegion({...r, width: Math.max(MIN_REGION, r.width + dx), height: Math.max(MIN_REGION, r.height + dy)});
        else
            this._setRegion({...r, x: r.x + dx, y: r.y + dy});
        return Clutter.EVENT_STOP;
    }

    /** The first keyboard rectangle: a third of the current monitor, in its middle. */
    private _keyboardStart(): Region {
        const m = Main.layoutManager.currentMonitor;
        const [x, y, w, h] = m ? [m.x, m.y, m.width, m.height] : [0, 0, global.stage.width, global.stage.height];
        const width = Math.round(w / 3);
        const height = Math.round(h / 6);
        return {x: x + Math.round((w - width) / 2), y: y + Math.round((h - height) / 2), width, height};
    }

    private _setRegion(r: Region | null): void {
        if (r) {
            // keep it on the stage
            const sw = global.stage.width;
            const sh = global.stage.height;
            const width = Math.min(r.width, sw);
            const height = Math.min(r.height, sh);
            r = {
                x: Math.max(0, Math.min(r.x, sw - width)), y: Math.max(0, Math.min(r.y, sh - height)),
                width, height,
            };
        }
        this._region = r;
        this._layout();
    }

    private _layout(): void {
        const sw = global.stage.width;
        const sh = global.stage.height;
        const [top, bottom, left, right] = this._dims;
        const r = this._region;
        if (!r || !top || !bottom || !left || !right) {
            top?.set_position(0, 0);
            top?.set_size(sw, sh);
            for (const d of [bottom, left, right])
                d?.set_size(0, 0);
            this._frame.hide();
        } else {
            top.set_position(0, 0);
            top.set_size(sw, r.y);
            bottom.set_position(0, r.y + r.height);
            bottom.set_size(sw, Math.max(0, sh - r.y - r.height));
            left.set_position(0, r.y);
            left.set_size(r.x, r.height);
            right.set_position(r.x + r.width, r.y);
            right.set_size(Math.max(0, sw - r.x - r.width), r.height);
            this._frame.set_position(r.x, r.y);
            this._frame.set_size(r.width, r.height);
            this._frame.show();
        }
        // the hint sits at the top centre of the current monitor
        const m = Main.layoutManager.currentMonitor;
        if (m) {
            const [, natW] = this._hint.get_preferred_width(-1);
            this._hint.set_position(Math.round(m.x + (m.width - natW) / 2), m.y + Math.round(m.height / 12));
        }
    }

    destroy(): void {
        if (this._destroyed)
            return;
        this._destroyed = true;
        this._onDone = null;
        if (this._grab) {
            Main.popModal(this._grab);
            this._grab = null;
        }
        if (this._actor.get_parent())
            Main.layoutManager.removeChrome(this._actor);
        this._actor.destroy();
    }
}
