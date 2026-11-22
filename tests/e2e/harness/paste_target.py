#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Paste target of the clipboard test: a GTK 4 window with one focused text view. Prints READY
once the view has the focus, then every change of the text as one JSON line on stdout, so the
test waits on output instead of polling a file."""
import json
import sys

import gi

gi.require_version('Gtk', '4.0')
from gi.repository import Gtk  # noqa: E402


def emit(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + '\n')
    sys.stdout.flush()


def on_activate(app):
    win = Gtk.ApplicationWindow(application=app, title='Paste target', default_width=900, default_height=500)
    view = Gtk.TextView()
    buf = view.get_buffer()
    buf.connect('changed', lambda b: emit({'text': b.get_text(b.get_start_iter(), b.get_end_iter(), False)}))
    win.set_child(view)

    def focused(*_):
        if win.is_active() and view.has_focus():
            emit({'ready': True})
    win.connect('notify::is-active', focused)
    view.connect('notify::has-focus', focused)
    win.present()
    view.grab_focus()


app = Gtk.Application(application_id='org.example.PasteTarget')
app.connect('activate', on_activate)
app.run([sys.argv[0]])
