# SPDX-License-Identifier: GPL-3.0-or-later
"""Fixtures of the end-to-end harness. Each test gets a fresh headless GNOME Shell session
(harness/shell.py) with the freshly built bridge, deskd and `themes` installed; the session's log
is checked for JS errors of our extensions when the test ends, and a screenshot plus the log are
kept in $E2E_OUT on failure (the CI job uploads them)."""

import contextlib
import json
import os
import pathlib
import subprocess

import pytest

from gi.repository import Gio, GLib

from harness.shell import SRC, UUID, Session, wait_for

OUT = pathlib.Path(os.environ.get('E2E_OUT', '/tmp/e2e-out'))
WORK = pathlib.Path(os.environ.get('E2E_WORK', '/tmp/e2e-work'))

# skin -> the theme whose look module (desk.dconf) sets it, and that theme's panel shortcut
SKINS = {
    'macos': ('macos', 'Shift+Super+v'),
    'windows11': ('windows11', 'Super+v'),
    'default': ('fedora', 'Shift+Super+v'),
}
PANEL_CLASS = {'macos': 'ListPanel', 'windows11': 'WinFlyout', 'default': 'ListPanel'}

DESK = 'io.github.OlehDzhumyk.Desk'
CLIP_PATH = '/io/github/OlehDzhumyk/Desk/Clipboard'
CLIP_IFACE = 'io.github.OlehDzhumyk.Desk.Clipboard'


def skin_dconf(skin):
    """The theme's desk.dconf (skin + panel shortcut). For windows11 also the line of its behavior
    module that moves the shell's notification list off Super+V, which the flyout takes."""
    theme = SKINS[skin][0]
    text = (SRC / 'themes' / theme / 'desk.dconf').read_text()
    if theme == 'windows11':
        behavior = (SRC / 'themes/windows11/behavior.dconf').read_text().splitlines()
        tray = next(line for line in behavior if line.startswith('toggle-message-tray='))
        text += f'\n[org/gnome/shell/keybindings]\n{tray}\n'
    return text


@pytest.hookimpl(wrapper=True, tryfirst=True)
def pytest_runtest_makereport(item, call):
    rep = yield
    item.stash.setdefault('failed', False)
    if rep.failed:
        item.stash['failed'] = True
    return rep


@pytest.fixture
def make_shell(request):
    """make_shell(skin=None, extensions=(UUID,), dconf='', before_shell=None) -> a started Session.
    before_shell(session) runs once the private bus is up, before GNOME Shell starts."""
    sessions = []
    base = request.node.name.replace('/', '_').replace('[', '-').replace(']', '')

    def make(skin=None, extensions=(UUID,), dconf='', check_errors=True, before_shell=None):
        name = base if not sessions else f'{base}-{len(sessions)}'
        extra = (skin_dconf(skin) + '\n' if skin else '') + dconf
        s = Session(WORK / name, OUT, name, extensions=extensions, dconf=extra,
                    before_shell=before_shell)
        s.check_errors = check_errors
        sessions.append(s)
        return s.start()

    yield make
    failed = request.node.stash.get('failed', False)
    problems = []
    for s in sessions:
        if failed and s.shell_proc and s.shell_proc.poll() is None:
            with contextlib.suppress(Exception):
                s.screenshot('failure')
        s.stop()
        if s.check_errors:
            problems += s.errors_from((UUID, 'desk-bridge'))
    if not failed and not os.environ.get('E2E_KEEP_LOGS'):
        for s in sessions:
            # passing tests keep no logs (the CI artifact holds the failures only)
            with contextlib.suppress(FileNotFoundError):
                s.log_path.unlink()
    assert not problems, 'errors from desk-bridge in the shell log:\n' + '\n\n'.join(problems)


class Clipboard:
    """deskd's clipboard over the session bus, with a wait on its Changed signal."""

    def __init__(self, shell):
        self.shell = shell

    def call(self, method, args, rtype):
        """A call from the test process itself: a foreign client, which deskd refuses content."""
        return self.shell.call(DESK, CLIP_PATH, CLIP_IFACE, method, args, rtype).unpack()

    def texts(self):
        """The history as the bridge reads it (deskd serves it to the shell only, ADR 0008)."""
        return self.shell.hook('clipTexts')

    def copy_and_wait(self, text, procs):
        """wl-copy `text` (kept running in the foreground, it owns the selection) and wait for
        deskd to report it in the history."""
        def setup(done):
            def changed(*_):
                if text in self.texts():
                    done(True)
            sid = self.shell.bus.signal_subscribe(DESK, CLIP_IFACE, 'Changed', CLIP_PATH, None,
                                                  Gio.DBusSignalFlags.NONE, changed)
            procs.append(self.shell.spawn('wl-copy', '--foreground', '--', text,
                                          stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                          stderr=subprocess.DEVNULL))
            return lambda: self.shell.bus.signal_unsubscribe(sid)
        return wait_for(setup, 20, f'deskd to record {text!r}')


@pytest.fixture
def procs():
    """Processes a test spawns; terminated (by their own PID) at the end."""
    started = []
    yield started
    for p in started:
        if p.poll() is None:
            p.terminate()
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()


class LineReader:
    """JSON lines of a child's stdout, read through the GLib main loop (no polling)."""

    def __init__(self, proc):
        self.stream = Gio.DataInputStream.new(Gio.UnixInputStream.new(proc.stdout.fileno(), False))

    def wait(self, pred, what, timeout_s=20):
        def setup(done):
            cancel = Gio.Cancellable()

            def got(stream, res):
                try:
                    line, _ = stream.read_line_finish_utf8(res)
                except GLib.Error:
                    return
                if line is None:
                    done(None)
                    return
                obj = json.loads(line)
                if pred(obj):
                    done(obj)
                else:
                    stream.read_line_async(GLib.PRIORITY_DEFAULT, cancel, got)
            self.stream.read_line_async(GLib.PRIORITY_DEFAULT, cancel, got)
            return cancel.cancel
        return wait_for(setup, timeout_s, what)


@pytest.fixture
def paste_target(procs):
    def start(shell):
        p = shell.spawn('python3', str(pathlib.Path(__file__).with_name('harness') / 'paste_target.py'),
                        stdout=subprocess.PIPE, stdin=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        procs.append(p)
        reader = LineReader(p)
        reader.wait(lambda o: o.get('ready'), 'the paste target to get the focus')
        return reader
    return start


def themes(shell, *args, check=False):
    """Run the built `themes` CLI with --json inside the session; returns (rc, parsed JSON or text)."""
    r = shell.run('themes', '--json', *args, timeout=180)
    try:
        out = json.loads(r.stdout) if r.stdout.strip() else None
    except json.JSONDecodeError:
        out = r.stdout
    if check and r.returncode != 0:
        raise AssertionError(f'themes {" ".join(args)} -> {r.returncode}\n{r.stdout}\n{r.stderr}')
    return r.returncode, out, r.stderr
