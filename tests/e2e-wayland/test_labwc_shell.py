# SPDX-License-Identifier: GPL-3.0-or-later
"""desk-shell --skin windows11 and deskd on headless labwc (ADR 0012), through the real binaries.

One session per module (conftest.Session): labwc with the bundled rc.xml, then deskd, then
desk-shell, as data/labwc/windows11/autostart starts them. The flyouts are driven the way the
rc.xml key bindings drive them: a second `desk-shell --toggle-...` process. The clipboard tests
copy with wl-copy (deskd's data-control watcher records it); the window test maps a GTK 4 window
(desk-shell's foreign-toplevel client adds its taskbar button).
"""

import json
import pathlib
import stat
import subprocess
import uuid

import pytest
from gi.repository import Gio, GLib

from conftest import (CLIPBOARD_IFACE, CLIPBOARD_PATH, DESK, DESK_SHELL, LAYER_LIBRARY, TIMEOUT,
                      destroy_layer, new_layer, problems)

# flag -> layer-shell namespace of the flyout it toggles (start_menu.rs, quick_settings.rs,
# clipboard_flyout.rs)
FLYOUTS = {
    '--toggle-start': 'desk-shell-start',
    '--toggle-quick-settings': 'desk-shell-quick-settings',
    '--toggle-clipboard': 'desk-shell-clipboard',
}
TASKBAR_SURFACE = (r'-> zwlr_layer_shell_v1#\d+\.get_layer_surface'
                   r'\(new id zwlr_layer_surface_v1#\d+, wl_surface#(\d+),.*"desk-shell-taskbar"\)')


def layer_surface(log, namespace, since=0):
    """The object id of the next zwlr_layer_surface_v1 that desk-shell creates for `namespace`."""
    return log.wait(new_layer(namespace), since).group(1)


def test_desk_shell_loads_gtk4_layer_shell(session):
    maps = pathlib.Path(f'/proc/{session.procs["desk-shell"].pid}/maps').read_text()
    assert LAYER_LIBRARY in maps
    assert not [line for line in session.logs['desk-shell'].snapshot()
                if 'using normal windows' in line]


def test_taskbar_is_a_layer_surface_on_labwc(session):
    log = session.logs['desk-shell']
    taskbar = layer_surface(log, 'desk-shell-taskbar')
    surface = log.wait(TASKBAR_SURFACE).group(1)
    # auto exclusive zone: labwc keeps windows off the bar once GTK knows its height (48 px)
    log.wait(rf'-> zwlr_layer_surface_v1#{taskbar}\.set_exclusive_zone\(48\)')
    # full monitor width: without resizable(false), GTK honours labwc's configure(output_width, 48)
    cfg = log.wait(rf'zwlr_layer_surface_v1#{taskbar}\.configure\(\d+,\s*(\d+),\s*48\)')
    width = int(cfg.group(1))
    assert width >= 1280, f'expected full-width taskbar configure (>= 1280), got {width}x48'
    log.wait(
        rf'-> (?:wl_surface#{surface}\.damage_buffer\(0,\s*0,\s*{width},\s*48\)'
        rf'|wl_shm_pool#\d+\.create_buffer\(.*,\s*{width},\s*48,\s*\d+,\s*\d+\))'
    )


def test_labwc_accepts_the_bundled_config(session):
    # labwc 0.20 logs [ERROR] for an unknown rc.xml or themerc option (#65 removed the old ones)
    errors = [line for line in session.logs['labwc'].snapshot() if '[ERROR]' in line]
    assert not errors, '\n'.join(errors)


def test_deskd_and_desk_shell_own_their_names(session):
    assert session.name_owner(DESK)
    assert session.name_owner(DESK_SHELL)


@pytest.mark.parametrize('flag', list(FLYOUTS))
def test_toggle_reaches_the_running_shell(session, flag):
    log = session.logs['desk-shell']
    owner = session.name_owner(DESK_SHELL)
    for step in ('open', 'close'):
        since = log.mark()
        r = session.run_shell(flag)
        err = r.stderr.decode(errors='replace')
        assert r.returncode == 0, f'{flag} ({step}) exited {r.returncode}: {err}'
        assert 'already running' not in err
        assert not problems(err.splitlines()), err
        if step == 'open':
            surface = layer_surface(log, FLYOUTS[flag], since)
        else:
            log.wait(destroy_layer(surface), since)
    # the primary instance answered both times: still running, same bus name owner
    assert session.alive('desk-shell')
    assert session.name_owner(DESK_SHELL) == owner


# ---- clipboard capture on labwc (deskd's ext/wlr data-control watcher, #63)

def copy(session, name, *args):
    """wl-copy in the foreground: it serves the selection until the next copy replaces it."""
    return session.spawn_client(name, ['wl-copy', '--foreground', *args])


def copy_and_wait_changed(session, name, *args):
    """Copy, and wait for deskd's Clipboard.Changed (subscribed before the copy)."""
    session.wait_signal(CLIPBOARD_IFACE, 'Changed', CLIPBOARD_PATH,
                        lambda: copy(session, name, *args), f'Clipboard.Changed after {name}')


def history_texts(text):
    return [item.get('text', '') for item in json.loads(text).get('items', [])]


def test_wl_copy_is_recorded_and_saved_privately(session):
    # spaces: never taken for a probable secret (clipboard::looks_secret), so it is saved
    payload = f'e2e clipboard payload {uuid.uuid4().hex}'
    copy_and_wait_changed(session, 'wl-copy-payload', payload)
    path = session.clipboard_history
    text = session.wait_file(path, lambda t: payload in t, 'history.json with the payload')
    assert payload in history_texts(text)
    assert stat.S_IMODE(path.stat().st_mode) == 0o600


def test_password_manager_copy_is_never_recorded(session):
    secret = f'e2e-secret-marker-{uuid.uuid4().hex}'
    sentinel = f'e2e clipboard sentinel {uuid.uuid4().hex}'
    # the offer carries only the hint type, as wl-copy offers one type per copy; deskd must skip
    # it for the hint (clipboard::SECRET_HINTS) before it looks at any type to read
    copy(session, 'wl-copy-secret', '--type', 'x-kde-passwordManagerHint', secret)
    # the sentinel replaces the secret's selection: that wl-copy ends, and deskd has seen both
    copy_and_wait_changed(session, 'wl-copy-sentinel', sentinel)
    assert session.wait_exit('wl-copy-secret') == 0
    text = session.wait_file(session.clipboard_history, lambda t: sentinel in t,
                             'history.json with the sentinel')
    assert sentinel in history_texts(text)
    assert secret not in text
    found = problems(session.logs['wl-copy-secret'].snapshot())
    assert not found, '\n'.join(found)


def test_clipboard_list_is_denied_to_untrusted_clients(session):
    # the test process (python3) is not one of the trusted binaries (ADR 0008)
    with pytest.raises(GLib.Error) as err:
        session.bus.call_sync(DESK, CLIPBOARD_PATH, CLIPBOARD_IFACE, 'List',
                              GLib.Variant('(su)', ('', 10)), None,
                              Gio.DBusCallFlags.NONE, TIMEOUT * 1000, None)
    assert 'org.freedesktop.DBus.Error.AccessDenied' in Gio.dbus_error_get_remote_error(err.value)


# ---- running windows on the taskbar (zwlr_foreign_toplevel_management_v1, #65)

CLIENT_APP_ID = 'io.github.OlehDzhumyk.E2eWindow'
CLIENT_TITLE = 'desk e2e window'
# A GTK 4 window: prints "mapped" after its first frame is painted (the toplevel has a buffer, so
# labwc maps it), and closes itself when a line "close" (or end of input) comes on stdin.
CLIENT = r'''
import sys
import gi
gi.require_version('Gtk', '4.0')
from gi.repository import GLib, Gtk

app = Gtk.Application(application_id=sys.argv[1])


def activate(app):
    win = Gtk.ApplicationWindow(application=app, title=sys.argv[2])
    win.set_default_size(320, 200)

    def realized(w):
        clock = w.get_frame_clock()
        handler = []

        def painted(_clock):
            clock.disconnect(handler[0])
            print('mapped', flush=True)
        handler.append(clock.connect('after-paint', painted))
    win.connect('realize', realized)

    def command(_channel, _cond):
        line = sys.stdin.readline()
        if line.strip() == 'close' or not line:
            win.close()
            return GLib.SOURCE_REMOVE
        return GLib.SOURCE_CONTINUE
    GLib.io_add_watch(GLib.IOChannel.unix_new(sys.stdin.fileno()), GLib.PRIORITY_DEFAULT,
                      GLib.IOCondition.IN | GLib.IOCondition.HUP, command)
    win.present()


app.connect('activate', activate)
sys.exit(app.run(None))
'''


def test_running_window_updates_the_taskbar(session):
    log = session.logs['desk-shell']
    taskbar = log.wait(TASKBAR_SURFACE).group(1)
    since = log.mark()
    client = session.spawn_client('gtk-window', ['python3', '-c', CLIENT, CLIENT_APP_ID,
                                                 CLIENT_TITLE], stdin=subprocess.PIPE)
    session.logs['gtk-window'].wait(r'^mapped$')
    # desk-shell got the new toplevel and redrew its taskbar with the running button
    log.wait(rf'-> wl_surface#{taskbar}\.commit\(\)', since)
    since = log.mark()
    client.stdin.write(b'close\n')
    client.stdin.close()
    assert session.wait_exit('gtk-window') == 0
    # the toplevel closed: desk-shell drops the button and redraws again
    log.wait(rf'-> wl_surface#{taskbar}\.commit\(\)', since)
    for name in ('labwc', 'deskd', 'desk-shell'):
        assert session.alive(name), f'{name} exited (see {name}.log)'
    for name in ('deskd', 'desk-shell', 'gtk-window'):
        found = problems(session.logs[name].snapshot())
        assert not found, f'{name}: ' + '\n'.join(found)


def test_shell_and_deskd_stay_alive_without_errors(session):
    for name in ('labwc', 'deskd', 'desk-shell'):
        assert session.alive(name), f'{name} exited (see {name}.log)'
    assert session.name_owner(DESK)
    assert session.name_owner(DESK_SHELL)
    for name in ('deskd', 'desk-shell'):
        found = problems(session.logs[name].snapshot())
        assert not found, f'{name}: ' + '\n'.join(found)
    errors = [line for line in session.logs['labwc'].snapshot() if '[ERROR]' in line]
    assert not errors, '\n'.join(errors)
    # every layer surface the toggles opened was closed again; the taskbar stays
    log = session.logs['desk-shell'].snapshot()
    created, destroyed, opened = new_layer(), destroy_layer(), {}
    for line in log:
        if m := created.search(line):
            opened[m.group(1)] = m.group(2)
        elif m := destroyed.search(line):
            opened.pop(m.group(1), None)
    assert sorted(opened.values()) == ['desk-shell-taskbar']
