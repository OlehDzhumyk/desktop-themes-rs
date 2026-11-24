// SPDX-License-Identifier: GPL-3.0-or-later
// Screen recording (docs/spec/features/screenshots.md, AC-SHOT-07) through GNOME Shell's own
// screencast service (org.gnome.Shell.Screencast, a helper process of the shell; it accepts calls
// from the shell's own connection, which the bridge uses). While a recording runs, a compact item
// in the top bar shows a red dot and the elapsed time; a click (or Enter) stops the recording.
// GNOME picks the encoder and the file extension; the file goes to ~/Videos/Screencasts.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';

import {_} from '../i18n.js';
import {dbusCall} from '../trust.js';

const SCREENCAST_BUS = 'org.gnome.Shell.Screencast';
const SCREENCAST_PATH = '/org/gnome/Shell/Screencast';
const SCREENCAST_IFACE = 'org.gnome.Shell.Screencast';
const ROLE = 'desk-capture-recording';

/** Elapsed time as m:ss or h:mm:ss. */
function elapsedText(seconds: number): string {
    const total = Math.max(0, Math.floor(seconds));
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor((total % 3600) / 60);
    const secs = String(total % 60).padStart(2, '0');
    return hours > 0
        ? `${String(hours)}:${String(minutes).padStart(2, '0')}:${secs}`
        : `${String(minutes)}:${secs}`;
}

/** The folder of screen recordings (created when needed): Screencasts in the videos folder. */
function screencastDir(): string {
    const videos = GLib.get_user_special_dir(GLib.UserDirectory.DIRECTORY_VIDEOS) ??
        GLib.build_filenamev([GLib.get_home_dir(), 'Videos']);
    const dir = GLib.build_filenamev([videos, 'Screencasts']);
    GLib.mkdir_with_parents(dir, 0o700);
    return dir;
}

export interface RecorderEvents {
    /** The recording was stopped and saved at `path`. */
    saved(path: string): void;
    /** Starting or recording failed; `message` is for the user. */
    failed(message: string): void;
}

export class Recorder {
    private _events: RecorderEvents;
    private _state: 'idle' | 'starting' | 'recording' | 'stopping' = 'idle';
    private _path = '';
    private _startedAt = 0;
    private _indicator: PanelMenu.Button | null = null;
    private _timeLabel: St.Label | null = null;
    private _tickId = 0;
    private _errorSub = 0;
    private _destroyed = false;

    constructor(events: RecorderEvents) {
        this._events = events;
        // the service reports a recording that broke off (for example the encoder failed)
        this._errorSub = Gio.DBus.session.signal_subscribe(SCREENCAST_BUS, SCREENCAST_IFACE, 'Error',
            SCREENCAST_PATH, null, Gio.DBusSignalFlags.NONE, () => {
                if (this._state !== 'recording')
                    return;
                this._reset();
                this._events.failed(_('The screen recording stopped because of an error'));
            });
    }

    /** A recording runs (or is starting): the record action stops it. */
    get active(): boolean {
        return this._state !== 'idle';
    }

    toggle(drawCursor: boolean): void {
        if (this._state === 'idle')
            void this._start(drawCursor);
        else if (this._state === 'recording')
            void this.stop();
    }

    private async _start(drawCursor: boolean): Promise<void> {
        this._state = 'starting';
        try {
            const template = GLib.build_filenamev([screencastDir(), 'Screencast %d %t']);
            const reply = await dbusCall(SCREENCAST_BUS, SCREENCAST_PATH, SCREENCAST_IFACE, 'Screencast',
                new GLib.Variant('(sa{sv})', [template, {'draw-cursor': new GLib.Variant('b', drawCursor)}]),
                '(bs)');
            const [ok, path] = reply.deepUnpack<[boolean, string]>();
            if (!ok)
                throw new Error(_('GNOME refused to start the recording'));
            if (this._destroyed) {
                // disabled while the service was starting: never leave a recording running
                void this._stopCall().catch(() => { /* the service is gone: nothing runs */ });
                return;
            }
            this._path = path;
            this._state = 'recording';
            this._startedAt = GLib.get_monotonic_time();
            this._showIndicator();
        } catch (e) {
            this._state = 'idle';
            if (!this._destroyed)
                this._events.failed(e instanceof Error ? e.message : String(e));
        }
    }

    private _stopCall(): Promise<GLib.Variant> {
        return dbusCall(SCREENCAST_BUS, SCREENCAST_PATH, SCREENCAST_IFACE, 'StopScreencast',
            new GLib.Variant('()', []), '(b)');
    }

    async stop(): Promise<void> {
        if (this._state !== 'recording')
            return;
        this._state = 'stopping';
        const path = this._path;
        this._hideIndicator();
        try {
            const reply = await this._stopCall();
            const [ok] = reply.deepUnpack<[boolean]>();
            this._reset();
            if (this._destroyed)
                return;
            if (ok)
                this._events.saved(path);
            else
                this._events.failed(_('The screen recording could not be saved'));
        } catch (e) {
            this._reset();
            if (!this._destroyed)
                this._events.failed(e instanceof Error ? e.message : String(e));
        }
    }

    private _reset(): void {
        this._state = 'idle';
        this._path = '';
        this._hideIndicator();
    }

    private _showIndicator(): void {
        this._hideIndicator();
        const btn = new PanelMenu.Button(0.0, _('Screen recording'), true);
        btn.add_style_class_name('desk-capture-recording');
        btn.accessible_name = _('Stop the screen recording');
        const box = new St.BoxLayout({style_class: 'desk-capture-recording-box'});
        box.add_child(new St.Icon({icon_name: 'media-record-symbolic', style_class: 'system-status-icon desk-capture-recording-dot'}));
        this._timeLabel = new St.Label({text: elapsedText(0), y_align: Clutter.ActorAlign.CENTER,
            style_class: 'desk-capture-recording-time'});
        box.add_child(this._timeLabel);
        btn.add_child(box);
        btn.connect('button-press-event', () => {
            void this.stop();
            return Clutter.EVENT_STOP;
        });
        btn.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            const sym = ev.get_key_symbol();
            if (sym !== Clutter.KEY_Return && sym !== Clutter.KEY_KP_Enter && sym !== Clutter.KEY_space)
                return Clutter.EVENT_PROPAGATE;
            void this.stop();
            return Clutter.EVENT_STOP;
        });
        Main.panel.addToStatusArea(ROLE, btn, 0, 'right');
        this._indicator = btn;
        this._tickId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
            if (this._timeLabel)
                this._timeLabel.text = elapsedText((GLib.get_monotonic_time() - this._startedAt) / 1e6);
            return GLib.SOURCE_CONTINUE;
        });
    }

    private _hideIndicator(): void {
        if (this._tickId)
            GLib.source_remove(this._tickId);
        this._tickId = 0;
        this._timeLabel = null;
        this._indicator?.destroy();
        this._indicator = null;
    }

    /** Stops a running recording (the file is kept), removes the indicator and the subscription. */
    destroy(): void {
        this._destroyed = true;
        if (this._state === 'recording')
            void this._stopCall().catch(() => { /* the service is gone: nothing runs */ });
        this._reset();
        if (this._errorSub)
            Gio.DBus.session.signal_unsubscribe(this._errorSub);
        this._errorSub = 0;
    }
}
