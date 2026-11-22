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
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

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

/** Press and release a combination such as "Shift+Super+v" (all down, then up in reverse). */
export function keys(spec) {
    _keyboard ??= global.stage.context.get_backend().get_default_seat()
        .create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
    const vals = spec.split('+').map(keyval);
    const t = () => GLib.get_monotonic_time();
    for (const v of vals)
        _keyboard.notify_keyval(t(), v, Clutter.KeyState.PRESSED);
    for (const v of [...vals].reverse())
        _keyboard.notify_keyval(t(), v, Clutter.KeyState.RELEASED);
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
    };
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
            reject(new Error(`panel did not become ${open ? 'visible' : 'hidden'}`));
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

// ---- leak tracker: records every GObject signal handler and main-loop source that code under
// `fragment` (a path part such as the extension uuid) creates, and whether it is gone later.
const _orig = {};
let _fragment = null;
let _records = [];
let _sources = new Map();
const _destroyed = new Set();
const _watched = new Map();

function fromFragment() {
    const stack = new Error().stack ?? '';
    const line = stack.split('\n').find(l => l.includes(_fragment));
    return line ? line.replace(/^.*?@/, '').replace(/^.*\/(?=[^/]*$)/, '') : null;
}

function typeName(obj) {
    try {
        return GObject.type_name(obj.constructor.$gtype);
    } catch {
        return String(obj?.constructor?.name);
    }
}

function watchActor(obj) {
    if (!(obj instanceof Clutter.Actor) || _watched.has(obj))
        return;
    // our own destroy handler, released again in stopTracking() (a destroy handler left on an
    // actor that is garbage-collected later is exactly the warning the tests look for)
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
    const proto = GObject.Object.prototype;
    for (const name of ['connect', 'connect_after', 'disconnect'])
        _orig[name] = proto[name];
    for (const name of ['timeout_add', 'timeout_add_seconds', 'idle_add', 'source_remove'])
        _orig[name] = GLib[name];
    const record = name => function (signal, ...rest) {
        const id = _orig[name].call(this, signal, ...rest);
        const where = fromFragment();
        if (where) {
            watchActor(this);
            _records.push({obj: this, id, signal, where, type: typeName(this)});
        }
        return id;
    };
    proto.connect = record('connect');
    proto.connect_after = record('connect_after');
    const source = name => (...args) => {
        const id = _orig[name](...args);
        const where = fromFragment();
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

/** What code under the fragment left behind: connected handlers, pending sources, actors. */
export function leaks() {
    const handlers = [];
    for (const r of _records) {
        if (_destroyed.has(r.obj))
            continue; // destroying an actor drops its handlers
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
    const walk = a => {
        const sc = a.style_class ?? '';
        const t = typeName(a);
        if (/\bdesk-/.test(sc) || /desk-bridge/.test(t))
            actors.push(`${t} .${sc}`);
        for (const c of a.get_children())
            walk(c);
    };
    walk(global.stage);
    const keybinding = Main.wm._allowedKeybindings?.['toggle-shortcut'] !== undefined;
    return {handlers, sources, actors, keybinding, recorded: _records.length};
}

/** Restore the patched functions and drop every reference (also our destroy watchers). */
export function stopTracking() {
    if (!_fragment)
        return false;
    const proto = GObject.Object.prototype;
    for (const name of ['connect', 'connect_after', 'disconnect'])
        proto[name] = _orig[name];
    for (const name of ['timeout_add', 'timeout_add_seconds', 'idle_add', 'source_remove'])
        GLib[name] = _orig[name];
    for (const [obj, id] of _watched)
        _orig.disconnect.call(obj, id);
    _watched.clear();
    _destroyed.clear();
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
