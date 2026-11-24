// SPDX-License-Identifier: GPL-3.0-or-later
// The capture toolbar (docs/spec/features/screenshots.md): a modal, dimmed layer over all monitors
// with one bar in the skin's style. windows11: a compact Fluent bar at the top (icons only);
// macos: a capsule near the bottom (like Shift+Cmd+5); default: a shell-styled bar near the bottom.
// Modes Region (1), Screen (2), Record (3), the delay (off, 3 s, 10 s) and Close. Everything works
// with the keyboard alone (AC-SHOT-10): Tab, Shift+Tab and the arrow keys move the focus, Enter or
// Space presses, 1..3 pick a mode, Esc or a right click closes and nothing is captured (AC-SHOT-02).
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_, fmt} from '../i18n.js';
import {pushModal} from '../shellTypes.js';
import {animationsEnabled, syncSystemClasses} from '../ui/primitives.js';

export type CaptureMode = 'region' | 'screen' | 'record';

/** The delays the toolbar cycles through, in seconds (the `delay` setting's choices). */
export const DELAYS = [0, 3, 10] as const;

/** The dim layer fades in and out within this time (AC-SHOT-02: gone within 100 ms). */
const FADE_MS = 80;

function delayLabel(seconds: number): string {
    return seconds > 0 ? fmt(_('Delay: {seconds} s'), {seconds}) : _('Delay: off');
}

/** The next delay of the cycle 0 → 3 → 10 → 0. */
function nextDelay(seconds: number): number {
    const i = DELAYS.findIndex(d => d === seconds);
    return DELAYS[(i + 1) % DELAYS.length] ?? 0;
}

export interface ToolbarState {
    delay: number;
    /** A recording runs: the Record button stops it. */
    recording: boolean;
}

export class CaptureToolbar {
    private _layer: St.Widget;
    private _bar: St.BoxLayout;
    private _buttons: St.Button[] = [];
    private _delayButton: St.Button;
    private _delayLabel: St.Label | null;
    private _skin: string;
    private _delay: number;
    private _grab: Clutter.Grab | null = null;
    private _onChoose: ((mode: CaptureMode, delay: number) => void) | null;
    private _onClose: (() => void) | null;
    private _destroyed = false;

    /** `onChoose` or `onClose` runs once; the toolbar is destroyed before either runs. */
    constructor(skin: string, state: ToolbarState, onChoose: (mode: CaptureMode, delay: number) => void,
        onClose: () => void) {
        this._skin = skin;
        this._delay = state.delay;
        this._onChoose = onChoose;
        this._onClose = onClose;
        this._layer = new St.Widget({
            style_class: `desk-capture-layer desk-skin-${skin}`,
            reactive: true,
            x: 0, y: 0,
            width: global.stage.width,
            height: global.stage.height,
            accessible_role: Atk.Role.WINDOW,
            accessible_name: _('Screenshot toolbar'),
        });
        this._bar = new St.BoxLayout({
            style_class: `desk-capture-toolbar desk-skin-${skin}`,
            reactive: true,
            accessible_role: Atk.Role.TOOL_BAR,
            accessible_name: _('Screenshot toolbar'),
        });
        syncSystemClasses(this._bar);
        this._layer.add_child(this._bar);

        this._addButton('selection-mode-symbolic', _('Capture a region'), '1', () => { this._choose('region'); });
        this._addButton('video-display-symbolic', _('Capture the whole screen'), '2', () => { this._choose('screen'); });
        this._addButton(state.recording ? 'media-playback-stop-symbolic' : 'media-record-symbolic',
            state.recording ? _('Stop the recording') : _('Record the screen'), '3', () => { this._choose('record'); });
        this._bar.add_child(new St.Widget({style_class: 'desk-capture-separator', y_expand: true}));
        [this._delayButton, this._delayLabel] = this._addButton('alarm-symbolic', delayLabel(this._delay), '',
            () => { this._cycleDelay(); });
        this._addButton('window-close-symbolic', _('Close'), 'Esc', () => { this._close(); });

        this._layer.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => this._key(ev));
        this._layer.connect('button-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            if (ev.get_button() === Clutter.BUTTON_SECONDARY)
                this._close();
            return Clutter.EVENT_STOP;
        });
        this._layer.connect('destroy', () => { this._grab = null; });
    }

    /** One button: an icon, and a label except in the compact windows11 bar. */
    private _addButton(icon: string, text: string, key: string, onClick: () => void): [St.Button, St.Label | null] {
        const box = new St.BoxLayout({style_class: 'desk-capture-button-box'});
        box.add_child(new St.Icon({icon_name: icon, style_class: 'desk-capture-button-icon'}));
        let textLabel: St.Label | null = null;
        if (this._skin !== 'windows11') {
            textLabel = new St.Label({text, style_class: 'desk-capture-button-label', y_align: Clutter.ActorAlign.CENTER});
            box.add_child(textLabel);
        }
        const b = new St.Button({
            style_class: 'desk-capture-button',
            can_focus: true,
            reactive: true,
            accessible_name: key ? `${text} (${key})` : text,
            child: box,
        });
        b.connect('clicked', () => { onClick(); });
        this._bar.add_child(b);
        this._buttons.push(b);
        return [b, textLabel];
    }

    /** Show the layer and take the input. False when the shell refused the grab (nothing shown). */
    open(): boolean {
        Main.layoutManager.addTopChrome(this._layer);
        this._layout();
        this._grab = pushModal(this._layer, Shell.ActionMode.POPUP);
        if (!this._grab) {
            this.destroy();
            return false;
        }
        this._buttons[0]?.grab_key_focus();
        if (animationsEnabled()) {
            this._layer.opacity = 0;
            this._layer.ease({opacity: 255, duration: FADE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        }
        return true;
    }

    private _layout(): void {
        const m = Main.layoutManager.currentMonitor;
        if (!m)
            return;
        const [, natW] = this._bar.get_preferred_width(-1);
        const [, natH] = this._bar.get_preferred_height(natW);
        const x = Math.round(m.x + (m.width - natW) / 2);
        let y: number;
        if (this._skin === 'windows11')
            y = m.y + Math.round(m.height / 36);
        else if (this._skin === 'macos')
            y = m.y + m.height - natH - Math.round(m.height / 10);
        else
            y = m.y + m.height - natH - Math.round(m.height / 14);
        this._bar.set_position(x, y);
    }

    private _cycleDelay(): void {
        this._delay = nextDelay(this._delay);
        const text = delayLabel(this._delay);
        this._delayButton.accessible_name = text;
        if (this._delayLabel)
            this._delayLabel.text = text;
        // the bar keeps its place when the label gets longer
        this._layout();
    }

    private _key(ev: Clutter.Event): boolean {
        const sym = ev.get_key_symbol();
        const shift = (ev.get_state() & Clutter.ModifierType.SHIFT_MASK) !== 0;
        const focus = global.stage.key_focus;
        switch (sym) {
        case Clutter.KEY_Escape:
            this._close();
            return Clutter.EVENT_STOP;
        case Clutter.KEY_1:
        case Clutter.KEY_KP_1:
            this._choose('region');
            return Clutter.EVENT_STOP;
        case Clutter.KEY_2:
        case Clutter.KEY_KP_2:
            this._choose('screen');
            return Clutter.EVENT_STOP;
        case Clutter.KEY_3:
        case Clutter.KEY_KP_3:
            this._choose('record');
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Tab:
            this._bar.navigate_focus(focus, shift ? St.DirectionType.TAB_BACKWARD : St.DirectionType.TAB_FORWARD, true);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_ISO_Left_Tab:
            this._bar.navigate_focus(focus, St.DirectionType.TAB_BACKWARD, true);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Left:
        case Clutter.KEY_Up:
            this._bar.navigate_focus(focus, St.DirectionType.LEFT, true);
            return Clutter.EVENT_STOP;
        case Clutter.KEY_Right:
        case Clutter.KEY_Down:
            this._bar.navigate_focus(focus, St.DirectionType.RIGHT, true);
            return Clutter.EVENT_STOP;
        default:
            return Clutter.EVENT_PROPAGATE;
        }
    }

    private _choose(mode: CaptureMode): void {
        const done = this._onChoose;
        const delay = this._delay;
        this._onChoose = null;
        this._onClose = null;
        this.destroy();
        done?.(mode, delay);
    }

    private _close(): void {
        const done = this._onClose;
        this._onChoose = null;
        this._onClose = null;
        this.destroy();
        done?.();
    }

    /** Removes the layer at once: the dim layer must be gone before any pixels are read. */
    destroy(): void {
        if (this._destroyed)
            return;
        this._destroyed = true;
        this._onChoose = null;
        this._onClose = null;
        if (this._grab) {
            Main.popModal(this._grab);
            this._grab = null;
        }
        this._layer.remove_all_transitions();
        if (this._layer.get_parent())
            Main.layoutManager.removeChrome(this._layer);
        this._layer.destroy();
        this._buttons = [];
    }
}
