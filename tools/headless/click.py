#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Клацання мишею в headless-шелі (org.gnome.Mutter.RemoteDesktop) і знімок екрана.

Використання (через run.sh): click.py OUT.png X,Y [X,Y ...]
Кожна точка — клік лівою кнопкою з паузою 1.5 с; наприкінці знімок через ту саму ScreenCast-сесію.
"""
import subprocess
import sys
import time

import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

out = sys.argv[1]
points = [tuple(float(v) for v in p.split(",")) for p in sys.argv[2:]]
bus = Gio.bus_get_sync(Gio.BusType.SESSION)


def call(name, path, iface, method, args, rtype):
    return bus.call_sync(name, path, iface, method, args, GLib.VariantType(rtype) if rtype else None,
                         Gio.DBusCallFlags.NONE, 10000, None)


RD, SC = "org.gnome.Mutter.RemoteDesktop", "org.gnome.Mutter.ScreenCast"
(rd_session,) = call(RD, "/org/gnome/Mutter/RemoteDesktop", RD, "CreateSession", None, "(o)").unpack()
sid = bus.call_sync(RD, rd_session, "org.freedesktop.DBus.Properties", "Get",
                    GLib.Variant("(ss)", (RD + ".Session", "SessionId")), GLib.VariantType("(v)"),
                    Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
(sc_session,) = call(SC, "/org/gnome/Mutter/ScreenCast", SC, "CreateSession",
                     GLib.Variant("(a{sv})", ({"remote-desktop-session-id": GLib.Variant("s", sid)},)), "(o)").unpack()
(stream,) = call(SC, sc_session, SC + ".Session", "RecordMonitor",
                 GLib.Variant("(sa{sv})", ("", {"cursor-mode": GLib.Variant("u", 1)})), "(o)").unpack()
loop = GLib.MainLoop()
node = {}
bus.signal_subscribe(SC, SC + ".Stream", "PipeWireStreamAdded", stream, None, Gio.DBusSignalFlags.NONE,
                     lambda *a: (node.__setitem__("id", a[5].unpack()[0]), loop.quit()))
call(RD, rd_session, RD + ".Session", "Start", None, None)
GLib.timeout_add_seconds(10, loop.quit)
loop.run()


def pump(sec):
    end = time.time() + sec
    ctx = GLib.MainContext.default()
    while time.time() < end:
        ctx.iteration(False)
        time.sleep(0.02)


pump(0.5)
for x, y in points:
    call(RD, rd_session, RD + ".Session", "NotifyPointerMotionAbsolute", GLib.Variant("(sdd)", (stream, x, y)), None)
    pump(0.3)
    for pressed in (True, False):
        call(RD, rd_session, RD + ".Session", "NotifyPointerButton", GLib.Variant("(ib)", (272, pressed)), None)
        pump(0.08)
    pump(1.5)
r = subprocess.run(["gst-launch-1.0", "-q", "pipewiresrc", "path=%d" % node["id"], "num-buffers=5", "!",
                    "videoconvert", "!", "pngenc", "snapshot=true", "!", "filesink", "location=" + out], timeout=30)
call(RD, rd_session, RD + ".Session", "Stop", None, None)
print("знімок: %s після %d кліків (gst=%d)" % (out, len(points), r.returncode))
