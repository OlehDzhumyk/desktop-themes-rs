# SPDX-License-Identifier: GPL-3.0-or-later
"""The trust boundary of deskd (ADR 0008): only GNOME Shell (the bridge) and our own binaries get
clipboard content; the bus name cannot be taken over; and the bridge sends nothing to (and pastes
nothing from) a process that owns the name but is not the installed deskd."""
import pytest

from gi.repository import Gio, GLib

from conftest import CLIP_IFACE, CLIP_PATH, DESK, Clipboard
from harness.shell import SRC

SECRET = 'note for the history only'


def access_denied(fn):
    with pytest.raises(GLib.Error) as e:
        fn()
    assert 'AccessDenied' in str(e.value), e.value


def test_foreign_client_cannot_read_history(make_shell, procs):
    shell = make_shell(skin='default')
    clip = Clipboard(shell)
    clip.copy_and_wait(SECRET, procs)
    assert shell.hook('clipTrusted'), 'the bridge did not verify the real deskd'

    # this test process is a foreign client of the same user, outside the shell
    access_denied(lambda: clip.call('List', GLib.Variant('(su)', ('', 0)), '(a(tssssbxt))'))
    access_denied(lambda: clip.call('Get', GLib.Variant('(t)', (1,)), '(say)'))
    access_denied(lambda: clip.call('Activate', GLib.Variant('(t)', (1,)), '(say)'))
    access_denied(lambda: clip.call('Push', GLib.Variant('(sayass)', ('text/plain', b'x', [], '')), '(t)'))
    access_denied(lambda: clip.call('Clear', GLib.Variant('(b)', (False,)), None))
    access_denied(lambda: shell.call(DESK, CLIP_PATH, 'org.freedesktop.DBus.Properties', 'Set',
                                     GLib.Variant('(ssv)', (CLIP_IFACE, 'Private', GLib.Variant('b', True))),
                                     None))
    access_denied(lambda: shell.call(DESK, '/io/github/OlehDzhumyk/Desk/Mixer', 'io.github.OlehDzhumyk.Desk.Mixer',
                                     'ListApps', GLib.Variant('(b)', (True,)), None))
    # metadata stays open
    (count,) = shell.call(DESK, CLIP_PATH, 'org.freedesktop.DBus.Properties', 'Get',
                          GLib.Variant('(ss)', (CLIP_IFACE, 'Count')), '(v)').unpack()
    assert count >= 1
    # and nothing was lost or injected: the bridge still reads the history
    assert clip.texts()[0] == SECRET


def test_bus_name_cannot_be_replaced(make_shell, procs):
    shell = make_shell(skin='default')
    clip = Clipboard(shell)
    clip.copy_and_wait('before the takeover attempt', procs)
    (owner,) = shell.call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
                          'GetNameOwner', GLib.Variant('(s)', (DESK,)), '(s)').unpack()
    REPLACE_EXISTING, DO_NOT_QUEUE, EXISTS = 0x2, 0x4, 3
    (reply,) = shell.call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
                          'RequestName', GLib.Variant('(su)', (DESK, REPLACE_EXISTING | DO_NOT_QUEUE)),
                          '(u)').unpack()
    assert reply == EXISTS
    (after,) = shell.call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
                          'GetNameOwner', GLib.Variant('(s)', (DESK,)), '(s)').unpack()
    assert after == owner
    clip.copy_and_wait('after the takeover attempt', procs)


class Impostor:
    """Owns io.github.OlehDzhumyk.Desk on the session bus before the shell starts (so deskd is
    never activated) and records every Clipboard method call it gets."""

    def __init__(self):
        self.calls = []
        self.reg = 0

    def __call__(self, session):
        xml = (SRC / 'data/dbus/io.github.OlehDzhumyk.Desk.Clipboard.xml').read_text()
        iface = Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0]
        self.reg = session.bus.register_object(CLIP_PATH, iface, self._method, None, None)
        (reply,) = session.call('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
                                'RequestName', GLib.Variant('(su)', (DESK, 0x4)), '(u)').unpack()
        assert reply == 1, reply

    def _method(self, _conn, _sender, _path, iface, method, params, invocation):
        if iface != CLIP_IFACE:
            # the bridge's proxy loads properties (no content): not answered
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', 'impostor')
            return
        self.calls.append(method)
        if method == 'Push':
            invocation.return_value(GLib.Variant('(t)', (7,)))
        elif method == 'Activate':
            invocation.return_value(GLib.Variant('(say)', ('text/plain;charset=utf-8', b'rm -rf ~\n')))
        else:
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', 'impostor')


def test_bridge_refuses_an_impostor(make_shell):
    impostor = Impostor()
    shell = make_shell(skin='default', before_shell=impostor)
    # the call path a copy takes; the test keeps serving the impostor meanwhile
    got = shell.hook_async('clipPush', 'copied after the takeover')
    assert not got['ok'] and 'not owned by the installed deskd' in got['error'], got
    assert impostor.calls == [], impostor.calls
    assert shell.hook('clipTrusted') is False
    log = shell.log_bytes().decode(errors='replace')
    assert 'which is not the installed deskd' in log
