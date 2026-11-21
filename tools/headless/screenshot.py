#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Знімок екрана headless-шела через org.gnome.Mutter.ScreenCast + gst pipewiresrc.

Використання (через run.sh): screenshot.py OUT.png
(org.gnome.Shell.Screenshot для сторонніх клієнтів заборонено, тому — ScreenCast.)
"""
import subprocess
import sys

import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

out = sys.argv[1]
bus = Gio.bus_get_sync(Gio.BusType.SESSION)


def call(path, iface, method, args, rtype):
    return bus.call_sync("org.gnome.Mutter.ScreenCast", path, iface, method, args,
                         GLib.VariantType(rtype), Gio.DBusCallFlags.NONE, 10000, None).unpack()


(session,) = call("/org/gnome/Mutter/ScreenCast", "org.gnome.Mutter.ScreenCast", "CreateSession",
                  GLib.Variant("(a{sv})", ({},)), "(o)")
(stream,) = call(session, "org.gnome.Mutter.ScreenCast.Session", "RecordMonitor",
                 GLib.Variant("(sa{sv})", ("", {"cursor-mode": GLib.Variant("u", 0)})), "(o)")
loop = GLib.MainLoop()
node = {}


def on_added(_c, _s, _p, _i, _sig, params):
    node["id"] = params.unpack()[0]
    loop.quit()


bus.signal_subscribe("org.gnome.Mutter.ScreenCast", "org.gnome.Mutter.ScreenCast.Stream", "PipeWireStreamAdded",
                     stream, None, Gio.DBusSignalFlags.NONE, on_added)
call(session, "org.gnome.Mutter.ScreenCast.Session", "Start", None, "()")
GLib.timeout_add_seconds(10, loop.quit)
loop.run()
if "id" not in node:
    sys.exit("PipeWireStreamAdded не надійшов")
r = subprocess.run(["gst-launch-1.0", "-q", "pipewiresrc", "path=%d" % node["id"], "num-buffers=5", "!",
                    "videoconvert", "!", "pngenc", "snapshot=true", "!", "filesink", "location=" + out],
                   timeout=30)
try:
    call(session, "org.gnome.Mutter.ScreenCast.Session", "Stop", None, "()")
except GLib.Error:
    pass
print("знімок: %s (pipewire node %d, gst=%d)" % (out, node["id"], r.returncode))
sys.exit(r.returncode)
