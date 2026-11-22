#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""A tiny GTK 4 window with one text field, used as the paste target in headless tests.
Every change of the text is written to OUT (so a test can check what was pasted).

Usage (through run.sh): paste-target.py OUT.txt
"""
import sys

import gi
gi.require_version("Gtk", "4.0")
from gi.repository import Gtk  # noqa: E402

out = sys.argv[1]


def on_activate(app):
    win = Gtk.ApplicationWindow(application=app, title="Paste target", default_width=900, default_height=500)
    view = Gtk.TextView(wrap_mode=Gtk.WrapMode.WORD_CHAR, top_margin=24, left_margin=24, right_margin=24)
    buf = view.get_buffer()

    def changed(b):
        with open(out, "w", encoding="utf-8") as f:
            f.write(b.get_text(b.get_start_iter(), b.get_end_iter(), False))

    buf.connect("changed", changed)
    changed(buf)
    win.set_child(view)
    win.present()
    view.grab_focus()


app = Gtk.Application(application_id="org.example.PasteTarget")
app.connect("activate", on_activate)
app.run([sys.argv[0]])
