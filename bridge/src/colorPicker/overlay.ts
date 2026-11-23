// SPDX-License-Identifier: GPL-3.0-or-later
// The eyedropper (docs/spec/features/color-picker.md, «Flow»): a transparent modal surface over
// the whole stage with a crosshair cursor, and a preview pill next to the pointer: a magnifying
// loupe, a swatch, the value in the chosen format and the zoom. The pill flips to the other side
// near the edges of the monitor under the pointer.
//
// Pixels: Shell.Screenshot.pick_color() reads the live pixel under the pointer (the same call as
// org.gnome.Shell.Screenshot PickColor). The loupe magnifies a frame of the stage taken when the
// picker opens (Shell.Screenshot.screenshot_stage_to_content(), kept on the GPU, never written to
// disk); the swatch and the value are live.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {HORIZONTAL, VERTICAL, animationsEnabled, syncSystemClasses} from '../ui/primitives.js';
import {_} from '../i18n.js';
import {errorText, pushModal} from '../shellTypes.js';
import {type ColorFormat, type ColorPickerClient, FORMATS, asFormat} from './client.js';

/** Side of the square loupe, in logical pixels (spec: 120 px, 10x of a 12 px area). */
const LOUPE = 120;
/** Distance of the pill from the pointer. */
const OFFSET = 24;
const ZOOM_MIN = 4;
const ZOOM_MAX = 16;
const FADE_MS = 80;

type Monitor = NonNullable<typeof Main.layoutManager.currentMonitor>;

export interface PickerHost {
    readonly client: ColorPickerClient;
    readonly settings: Gio.Settings;
    /** Enter or a click took the colour of a pixel (sRGB, 0..255). */
    picked(r: number, g: number, b: number): void;
    /** H or a right click: show the history of picks instead. */
    openHistory(): void;
}

/** #RRGGBB, as deskd writes it. */
export function toHex(r: number, g: number, b: number): string {
    return `#${[r, g, b].map(v => v.toString(16).padStart(2, '0')).join('').toUpperCase()}`;
}

/** Short name of a format for chips and buttons (not translated: these are technical terms). */
export function formatLabel(f: ColorFormat): string {
    return f === 'name' ? 'CSS' : f === 'decimal' ? 'r,g,b' : f.toUpperCase();
}

function monitorAt(x: number, y: number): Monitor | null {
    return Main.layoutManager.monitors.find(m =>
        x >= m.x && y >= m.y && x < m.x + m.width && y < m.y + m.height) ?? null;
}

export class PickerOverlay {
    readonly actor: St.Widget;
    private _pill: St.BoxLayout;
    private _loupe: St.Widget;
    private _zoomed: Clutter.Actor;
    private _center: St.Widget;
    private _swatch: St.Widget;
    private _value: St.Label;
    private _formatChip: St.Label;
    private _zoomLabel: St.Label;
    /** Live samples, one at a time. */
    private _sampler = new Shell.Screenshot();
    private _grab: Clutter.Grab | null = null;
    private _skin = 'default';
    /** Increases on every open and close: replies of an earlier session are dropped. */
    private _session = 0;
    private _x = 0;
    private _y = 0;
    private _zoom = 10;
    private _format: ColorFormat = 'hex';
    private _color: [number, number, number] | null = null;
    private _shownHex = '';
    private _textSerial = 0;
    /** deskd did not answer Format(): the pill shows #RRGGBB for the rest of the session. */
    private _formatBroken = false;
    private _sampling = false;
    private _resample = false;
    private _picking = false;
    private _snapshotSize: [number, number] | null = null;
    private _gone = false;

    constructor(private _host: PickerHost) {
        this.actor = new St.Widget({reactive: true, visible: false, style_class: 'desk-color-backdrop'});
        this.actor.set_cursor_type(Clutter.CursorType.CROSSHAIR);
        this.actor.accessible_role = Atk.Role.DIALOG;
        this.actor.accessible_name = _('Colour picker');

        this._pill = new St.BoxLayout({...VERTICAL, style_class: 'desk-color-pill', visible: false});
        this._loupe = new St.Widget({
            style_class: 'desk-color-loupe', width: LOUPE, height: LOUPE, clip_to_allocation: true,
        });
        this._zoomed = new Clutter.Actor();
        this._zoomed.set_content_scaling_filters(Clutter.ScalingFilter.NEAREST, Clutter.ScalingFilter.NEAREST);
        this._center = new St.Widget({style_class: 'desk-color-loupe-center'});
        this._loupe.add_child(this._zoomed);
        this._loupe.add_child(this._center);
        this._pill.add_child(this._loupe);

        const row = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-color-pill-row'});
        this._swatch = new St.Widget({style_class: 'desk-color-swatch', y_align: Clutter.ActorAlign.CENTER});
        this._value = new St.Label({
            style_class: 'desk-color-value', can_focus: true, y_align: Clutter.ActorAlign.CENTER, x_expand: true,
        });
        row.add_child(this._swatch);
        row.add_child(this._value);
        this._pill.add_child(row);

        const meta = new St.BoxLayout({...HORIZONTAL, style_class: 'desk-color-pill-meta'});
        this._formatChip = new St.Label({style_class: 'desk-color-chip', y_align: Clutter.ActorAlign.CENTER});
        this._zoomLabel = new St.Label({style_class: 'desk-color-chip', y_align: Clutter.ActorAlign.CENTER});
        meta.add_child(this._formatChip);
        meta.add_child(this._zoomLabel);
        this._pill.add_child(meta);
        this._pill.add_child(new St.Label({
            style_class: 'desk-color-hint',
            text: _('Enter picks · F format · H history · Esc cancels'),
        }));
        this.actor.add_child(this._pill);

        this.actor.connect('motion-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            const [x, y] = ev.get_coords();
            this._moveTo(x, y);
            return Clutter.EVENT_STOP;
        });
        this.actor.connect('button-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            const [x, y] = ev.get_coords();
            this._moveTo(x, y);
            const button = ev.get_button();
            if (button === Clutter.BUTTON_PRIMARY)
                this._pick();
            else if (button === Clutter.BUTTON_SECONDARY)
                this._history();
            return Clutter.EVENT_STOP;
        });
        this.actor.connect('scroll-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            const dir = ev.get_scroll_direction();
            if (dir === Clutter.ScrollDirection.UP)
                this._setZoom(this._zoom + 1);
            else if (dir === Clutter.ScrollDirection.DOWN)
                this._setZoom(this._zoom - 1);
            else if (dir === Clutter.ScrollDirection.SMOOTH) {
                const [, dy] = ev.get_scroll_delta();
                if (dy !== 0)
                    this._setZoom(this._zoom + (dy < 0 ? 1 : -1));
            }
            return Clutter.EVENT_STOP;
        });
        this.actor.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._onKey(ev));
        this.actor.connect('destroy', () => {
            this._gone = true;
            this._grab = null;
        });
        Main.layoutManager.addTopChrome(this.actor);
    }

    get isOpen(): boolean {
        return !this._gone && this.actor.visible;
    }

    setSkin(skin: string): void {
        this._skin = skin;
        this._pill.style_class = `desk-color-pill desk-skin-${skin}`;
    }

    open(): void {
        if (this.isOpen)
            return;
        const session = ++this._session;
        this.actor.set_position(0, 0);
        this.actor.set_size(global.stage.width, global.stage.height);
        this._pill.hide();
        this.actor.show();
        this._grab = pushModal(this.actor, Shell.ActionMode.POPUP);
        if (!this._grab) {
            this.actor.hide();
            return;
        }
        this.setSkin(this._skin);
        syncSystemClasses(this._pill);
        this._format = asFormat(this._host.settings.get_string('format'));
        this._zoom = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, this._host.settings.get_uint('zoom')));
        this._color = null;
        this._shownHex = '';
        this._formatBroken = false;
        this._picking = false;
        this._value.text = '…';
        this._swatch.style = null;
        this._syncChips();
        const [x, y] = global.get_pointer();
        this._x = x;
        this._y = y;
        this._value.grab_key_focus();
        this._sample();
        // the frame for the loupe is taken while the pill is hidden, so the pill is not in it
        new Shell.Screenshot().screenshot_stage_to_content((shot, res) => {
            if (session !== this._session || !this.isOpen || !shot)
                return;
            try {
                const [content] = shot.screenshot_stage_to_content_finish(res);
                const [ok, w, h] = content.get_preferred_size();
                this._zoomed.content = content;
                this._snapshotSize = ok && w > 0 && h > 0 ? [w, h] : null;
            } catch (e) {
                // no loupe then: the swatch and the value still work
                console.warn(`desk-bridge: colour picker loupe: ${errorText(e)}`);
                this._snapshotSize = null;
            }
            this._loupe.visible = this._snapshotSize !== null;
            this._showPill();
        });
    }

    private _showPill(): void {
        this._updateLoupe();
        this._pill.show();
        this._placePill();
        this._pill.remove_all_transitions();
        if (animationsEnabled()) {
            this._pill.opacity = 0;
            this._pill.ease({opacity: 255, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        } else {
            this._pill.opacity = 255;
        }
    }

    close(): void {
        if (!this.isOpen)
            return;
        this._session++;
        if (this._grab)
            Main.popModal(this._grab);
        this._grab = null;
        this._pill.remove_all_transitions();
        this.actor.hide();
        // free the frame of the stage at once
        this._zoomed.set_content(null);
        this._snapshotSize = null;
    }

    private _onKey(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const step = (ev.get_state() & Clutter.ModifierType.SHIFT_MASK) !== 0 ? 10 : 1;
        switch (sym) {
        case Clutter.KEY_Escape:
            this.close();
            break;
        case Clutter.KEY_Left:
        case Clutter.KEY_KP_Left:
            this._nudge(-step, 0);
            break;
        case Clutter.KEY_Right:
        case Clutter.KEY_KP_Right:
            this._nudge(step, 0);
            break;
        case Clutter.KEY_Up:
        case Clutter.KEY_KP_Up:
            this._nudge(0, -step);
            break;
        case Clutter.KEY_Down:
        case Clutter.KEY_KP_Down:
            this._nudge(0, step);
            break;
        case Clutter.KEY_Return:
        case Clutter.KEY_KP_Enter:
        case Clutter.KEY_space:
            this._pick();
            break;
        case Clutter.KEY_f:
        case Clutter.KEY_F:
            this._cycleFormat();
            break;
        case Clutter.KEY_h:
        case Clutter.KEY_H:
            this._history();
            break;
        case Clutter.KEY_plus:
        case Clutter.KEY_equal:
        case Clutter.KEY_KP_Add:
            this._setZoom(this._zoom + 1);
            break;
        case Clutter.KEY_minus:
        case Clutter.KEY_underscore:
        case Clutter.KEY_KP_Subtract:
            this._setZoom(this._zoom - 1);
            break;
        default:
            break;
        }
        // modal: no key reaches the windows below while picking
        return Clutter.EVENT_STOP;
    }

    /** Arrow keys: move the pointer itself, so a pick works without a mouse (AC-COLOR-06). */
    private _nudge(dx: number, dy: number): void {
        const x = Math.min(global.stage.width - 1, Math.max(0, this._x + dx));
        const y = Math.min(global.stage.height - 1, Math.max(0, this._y + dy));
        Clutter.get_default_backend().get_default_seat().warp_pointer(x, y);
        this._moveTo(x, y);
    }

    private _moveTo(x: number, y: number): void {
        const nx = Math.floor(x);
        const ny = Math.floor(y);
        if (nx === this._x && ny === this._y)
            return;
        this._x = nx;
        this._y = ny;
        this._updateLoupe();
        this._placePill();
        this._sample();
    }

    /** Read the pixel under the pointer; while a read runs, only the newest position waits. */
    private _sample(): void {
        if (this._sampling) {
            this._resample = true;
            return;
        }
        const session = this._session;
        const [x, y] = [this._x, this._y];
        if (!monitorAt(x, y)) {
            this._setColor(null, _('No colour here'));
            return;
        }
        this._sampling = true;
        this._sampler.pick_color(x, y, (_s, res) => {
            this._sampling = false;
            if (session === this._session && this.isOpen) {
                try {
                    const [ok, c] = this._sampler.pick_color_finish(res);
                    if (ok && c.alpha > 0)
                        this._setColor([c.red, c.green, c.blue], '');
                    else
                        this._setColor(null, _('No colour here'));
                } catch (e) {
                    console.warn(`desk-bridge: colour picker: ${errorText(e)}`);
                    this._setColor(null, _('Could not read the colour'));
                }
            }
            // a newer position (or a new session) waited for this read
            if (this._resample) {
                this._resample = false;
                if (this.isOpen)
                    this._sample();
            }
        });
    }

    private _setColor(color: [number, number, number] | null, problem: string): void {
        this._color = color;
        if (!color) {
            this._shownHex = '';
            this._textSerial++;
            this._swatch.style = null;
            this._value.text = problem;
            return;
        }
        const hex = toHex(...color);
        if (hex === this._shownHex)
            return;
        this._shownHex = hex;
        this._swatch.style = `background-color: ${hex};`;
        this._showText(color, hex);
    }

    /** #RRGGBB at once; the text in another format when deskd answers. */
    private _showText([r, g, b]: [number, number, number], hex: string): void {
        const serial = ++this._textSerial;
        this._value.text = hex;
        if (this._format === 'hex' || this._formatBroken)
            return;
        const session = this._session;
        this._host.client.format(r, g, b, this._format)
            .then(text => {
                if (serial === this._textSerial && session === this._session && !this._gone)
                    this._value.text = text;
            })
            .catch((e: unknown) => {
                if (session !== this._session)
                    return;
                this._formatBroken = true;
                console.warn(`desk-bridge: colour picker Format: ${errorText(e)}`);
            });
    }

    private _cycleFormat(): void {
        const i = FORMATS.indexOf(this._format);
        this._format = FORMATS[(i + 1) % FORMATS.length] ?? 'hex';
        // the setting in force: deskd formats the pick and the history in it
        this._host.settings.set_string('format', this._format);
        this._syncChips();
        if (this._color)
            this._showText(this._color, toHex(...this._color));
    }

    private _setZoom(zoom: number): void {
        const z = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom));
        if (z === this._zoom)
            return;
        this._zoom = z;
        this._host.settings.set_uint('zoom', z);
        this._syncChips();
        this._updateLoupe();
    }

    private _syncChips(): void {
        this._formatChip.text = formatLabel(this._format);
        this._zoomLabel.text = `${String(this._zoom)}×`;
    }

    /** Place the frame so that the physical pixel under the pointer is in the centre of the loupe. */
    private _updateLoupe(): void {
        const size = this._snapshotSize;
        if (!size)
            return;
        const [cw, ch] = size;
        const k = cw / Math.max(1, global.stage.width); // physical pixels per logical pixel
        const z = this._zoom;
        const px = Math.floor(this._x * k);
        const py = Math.floor(this._y * k);
        const half = Math.round((LOUPE - z) / 2);
        this._zoomed.set_size(cw * z, ch * z);
        this._zoomed.set_position(half - px * z, half - py * z);
        this._center.set_size(z, z);
        this._center.set_position(half, half);
    }

    private _placePill(): void {
        if (!this._pill.visible)
            return;
        const mon = monitorAt(this._x, this._y) ?? Main.layoutManager.currentMonitor;
        if (!mon)
            return;
        const [, w] = this._pill.get_preferred_width(-1);
        const [, h] = this._pill.get_preferred_height(w);
        let x = this._x + OFFSET;
        let y = this._y + OFFSET;
        if (x + w > mon.x + mon.width)
            x = this._x - OFFSET - w;
        if (y + h > mon.y + mon.height)
            y = this._y - OFFSET - h;
        x = Math.min(Math.max(x, mon.x), mon.x + mon.width - w);
        y = Math.min(Math.max(y, mon.y), mon.y + mon.height - h);
        this._pill.set_position(Math.round(x), Math.round(y));
    }

    /** Take the pixel at the pointer now (not the last sample, which may be older). */
    private _pick(): void {
        if (this._picking)
            return;
        const session = this._session;
        const [x, y] = [this._x, this._y];
        if (!monitorAt(x, y)) {
            this._setColor(null, _('No colour here'));
            return;
        }
        this._picking = true;
        const shot = new Shell.Screenshot();
        shot.pick_color(x, y, (_s, res) => {
            this._picking = false;
            if (session !== this._session || !this.isOpen)
                return;
            let color: [number, number, number] | null = null;
            try {
                const [ok, c] = shot.pick_color_finish(res);
                if (ok && c.alpha > 0)
                    color = [c.red, c.green, c.blue];
            } catch (e) {
                console.warn(`desk-bridge: colour picker: ${errorText(e)}`);
                this._setColor(null, _('Could not read the colour'));
                return;
            }
            if (!color) {
                // a transparent or empty region: refused, nothing is copied
                this._setColor(null, _('No colour here'));
                return;
            }
            this.close();
            this._host.picked(...color);
        });
    }

    private _history(): void {
        this.close();
        this._host.openHistory();
    }

    destroy(): void {
        if (this._gone)
            return;
        this.close();
        Main.layoutManager.removeChrome(this.actor);
        this.actor.destroy();
    }
}
