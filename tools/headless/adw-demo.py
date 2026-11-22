#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""A libadwaita window with the common widgets (header bar, sidebar, boxed lists, suggested and
destructive buttons, linked buttons, entries, switches, a popover menu), used to look at a theme
in the headless shell. Neutral sample content only.

Usage (through run.sh): adw-demo.py [--menu]   (--menu opens the main menu after start)
"""
import sys

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gio, GLib, Gtk  # noqa: E402

OPEN_MENU = "--menu" in sys.argv


def row(title, subtitle=None, suffix=None):
    r = Adw.ActionRow(title=title)
    if subtitle:
        r.set_subtitle(subtitle)
    if suffix:
        r.add_suffix(suffix)
    return r


def content():
    page = Adw.PreferencesPage()
    g = Adw.PreferencesGroup(title="Appearance", description="Sample settings rendered with the theme")
    g.add(Adw.SwitchRow(title="Dark sidebar", subtitle="A switch row", active=True))
    g.add(Adw.SwitchRow(title="Compact mode"))
    combo = Adw.ComboRow(title="Density", model=Gtk.StringList.new(["Comfortable", "Regular", "Compact"]))
    g.add(combo)
    g.add(Adw.EntryRow(title="Display name", text="Sample text"))
    page.add(g)

    g2 = Adw.PreferencesGroup(title="Actions")
    box = Gtk.Box(spacing=12, halign=Gtk.Align.START, margin_top=6, margin_bottom=6)
    box.append(Gtk.Button(label="Save", css_classes=["suggested-action"]))
    box.append(Gtk.Button(label="Delete", css_classes=["destructive-action"]))
    box.append(Gtk.Button(label="Cancel"))
    linked = Gtk.Box(css_classes=["linked"])
    for icon in ["format-justify-left-symbolic", "format-justify-center-symbolic", "format-justify-right-symbolic"]:
        linked.append(Gtk.ToggleButton(icon_name=icon))
    box.append(linked)
    box.append(Gtk.Button(icon_name="list-add-symbolic", css_classes=["circular"]))
    box.append(Gtk.Button(label="Pill", css_classes=["pill"]))
    g2.add(box)
    ebox = Gtk.Box(spacing=12, margin_bottom=6)
    ebox.append(Gtk.Entry(placeholder_text="Search…", hexpand=True))
    ebox.append(Gtk.SpinButton.new_with_range(0, 100, 1))
    ebox.append(Gtk.CheckButton(label="Check", active=True))
    ebox.append(Gtk.Switch(active=True, valign=Gtk.Align.CENTER))
    g2.add(ebox)
    g2.add(row("Linked link", "An accent-coloured link", Gtk.LinkButton(label="Open", uri="https://example.org")))
    page.add(g2)

    g3 = Adw.PreferencesGroup(title="Status")
    lb = Gtk.LevelBar(value=0.6, hexpand=True, valign=Gtk.Align.CENTER)
    g3.add(row("Progress", None, lb))
    for name, cls in [("Success", "success"), ("Warning", "warning"), ("Error", "error")]:
        g3.add(row(name, None, Gtk.Label(label=name.lower(), css_classes=[cls])))
    page.add(g3)
    return page


def on_activate(app):
    win = Adw.ApplicationWindow(application=app, title="Token preview", default_width=1180, default_height=760)
    sidebar_list = Gtk.ListBox(css_classes=["navigation-sidebar"])
    for name, icon in [("Overview", "go-home-symbolic"), ("Appearance", "applications-graphics-symbolic"),
                       ("Sound", "audio-speakers-symbolic"), ("Network", "network-wireless-symbolic"),
                       ("About", "help-about-symbolic")]:
        b = Gtk.Box(spacing=12, margin_top=6, margin_bottom=6, margin_start=6)
        b.append(Gtk.Image(icon_name=icon))
        b.append(Gtk.Label(label=name))
        sidebar_list.append(b)
    sidebar_list.select_row(sidebar_list.get_row_at_index(1))
    sv = Adw.ToolbarView()
    sv.add_top_bar(Adw.HeaderBar(title_widget=Adw.WindowTitle(title="Settings")))
    sv.set_content(Gtk.ScrolledWindow(child=sidebar_list))
    menu = Gio.Menu()
    menu.append("Preferences", "app.none")
    menu.append("Keyboard Shortcuts", "app.none")
    menu.append("About", "app.none")
    mb = Gtk.MenuButton(icon_name="open-menu-symbolic", menu_model=menu)
    hb = Adw.HeaderBar(title_widget=Adw.WindowTitle(title="Appearance", subtitle="Token preview"))
    hb.pack_end(mb)
    cv = Adw.ToolbarView()
    cv.add_top_bar(hb)
    cv.set_content(content())
    split = Adw.NavigationSplitView(sidebar=Adw.NavigationPage(child=sv, title="Settings"),
                                    content=Adw.NavigationPage(child=cv, title="Appearance"))
    win.set_content(split)
    win.present()
    if OPEN_MENU:
        GLib.timeout_add(1200, lambda: (mb.popup(), False)[1])


app = Adw.Application(application_id="io.github.OlehDzhumyk.TokenPreview", flags=Gio.ApplicationFlags.NON_UNIQUE)
app.connect("activate", on_activate)
app.run([])
