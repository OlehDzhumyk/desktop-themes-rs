#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Drive the headless shell with org.gnome.Mutter.RemoteDesktop and take screenshots through the same
ScreenCast session (Mutter ScreenCast -> PipeWire -> gst).

Usage (through run.sh): input.py ACTION [ACTION ...]
  key:Super+Shift+v   press a key combination (modifiers: Super, Shift, Ctrl, Alt; keys by X keysym
                      name: v, Return, Escape, Down, Delete, ...)
  type:text           type text (ASCII)
  click:X,Y           left click at X,Y
  move:X,Y            move the pointer
  sleep:SECONDS       wait (keeps the main loop running)
  shot:FILE.png       screenshot
HS_NO_SHOTS=1 skips the ScreenCast stream (no PipeWire needed): keys work, shot: is a no-op.
"""
import os
import subprocess
import sys
import time

import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

KEYSYMS = {
    "super": 0xffeb, "shift": 0xffe1, "ctrl": 0xffe3, "control": 0xffe3, "alt": 0xffe9,
    "return": 0xff0d, "enter": 0xff0d, "kp_enter": 0xff8d, "escape": 0xff1b, "esc": 0xff1b, "tab": 0xff09,
    "backspace": 0xff08, "delete": 0xffff, "up": 0xff52, "down": 0xff54, "left": 0xff51,
    "right": 0xff53, "home": 0xff50, "end": 0xff57, "insert": 0xff63, "space": 0x20,
}

bus = Gio.bus_get_sync(Gio.BusType.SESSION)
RD, SC = "org.gnome.Mutter.RemoteDesktop", "org.gnome.Mutter.ScreenCast"


def call(name, path, iface, method, args, rtype):
    return bus.call_sync(name, path, iface, method, args, GLib.VariantType(rtype) if rtype else None,
                         Gio.DBusCallFlags.NONE, 10000, None)


def pump(sec):
    end = time.time() + sec
    ctx = GLib.MainContext.default()
    while time.time() < end:
        ctx.iteration(False)
        time.sleep(0.01)


(rd_session,) = call(RD, "/org/gnome/Mutter/RemoteDesktop", RD, "CreateSession", None, "(o)").unpack()
sid = bus.call_sync(RD, rd_session, "org.freedesktop.DBus.Properties", "Get",
                    GLib.Variant("(ss)", (RD + ".Session", "SessionId")), GLib.VariantType("(v)"),
                    Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
(sc_session,) = call(SC, "/org/gnome/Mutter/ScreenCast", SC, "CreateSession",
                     GLib.Variant("(a{sv})", ({"remote-desktop-session-id": GLib.Variant("s", sid)},)), "(o)").unpack()
NO_SHOTS = os.environ.get("HS_NO_SHOTS") == "1"
loop = GLib.MainLoop()
node = {}
stream = None
if NO_SHOTS:
    call(RD, rd_session, RD + ".Session", "Start", None, None)
else:
    (stream,) = call(SC, sc_session, SC + ".Session", "RecordMonitor",
                     GLib.Variant("(sa{sv})", ("", {"cursor-mode": GLib.Variant("u", 1)})), "(o)").unpack()
    bus.signal_subscribe(SC, SC + ".Stream", "PipeWireStreamAdded", stream, None, Gio.DBusSignalFlags.NONE,
                         lambda *a: (node.__setitem__("id", a[5].unpack()[0]), loop.quit()))
    call(RD, rd_session, RD + ".Session", "Start", None, None)
    GLib.timeout_add_seconds(10, loop.quit)
    loop.run()
    if "id" not in node:
        sys.exit("PipeWireStreamAdded did not arrive")
pump(0.5)


def keysym(name):
    k = KEYSYMS.get(name.lower())
    if k is not None:
        return k
    if len(name) == 1:
        return ord(name)
    raise SystemExit("unknown key " + name)


def press(sym, down):
    call(RD, rd_session, RD + ".Session", "NotifyKeyboardKeysym", GLib.Variant("(ub)", (sym, down)), None)


def combo(spec):
    syms = [keysym(p) for p in spec.split("+")]
    for s in syms:
        press(s, True)
        pump(0.03)
    for s in reversed(syms):
        press(s, False)
        pump(0.03)


def pointer(x, y):
    call(RD, rd_session, RD + ".Session", "NotifyPointerMotionAbsolute", GLib.Variant("(sdd)", (stream, x, y)), None)


def shot(out):
    if NO_SHOTS:
        print("screenshot skipped (HS_NO_SHOTS=1): " + out)
        return
    r = subprocess.run(["gst-launch-1.0", "-q", "pipewiresrc", "path=%d" % node["id"], "num-buffers=5", "!",
                        "videoconvert", "!", "pngenc", "snapshot=true", "!", "filesink", "location=" + out],
                       timeout=30)
    print("screenshot: %s (gst=%d)" % (out, r.returncode))


for action in sys.argv[1:]:
    kind, _, arg = action.partition(":")
    if kind == "key":
        combo(arg)
        pump(0.2)
    elif kind == "type":
        for ch in arg:
            if ch.isupper():
                combo("Shift+" + ch.lower())
            else:
                combo("space" if ch == " " else ch)
    elif kind in ("click", "move"):
        x, y = (float(v) for v in arg.split(","))
        pointer(x, y)
        pump(0.2)
        if kind == "click":
            for down in (True, False):
                call(RD, rd_session, RD + ".Session", "NotifyPointerButton", GLib.Variant("(ib)", (272, down)), None)
                pump(0.06)
        pump(0.3)
    elif kind == "sleep":
        pump(float(arg))
    elif kind == "shot":
        shot(arg)
    else:
        raise SystemExit("unknown action " + action)
call(RD, rd_session, RD + ".Session", "Stop", None, None)
