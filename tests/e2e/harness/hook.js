// SPDX-License-Identifier: GPL-3.0-or-later
// Test hook loaded INTO the headless GNOME Shell of the end-to-end harness through
// org.gnome.Shell.Eval (the shell runs with --unsafe-mode, inside the test container only):
//   import('file:///.../hook.js').then(m => m.someFunction(args))
// The module stays loaded, so its state (the connect/timeout tracker) lives across calls.
// Idea of loading a helper module into the shell over Eval from ddterm's tests/shellhook.js
// (GPL-3.0-or-later, Copyright 2024 Aleksandr Mezin, https://github.com/ddterm/gnome-shell-extension-ddterm);
// the code here is our own.
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

export const UUID = 'desk-bridge@desktop-themes-rs';

Gio._promisify(Shell.Screenshot.prototype, 'screenshot');

/** Full-screen PNG through Shell.Screenshot (no PipeWire / ScreenCast needed). */
export async function screenshot(path) {
    const file = Gio.File.new_for_path(path);
    const stream = file.replace(null, false, Gio.FileCreateFlags.NONE, null);
    try {
        await new Shell.Screenshot().screenshot(false, stream);
    } finally {
        stream.close(null);
    }
    return path;
}

// ---- keyboard: a virtual keyboard of the shell's own seat (events take the normal path, so
// mutter keybindings and the focused client see them like real key presses)
const NAMED = {
    super: Clutter.KEY_Super_L, shift: Clutter.KEY_Shift_L, ctrl: Clutter.KEY_Control_L,
    control: Clutter.KEY_Control_L, alt: Clutter.KEY_Alt_L, return: Clutter.KEY_Return,
    enter: Clutter.KEY_Return, escape: Clutter.KEY_Escape, esc: Clutter.KEY_Escape,
    tab: Clutter.KEY_Tab, up: Clutter.KEY_Up, down: Clutter.KEY_Down, left: Clutter.KEY_Left,
    right: Clutter.KEY_Right, delete: Clutter.KEY_Delete, backspace: Clutter.KEY_BackSpace,
    space: Clutter.KEY_space, insert: Clutter.KEY_Insert,
};
let _keyboard = null;

function keyval(name) {
    const k = NAMED[name.toLowerCase()];
    if (k !== undefined)
        return k;
    if ([...name].length === 1)
        return Clutter.unicode_to_keysym(name.codePointAt(0));
    throw new Error(`unknown key ${name}`);
}

/** A new virtual keyboard, resolved once the seat has added it (key events sent to a device the
 * seat does not know yet are dropped). */
function newKeyboard(timeoutMs = 5000) {
    const seat = global.stage.context.get_backend().get_default_seat();
    return new Promise((resolve, reject) => {
        let timer = 0, device = null;
        const id = seat.connect('device-added', (_s, dev) => {
            if (dev.get_device_type() !== Clutter.InputDeviceType.KEYBOARD_DEVICE)
                return;
            seat.disconnect(id);
            GLib.source_remove(timer);
            resolve(device);
        });
        timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, timeoutMs, () => {
            seat.disconnect(id);
            reject(new Error('the seat did not add the virtual keyboard'));
            return GLib.SOURCE_REMOVE;
        });
        device = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
    });
}

/**
 * Press and release a combination such as "Shift+Super+v" (all down, then up in reverse).
 * Each call uses a new virtual keyboard: mutter does not apply a keybinding that was added at run
 * time (e.g. by enabling an extension) to a keyboard device that existed before it, so a reused
 * device would miss the shortcut of an extension enabled later in the test.
 */
export async function keys(spec) {
    const vals = spec.split('+').map(keyval);
    const kb = await newKeyboard();
    const t = () => GLib.get_monotonic_time();
    for (const v of vals)
        kb.notify_keyval(t(), v, Clutter.KeyState.PRESSED);
    for (const v of [...vals].reverse())
        kb.notify_keyval(t(), v, Clutter.KeyState.RELEASED);
    // the previous device is released now that a newer one carried these events
    _keyboard?.run_dispose();
    _keyboard = kb;
    return true;
}

/**
 * Let the shell finish what a change queued: one redraw, then idle (ddterm's tests wait the same
 * way, with Meta laters, after the shell starts). Enabling an extension at run time needs this
 * before its keybinding reacts to key events.
 */
export async function settle() {
    const laters = global.compositor.get_laters();
    for (const type of [Meta.LaterType.BEFORE_REDRAW, Meta.LaterType.IDLE]) {
        await new Promise(resolve => {
            laters.add(type, () => {
                resolve(true);
                return GLib.SOURCE_REMOVE;
            });
            global.stage.queue_redraw();
        });
    }
    return true;
}

// ---- the bridge
function bridge() {
    return Main.extensionManager.lookup(UUID)?.stateObj ?? null;
}

/** What the tests check about the bridge: skin, whether the clipboard panel is open, keybinding. */
export function bridgeState() {
    const ext = Main.extensionManager.lookup(UUID);
    const b = ext?.stateObj;
    const clip = b?._clipboard ?? null;
    return {
        state: ext?.state ?? null,
        error: ext?.error ? String(ext.error) : '',
        skin: clip?.skin ?? null,
        panelOpen: clip?._panel?.isOpen ?? false,
        panelClass: clip?._panel?.constructor?.name ?? null,
        keybound: clip?._bound ?? false,
        language: b?._settings?.get_string('language') ?? null,
    };
}

// ---- the accent control of the «Theme» quick settings menu (accentPicker.ts)
function accentItems() {
    return bridge()?._indicator?._themes?._accentSection?._getMenuItems() ?? [];
}

function accentDots() {
    const out = [];
    const walk = a => {
        for (const c of a.get_children()) {
            if (c.has_style_class_name?.('desk-accent-dot'))
                out.push(c);
            walk(c);
        }
    };
    accentItems().forEach(walk);
    return out;
}

/** Which control the skin shows: kind 'switch', 'dots' or 'dropdown', with its state. */
export function accentPicker() {
    const res = {kind: null, label: '', on: null, options: [], dots: []};
    for (const it of accentItems()) {
        if (it instanceof PopupMenu.PopupSwitchMenuItem) {
            Object.assign(res, {kind: 'switch', label: it.label.text, on: it.state});
        } else if (it instanceof PopupMenu.PopupSubMenuMenuItem) {
            res.kind = 'dropdown';
            res.label = it.label.text;
            res.options = it.menu._getMenuItems().map(m => ({
                label: m.label.text, checked: m._ornament === PopupMenu.Ornament.CHECK,
            }));
        }
    }
    const dots = accentDots();
    if (dots.length) {
        res.kind = 'dots';
        res.dots = dots.map(d => ({name: d.accessible_name, checked: d.checked, focusable: d.can_focus}));
    }
    return res;
}

/** Click the dot or activate the drop-down option with this accessible name / label. */
export function pickAccent(name) {
    const dot = accentDots().find(d => d.accessible_name === name);
    if (dot) {
        dot.emit('clicked', 1);
        return true;
    }
    for (const it of accentItems()) {
        const opt = it.menu?._getMenuItems?.().find(m => m.label?.text === name);
        if (opt) {
            opt.activate(Clutter.get_current_event());
            return true;
        }
    }
    throw new Error(`no accent control named ${name}`);
}

/** Open quick settings and the «Theme» menu, then give the n-th dot the key focus. */
export function focusAccentDot(n) {
    Main.panel.statusArea.quickSettings.menu.open(false);
    bridge()._indicator._themes.menu.open(false);
    accentDots()[n].grab_key_focus();
    return global.stage.key_focus?.accessible_name ?? null;
}

/** Resolve once the dot with this accessible name has the key focus (notify::key-focus). */
export function waitFocus(name, timeoutMs = 10000) {
    return new Promise((resolve, reject) => {
        let timer = 0;
        const id = global.stage.connect('notify::key-focus', () => check());
        const finish = ok => {
            global.stage.disconnect(id);
            if (timer)
                GLib.source_remove(timer);
            if (ok)
                resolve(true);
            else
                reject(new Error(`key focus is on ${global.stage.key_focus?.accessible_name}, not ${name}`));
        };
        const check = () => {
            if (global.stage.key_focus?.accessible_name === name)
                finish(true);
        };
        timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, timeoutMs, () => {
            timer = 0;
            finish(false);
            return GLib.SOURCE_REMOVE;
        });
        check();
    });
}

/** State of the macOS logo menu in the top bar. */
export function logoMenuState() {
    const btn = Main.panel.statusArea['desk-logo-menu'] ?? null;
    const activities = Main.panel.statusArea.activities ?? null;
    const items = [];
    if (btn?.menu?._getMenuItems) {
        for (const item of btn.menu._getMenuItems()) {
            const text = item?.label?.text;
            if (typeof text === 'string' && text.length > 0)
                items.push(text);
        }
    }
    return {
        present: btn !== null,
        visible: Boolean(btn?.visible),
        activitiesVisible: Boolean(activities?.visible),
        items,
    };
}

/** Resolve once logoMenuState()[field] equals value (event-driven on Desk and LogoMenu settings). */
export function waitLogoMenu(field, value, timeoutMs = 10000) {
    const b = bridge();
    const objs = [b?._settings, b?._logoMenuSettings].filter(o => o);
    return new Promise((resolve, reject) => {
        const ids = [];
        let timer = 0;
        const finish = ok => {
            ids.forEach(([o, id]) => o.disconnect(id));
            if (timer)
                GLib.source_remove(timer);
            if (ok)
                settle().then(resolve, reject);
            else
                reject(new Error(`logoMenu ${field} is ${JSON.stringify(logoMenuState()[field])}, not ${JSON.stringify(value)}`));
        };
        const check = () => {
            if (logoMenuState()[field] === value)
                finish(true);
        };
        for (const o of objs) {
            ids.push([o, o.connect('changed', () => {
                GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
                    check();
                    return GLib.SOURCE_REMOVE;
                });
            })]);
        }
        timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, timeoutMs, () => {
            timer = 0;
            finish(false);
            return GLib.SOURCE_REMOVE;
        });
        check();
    });
}

/**
 * Resolve once bridgeState()[field] equals value. Checked again after every change of the
 * bridge's settings (one idle later, so the bridge's own handler of the same change ran first).
 */
export function waitBridge(field, value, timeoutMs = 10000) {
    const b = bridge();
    const objs = [b?._settings, b?._clipboard?.settings].filter(o => o);
    return new Promise((resolve, reject) => {
        const ids = [];
        let timer = 0;
        const finish = ok => {
            ids.forEach(([o, id]) => o.disconnect(id));
            if (timer)
                GLib.source_remove(timer);
            if (ok)
                settle().then(resolve, reject); // a rebuilt UI re-adds its keybinding
            else
                reject(new Error(`bridge ${field} is ${JSON.stringify(bridgeState()[field])}, not ${JSON.stringify(value)}`));
        };
        const check = () => {
            if (bridgeState()[field] === value)
                finish(true);
        };
        for (const o of objs) {
            ids.push([o, o.connect('changed', () => {
                GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
                    check();
                    return GLib.SOURCE_REMOVE;
                });
            })]);
        }
        timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, timeoutMs, () => {
            timer = 0;
            finish(false);
            return GLib.SOURCE_REMOVE;
        });
        check();
    });
}

/** Resolve once the clipboard panel's visibility is `open` (event-driven: notify::visible). */
export function waitPanel(open, timeoutMs = 10000) {
    const actor = bridge()?._clipboard?._panel?.actor;
    if (!actor)
        return Promise.reject(new Error('no clipboard panel'));
    if (actor.visible === open)
        return Promise.resolve(true);
    return new Promise((resolve, reject) => {
        let id = 0;
        const timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, timeoutMs, () => {
            actor.disconnect(id);
            reject(new Error(`panel did not become ${open ? 'visible' : 'hidden'} ` +
                `(modal ${Main.modalCount}, overview ${Main.overview.visible}, ` +
                `focus ${global.stage.key_focus}, action mode ${Main.actionMode})`));
            return GLib.SOURCE_REMOVE;
        });
        id = actor.connect('notify::visible', () => {
            if (actor.visible !== open)
                return;
            actor.disconnect(id);
            GLib.source_remove(timer);
            resolve(true);
        });
    });
}

/** Refresh the open panel from deskd and return the texts of its rows (awaits the List call). */
export async function panelItems() {
    const panel = bridge()?._clipboard?._panel;
    if (!panel)
        throw new Error('no clipboard panel');
    await panel.refresh();
    return panel.items.map(i => i.text);
}

// ---- deskd through the bridge: deskd serves history only to the shell (ADR 0008), so the tests
// read it the way the bridge does
function clipClient() {
    const c = bridge()?._clipboard?.client;
    if (!c)
        throw new Error('the bridge has no clipboard client');
    return c;
}

/** Texts of the history, newest first (deskd's List called by the bridge). */
export async function clipTexts() {
    return (await clipClient().list('', 0)).map(i => i.text);
}

/** Push a text through the bridge's client: {ok, id} or {ok: false, error}. */
export async function clipPush(text) {
    try {
        const id = await clipClient().push('text/plain;charset=utf-8', new TextEncoder().encode(text), [], '');
        return {ok: true, id};
    } catch (e) {
        return {ok: false, error: String(e?.message ?? e)};
    }
}

/** Whether the bridge verified the current owner of io.github.OlehDzhumyk.Desk. */
export function clipTrusted() {
    return clipClient().trusted;
}

// ---- leak tracker. While tracking, every GObject created and every signal handler / main-loop
// source added with `fragment` (a path part such as the extension uuid) in the JS stack is
// recorded. leaks() then reports what is still alive:
//   handlers  connected to objects the code did NOT create (shell objects, settings, the display):
//             these outlive the extension; handlers on its own objects go with those objects
//   sources   timeouts/idles the code added itself and that are still pending
//   actors    actors it created that were never destroyed and are still on the stage
//   orphans   actors it created that were never destroyed, are off the stage and carry destroy
//             handlers: they die in a garbage collection, which is exactly when "call back into
//             JSAPI during the sweeping phase of GC" appears
const _orig = {};
let _fragment = null;
let _records = [];
let _sources = new Map();
let _created = new Set();
const _destroyed = new Set();
const _watched = new Map();

function fromFragment() {
    const stack = new Error().stack ?? '';
    const line = stack.split('\n').find(l => l.includes(_fragment));
    return line ? line.replace(/^.*?@/, '').replace(/^.*\/(?=[^/]*$)/, '') : null;
}

/** Where the caller of the patched function is, when that caller is code under the fragment. */
function directCaller() {
    // [0] directCaller, [1] the patched function, [2] its caller
    const line = (new Error().stack ?? '').split('\n')[2] ?? '';
    return line.includes(_fragment) ? line.replace(/^.*?@/, '').replace(/^.*\/(?=[^/]*$)/, '') : null;
}

function typeName(obj) {
    try {
        return GObject.type_name(obj.constructor.$gtype);
    } catch {
        return String(obj?.constructor?.name);
    }
}

function watchActor(obj) {
    if (!(obj instanceof Clutter.Actor) || _watched.has(obj) || _destroyed.has(obj))
        return;
    // our own destroy handler, released again in stopTracking()
    _watched.set(obj, _orig.connect.call(obj, 'destroy', () => {
        _destroyed.add(obj);
        _watched.delete(obj);
    }));
}

/** Start recording; call before the code under test runs (e.g. before enabling the extension). */
export function startTracking(fragment = UUID) {
    if (_fragment)
        return false;
    _fragment = fragment;
    _records = [];
    _sources = new Map();
    _created = new Map();
    const proto = GObject.Object.prototype;
    for (const name of ['connect', 'connect_after', 'disconnect', '_init'])
        _orig[name] = proto[name];
    for (const name of ['timeout_add', 'timeout_add_seconds', 'idle_add', 'source_remove'])
        _orig[name] = GLib[name];
    proto._init = function (...args) {
        const r = _orig._init.apply(this, args);
        const where = fromFragment();
        if (where && !_created.has(this)) {
            _created.set(this, where);
            watchActor(this);
        }
        return r;
    };
    const record = name => function (signal, ...rest) {
        const id = _orig[name].call(this, signal, ...rest);
        // count handlers added by the extension itself (directly or via GObject connectObject /
        // signalTracker.js), not internal shell handlers such as Panel.addToStatusArea
        const frames = (new Error().stack ?? '').split('\n').slice(1);
        const caller = frames.find(l => !l.includes('signalTracker.js')) ?? '';
        const where = caller.includes(_fragment) ? fromFragment() : null;
        if (where)
            _records.push({obj: this, id, signal, where, type: typeName(this)});
        return id;
    };
    proto.connect = record('connect');
    proto.connect_after = record('connect_after');
    // sources count only when the extension's code adds them itself: the shell's own one-shot
    // timeouts (e.g. from Main.popModal called by the extension) are not the extension's to remove
    const source = name => function (...args) {
        const id = _orig[name](...args);
        const where = directCaller();
        if (where)
            _sources.set(id, {kind: name, where});
        return id;
    };
    GLib.timeout_add = source('timeout_add');
    GLib.timeout_add_seconds = source('timeout_add_seconds');
    GLib.idle_add = source('idle_add');
    GLib.source_remove = id => {
        _sources.delete(id);
        return _orig.source_remove(id);
    };
    return true;
}

/** What code under the fragment left behind (see the comment above). */
export function leaks() {
    const handlers = [];
    for (const r of _records) {
        if (_created.has(r.obj) || _destroyed.has(r.obj))
            continue;
        if (GObject.signal_handler_is_connected(r.obj, r.id))
            handlers.push(`${r.type}::${r.signal} (${r.where})`);
    }
    const sources = [];
    const ctx = GLib.MainContext.default();
    for (const [id, s] of _sources) {
        const src = ctx.find_source_by_id(id);
        if (src && !src.is_destroyed())
            sources.push(`${s.kind} ${id} (${s.where})`);
    }
    const actors = [];
    const orphans = [];
    for (const [obj, where] of _created) {
        if (!(obj instanceof Clutter.Actor) || _destroyed.has(obj))
            continue;
        const text = `${typeName(obj)} (${where})`;
        if (obj.get_stage()) {
            actors.push(text);
        } else if (!obj.get_parent()) {
            // never destroyed and off the stage: a hazard only with a destroy handler on it
            // (ours does not count, so it goes first)
            const id = _watched.get(obj);
            if (id) {
                _orig.disconnect.call(obj, id);
                _watched.delete(obj);
            }
            if (GObject.signal_has_handler_pending(obj, GObject.signal_lookup('destroy', Clutter.Actor), 0, true))
                orphans.push(text);
        }
    }
    const keybinding = (Main.wm._allowedKeybindings?.['toggle-shortcut'] ?? 0) !== 0;
    return {handlers, sources, actors, orphans, keybinding, recorded: _records.length, created: _created.size};
}

/** Restore the patched functions and drop every reference (also our destroy watchers). */
export function stopTracking() {
    if (!_fragment)
        return false;
    const proto = GObject.Object.prototype;
    for (const name of ['connect', 'connect_after', 'disconnect', '_init'])
        proto[name] = _orig[name];
    for (const name of ['timeout_add', 'timeout_add_seconds', 'idle_add', 'source_remove'])
        GLib[name] = _orig[name];
    for (const [obj, id] of _watched)
        _orig.disconnect.call(obj, id);
    _watched.clear();
    _destroyed.clear();
    _created.clear();
    _records = [];
    _sources.clear();
    _fragment = null;
    return true;
}

/** Run a full GC now (System.gc), so finalisation problems show up in the log during the test. */
export async function gc() {
    const System = (await import('system')).default;
    System.gc();
    return true;
}
