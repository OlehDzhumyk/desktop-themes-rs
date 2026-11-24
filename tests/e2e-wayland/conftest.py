# SPDX-License-Identifier: GPL-3.0-or-later
"""Harness of the Wayland end-to-end tests: one headless labwc session with deskd and desk-shell.

Runs INSIDE the test container (tools/e2e-wayland.sh). The container is the isolation boundary
(ADR 0007); the session also gets its own HOME, XDG_RUNTIME_DIR, XDG_CONFIG_HOME, GSettings
(memory backend) and a private session bus under a fresh work directory.

labwc runs with the wlroots headless backend and the pixman renderer (no GPU, no seat, no
systemd). desk-shell runs with WAYLAND_DEBUG=client, so its stderr shows the Wayland requests it
sends: the tests see the layer surfaces it maps and unmaps. Nothing sleeps: the harness waits on
D-Bus name ownership, on labwc's start-up command and on stderr lines; timeouts only bound a hang.
"""

import os
import pathlib
import re
import select
import shutil
import subprocess
import tempfile
import threading

import pytest

from gi.repository import Gio, GLib

try:
    from gi.repository import GLibUnix
    fd_add = GLibUnix.fd_add_full
except ImportError:  # PyGObject before GLibUnix
    fd_add = GLib.unix_fd_add_full

SRC = pathlib.Path(os.environ.get('E2E_SRC', '/src'))
BIN = pathlib.Path(os.environ.get('E2E_BIN', SRC / 'target/debug'))
OUT = pathlib.Path(os.environ.get('E2E_OUT', '/tmp/e2e-wayland-out'))
TIMEOUT = 60

DESK = 'io.github.OlehDzhumyk.Desk'
CLIPBOARD_PATH = '/io/github/OlehDzhumyk/Desk/Clipboard'
CLIPBOARD_IFACE = 'io.github.OlehDzhumyk.Desk.Clipboard'
DESK_SHELL = 'io.github.OlehDzhumyk.DeskShell'
# the soname desk-shell opens; /proc/PID/maps shows the file it resolves to (….so.1.3.0)
LAYER_LIBRARY = 'libgtk4-layer-shell.so'

# WAYLAND_DEBUG lines of libwayland-client: `[stamp] {queue} -> object#id.request(args)`
PROTOCOL = re.compile(r'^\[\s*\d+\.\d+\]')
NEW_LAYER = (r'-> zwlr_layer_shell_v1#\d+\.get_layer_surface'
             r'\(new id zwlr_layer_surface_v1#(\d+),.*"({ns})"\)')
DESTROY_LAYER = r'-> zwlr_layer_surface_v1#({id})\.destroy\(\)'


def new_layer(namespace=None):
    """get_layer_surface for `namespace` (any when None): groups = object id, namespace."""
    return re.compile(NEW_LAYER.format(ns=r'[^"]*' if namespace is None else re.escape(namespace)))


def destroy_layer(object_id=None):
    """zwlr_layer_surface_v1.destroy of `object_id` (any when None): group 1 = object id."""
    return re.compile(DESTROY_LAYER.format(id=r'\d+' if object_id is None else object_id))
# a crash or a critical in a GLib/GTK log line, a Rust panic, or a fatal signal
PROBLEM = re.compile(r'CRITICAL|-ERROR|\bERROR\b|panicked at|Segmentation fault|SIGSEGV|SIGABRT')


class HarnessError(Exception):
    pass


def wait_for(cond_setup, timeout_s, what):
    """Run a GLib main loop until `cond_setup(done)` calls done(value); return that value.

    cond_setup subscribes to an event and calls done() from its handler (or right away when the
    condition already holds); it may return a cleanup function. The timeout only bounds a hang.
    """
    loop = GLib.MainLoop()
    box = {}

    def done(value=True):
        if 'v' not in box:
            box['v'] = value
            loop.quit()

    cleanup = cond_setup(done)
    if 'v' not in box:
        def expire():
            box['timeout'] = True
            loop.quit()
            return GLib.SOURCE_REMOVE
        tid = GLib.timeout_add_seconds(timeout_s, expire)
        loop.run()
        if 'timeout' not in box:
            GLib.source_remove(tid)
    if cleanup:
        cleanup()
    if 'v' not in box:
        raise TimeoutError(f'timed out after {timeout_s}s waiting for {what}')
    return box['v']


class LineLog:
    """Collects a child's output lines from a reader thread into a file and a list."""

    def __init__(self, stream, path):
        self.lines = []
        self.cond = threading.Condition()
        self.closed = False
        self._file = open(path, 'w', encoding='utf-8')
        self._thread = threading.Thread(target=self._read, args=(stream,), daemon=True)
        self._thread.start()

    def _read(self, stream):
        for raw in stream:
            line = raw.decode('utf-8', 'replace').rstrip('\n')
            self._file.write(line + '\n')
            with self.cond:
                self.lines.append(line)
                self.cond.notify_all()
        self._file.close()
        with self.cond:
            self.closed = True
            self.cond.notify_all()

    def mark(self):
        with self.cond:
            return len(self.lines)

    def wait(self, pattern, since=0, timeout_s=TIMEOUT):
        """The first match of `pattern` in a line from index `since` on (waits on new lines)."""
        rx = re.compile(pattern) if isinstance(pattern, str) else pattern
        with self.cond:
            i = since
            while True:
                while i < len(self.lines):
                    m = rx.search(self.lines[i])
                    i += 1
                    if m:
                        return m
                if self.closed:
                    raise HarnessError(f'output ended before a line matched {rx.pattern!r}')
                if not self.cond.wait(timeout_s):
                    raise TimeoutError(f'timed out after {timeout_s}s waiting for {rx.pattern!r}')

    def snapshot(self):
        with self.cond:
            return list(self.lines)

    def join(self):
        self._thread.join(TIMEOUT)


def problems(lines):
    """Lines that report a crash or a critical (protocol traces are skipped)."""
    return [line for line in lines if not PROTOCOL.match(line) and PROBLEM.search(line)]


class Session:
    """A private bus, headless labwc, deskd and desk-shell --skin windows11."""

    def __init__(self, work, out):
        self.work = pathlib.Path(work)
        self.out = pathlib.Path(out)
        self.home = self.work / 'home'
        self.runtime = self.work / 'run'
        self.config = self.home / '.config'
        self.data = self.home / '.local/share'
        self.procs = {}
        self.logs = {}
        self.bus = None
        self.env = None
        self.wayland_display = None

    # ---- set-up
    def _install(self):
        for d in (self.home, self.config, self.data, self.runtime):
            d.mkdir(parents=True, exist_ok=True)
        self.runtime.chmod(0o700)
        for b in ('deskd', 'desk-shell'):
            if not (BIN / b).is_file():
                raise HarnessError(f'{BIN / b} is not built (cargo build -p deskd -p desk-shell)')
        schemas = self.work / 'schemas'
        schemas.mkdir()
        shutil.copy(SRC / 'data/io.github.OlehDzhumyk.Desk.gschema.xml', schemas)
        subprocess.run(['glib-compile-schemas', str(schemas)], check=True)
        # what desk-win11-session installs: the bundled rc.xml and the desk-win11 labwc theme
        labwc = self.config / 'desk-labwc'
        labwc.mkdir()
        shutil.copy(SRC / 'data/labwc/windows11/rc.xml', labwc / 'rc.xml')
        theme = self.data / 'themes/desk-win11/openbox-3'
        theme.mkdir(parents=True)
        shutil.copy(SRC / 'data/labwc/windows11/themerc', theme / 'themerc')
        # labwc runs this once its Wayland socket accepts clients
        ready = self.work / 'ready.sh'
        ready.write_text('#!/bin/sh\nprintf \'%s\\n\' "$WAYLAND_DISPLAY" > "$1"\n')
        ready.chmod(0o755)

    def _environment(self):
        return {
            'PATH': '/usr/local/bin:/usr/bin:/bin',
            'HOME': str(self.home),
            'USER': os.environ.get('USER', 'root'),
            'LANG': 'C.UTF-8',
            'XDG_RUNTIME_DIR': str(self.runtime),
            'XDG_CONFIG_HOME': str(self.config),
            'XDG_DATA_HOME': str(self.data),
            'XDG_CACHE_HOME': str(self.home / '.cache'),
            'XDG_STATE_HOME': str(self.home / '.local/state'),
            'XDG_DATA_DIRS': '/usr/local/share:/usr/share',
            'XDG_SESSION_TYPE': 'wayland',
            'XDG_CURRENT_DESKTOP': 'labwc:wlroots',
            'DESKD_RUNTIME_DIR': str(self.runtime / 'deskd'),
            'GSETTINGS_BACKEND': 'memory',
            'GSETTINGS_SCHEMA_DIR': str(self.work / 'schemas'),
            'WLR_BACKENDS': 'headless',
            'WLR_RENDERER': 'pixman',
            'WLR_HEADLESS_OUTPUTS': '1',
            'GDK_BACKEND': 'wayland',
            'GSK_RENDERER': 'cairo',
            'LIBGL_ALWAYS_SOFTWARE': '1',
            'NO_AT_BRIDGE': '1',
            'GTK_A11Y': 'none',
        }

    def _spawn(self, name, argv, env=None, stdin=subprocess.DEVNULL):
        if name in self.procs:
            raise HarnessError(f'a process named {name} was already spawned')
        proc = subprocess.Popen(argv, env=env or self.env, stdin=stdin,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.procs[name] = proc
        self.logs[name] = LineLog(proc.stdout, self.out / f'{name}.log')
        return proc

    def start(self):
        self.out.mkdir(parents=True, exist_ok=True)
        self._install()
        self.env = self._environment()
        # the private session bus; only the address line goes to the pipe
        sock = self.runtime / 'bus'
        bus = subprocess.Popen(
            ['dbus-daemon', '--session', '--nofork', '--nopidfile', f'--address=unix:path={sock}',
             '--print-address=1'],
            env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=open(self.out / 'dbus-daemon.log', 'wb'))
        self.procs['dbus-daemon'] = bus
        address = bus.stdout.readline().decode().strip()
        if not address or bus.poll() is not None:
            raise HarnessError('dbus-daemon did not start (see dbus-daemon.log)')
        self.env['DBUS_SESSION_BUS_ADDRESS'] = address
        self.bus = Gio.DBusConnection.new_for_address_sync(
            address,
            Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
            | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION)
        self._start_labwc()
        self.env['WAYLAND_DISPLAY'] = self.wayland_display
        # the order of data/labwc/windows11/autostart: deskd, then desk-shell
        self._spawn('deskd', [str(BIN / 'deskd')])
        self.wait_name(DESK, 'deskd')
        self._spawn('desk-shell', [str(BIN / 'desk-shell'), '--skin', 'windows11'],
                    env={**self.env, 'WAYLAND_DEBUG': 'client'})
        self.wait_name(DESK_SHELL, 'desk-shell')
        return self

    def _start_labwc(self):
        fifo = self.work / 'ready.fifo'
        os.mkfifo(fifo)
        # read-write: the open does not block and never sees end-of-file before the writer
        fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
        try:
            self._spawn('labwc', ['labwc', '-C', str(self.config / 'desk-labwc'),
                                  '-s', f'{self.work / "ready.sh"} {fifo}'])
            readable, _, _ = select.select([fd], [], [], TIMEOUT)
            if not readable:
                raise HarnessError(f'labwc did not start in {TIMEOUT}s (see labwc.log)')
            self.wayland_display = os.read(fd, 256).decode().strip()
        finally:
            os.close(fd)
        if not self.wayland_display or not (self.runtime / self.wayland_display).is_socket():
            raise HarnessError(f'labwc reported no Wayland socket: {self.wayland_display!r}')

    # ---- D-Bus
    def name_owner(self, name):
        try:
            reply = self.bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus',
                                       'org.freedesktop.DBus', 'GetNameOwner',
                                       GLib.Variant('(s)', (name,)), GLib.VariantType('(s)'),
                                       Gio.DBusCallFlags.NONE, -1, None)
        except GLib.Error:
            return None
        return reply.unpack()[0]

    def wait_name(self, name, proc_name):
        """Wait until `name` has an owner; fail early when the process exits first."""
        proc = self.procs[proc_name]

        def setup(done):
            def appeared(_conn, _name, owner):
                done(owner)

            watch = Gio.bus_watch_name_on_connection(
                self.bus, name, Gio.BusNameWatcherFlags.NONE, appeared, None)

            # a pidfd turns readable when the process exits, without reaping it (Popen does)
            pidfd = os.pidfd_open(proc.pid)

            fired = []

            def exited(_fd, _cond):
                fired.append(True)
                done(None)
                return GLib.SOURCE_REMOVE

            src = fd_add(GLib.PRIORITY_DEFAULT, pidfd, GLib.IOCondition.IN, exited)

            def cleanup():
                Gio.bus_unwatch_name(watch)
                if not fired:
                    GLib.source_remove(src)
                os.close(pidfd)
            return cleanup

        owner = wait_for(setup, TIMEOUT, f'{proc_name} to own {name}')
        if owner is None:
            raise HarnessError(f'{proc_name} exited before it owned {name} '
                               f'(see {proc_name}.log)')
        return owner

    def wait_signal(self, iface, member, path, trigger, what):
        """Subscribe to `member` of `iface` from deskd, then call trigger(); wait for the signal.

        The subscription is made before the trigger runs, so a signal sent at once is not missed.
        """
        sender = self.name_owner(DESK)
        if not sender:
            raise HarnessError(f'{DESK} has no owner')

        def setup(done):
            def received(_conn, _sender, _path, _iface, _member, params):
                done(params)

            sid = self.bus.signal_subscribe(sender, iface, member, path, None,
                                            Gio.DBusSignalFlags.NONE, received)
            trigger()
            return lambda: self.bus.signal_unsubscribe(sid)
        return wait_for(setup, TIMEOUT, what)

    # ---- files deskd writes
    @property
    def clipboard_history(self):
        """deskd's history file: $XDG_DATA_HOME/deskd/clipboard/history.json (clipboard.rs)."""
        return self.data / 'deskd/clipboard/history.json'

    def wait_file(self, path, accept, what):
        """Wait until the text of `path` satisfies accept(text); return the text.

        A GIO file monitor (inotify) wakes the check on every change of the file, also on the
        atomic rename that deskd's saver ends with. The monitor is made before the first check.
        """
        path = pathlib.Path(path)

        def setup(done):
            monitor = Gio.File.new_for_path(str(path)).monitor_file(
                Gio.FileMonitorFlags.NONE, None)

            def check(*_args):
                try:
                    text = path.read_text(encoding='utf-8')
                except FileNotFoundError:
                    return
                if accept(text):
                    done(text)

            monitor.connect('changed', check)
            check()
            return monitor.cancel
        return wait_for(setup, TIMEOUT, what)

    # ---- clients of the session
    def spawn_client(self, name, argv, stdin=subprocess.DEVNULL):
        """A Wayland or D-Bus client of the session; stop() ends it if it still runs."""
        return self._spawn(name, argv, stdin=stdin)

    def wait_exit(self, name):
        """Wait for a spawned process to exit; its exit status."""
        return self.procs[name].wait(TIMEOUT)

    # ---- the running shell
    def run_shell(self, *args):
        """A second desk-shell (as from a labwc key binding): it talks to the running one."""
        return subprocess.run([str(BIN / 'desk-shell'), *args], env=self.env,
                              stdin=subprocess.DEVNULL, capture_output=True, timeout=TIMEOUT)

    def alive(self, name):
        return self.procs[name].poll() is None

    def stop(self):
        if self.bus:
            self.bus.close_sync(None)
            self.bus = None
        # only the processes this session spawned, newest first
        for name in reversed(list(self.procs)):
            proc = self.procs[name]
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(10)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
        for log in self.logs.values():
            log.join()


@pytest.fixture(scope='module')
def session():
    work = tempfile.mkdtemp(prefix='e2e-wayland-')
    s = Session(work, OUT)
    try:
        s.start()
    except BaseException:
        s.stop()
        raise
    yield s
    s.stop()
    if not os.environ.get('E2E_KEEP_WORK'):
        shutil.rmtree(work, ignore_errors=True)
