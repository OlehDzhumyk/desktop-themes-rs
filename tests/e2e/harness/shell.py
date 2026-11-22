# SPDX-License-Identifier: GPL-3.0-or-later
"""One headless GNOME Shell session of the end-to-end harness.

Runs INSIDE the test container (tools/e2e.sh): the container is the isolation boundary of
ADR 0005/0007, and each session also gets its own HOME, XDG dirs, dconf database and private
session bus under a fresh work directory, so sessions never see each other's state.

The shell starts with --unsafe-mode so that org.gnome.Shell.Eval works (GNOME Shell 51 only
refuses Eval outside unsafe mode). Through Eval the tests load harness/hook.js into the shell.
Approach (session bus + shell + Eval + context.terminate) after ddterm's tests/fixtures.py and
tests/shelldbus.py (GPL-3.0-or-later, Copyright 2022-2024 Aleksandr Mezin,
https://github.com/ddterm/gnome-shell-extension-ddterm); rewritten for our layout.
"""

import json
import os
import pathlib
import shutil
import signal
import subprocess

from gi.repository import Gio, GLib

from . import logparser

SRC = pathlib.Path(os.environ.get('E2E_SRC', '/src'))
HOOK = pathlib.Path(__file__).with_name('hook.js')
UUID = 'desk-bridge@desktop-themes-rs'
SHELL_TIMEOUT = 60


class ShellError(Exception):
    pass


def wait_for(cond_setup, timeout_s, what):
    """Run a GLib main loop until `cond_setup(done)` calls done(value); return that value.

    cond_setup subscribes to an event and calls done() from its handler (or right away when the
    condition already holds). The timeout only bounds a hang; it is not part of what is tested.
    """
    loop = GLib.MainLoop()
    box = {}

    def done(value=True):
        if 'v' not in box:
            box['v'] = value
            loop.quit()

    cleanup = cond_setup(done)
    if 'v' not in box:
        tid = GLib.timeout_add_seconds(timeout_s, lambda: (box.setdefault('timeout', True), loop.quit())[1])
        loop.run()
        if 'timeout' not in box:
            GLib.source_remove(tid)
    if cleanup:
        cleanup()
    if 'v' not in box:
        raise TimeoutError(f'timed out after {timeout_s}s waiting for {what}')
    return box['v']


class Session:
    """A private bus + gnome-shell --headless with a virtual monitor."""

    def __init__(self, workdir, artifacts, name, *, extensions=(UUID,), dconf=''):
        self.work = pathlib.Path(workdir)
        self.artifacts = pathlib.Path(artifacts)
        self.name = name
        self.extensions = list(extensions)
        self.dconf_init = dconf
        self.home = self.work / 'home'
        self.runtime = self.work / 'run'
        self.data = self.home / '.local/share'
        self.config = self.home / '.config'
        self.themes_root = self.data / 'desktop-themes'
        self.log_path = self.artifacts / f'{name}.shell.log'
        self.bus = None
        self.bus_proc = None
        self.shell_proc = None
        self._log = None
        self.env = None

    # ---- set-up
    def _install(self):
        for d in (self.home, self.data, self.config, self.runtime):
            d.mkdir(parents=True, exist_ok=True)
        self.runtime.chmod(0o700)
        ext_dir = self.data / 'gnome-shell/extensions'
        ext_dir.mkdir(parents=True, exist_ok=True)
        built = SRC / 'bridge/build' / UUID
        if not (built / 'metadata.json').is_file():
            raise ShellError(f'bridge is not built: {built} (run npm run build in bridge/)')
        # a copy, not a link: the shell loads the files the test installed, like a real install
        shutil.copytree(built, ext_dir / UUID)
        lockscreen = SRC / 'extensions/lockscreen-layout@desktop-themes-rs'
        if lockscreen.is_dir():
            # as data/install.sh does: copy, then compile the extension's own schemas
            shutil.copytree(lockscreen, ext_dir / lockscreen.name)
            subprocess.run(['glib-compile-schemas', str(ext_dir / lockscreen.name / 'schemas')], check=True)
        schemas = self.data / 'glib-2.0/schemas'
        schemas.mkdir(parents=True)
        shutil.copy(SRC / 'data/io.github.OlehDzhumyk.Desk.gschema.xml', schemas)
        subprocess.run(['glib-compile-schemas', str(schemas)], check=True)
        bindir = self.home / '.local/bin'
        bindir.mkdir(parents=True)
        for b in ('deskd', 'themes'):
            exe = SRC / 'target/debug' / b
            if not exe.is_file():
                raise ShellError(f'{exe} is not built (cargo build -p deskd -p themes)')
            (bindir / b).symlink_to(exe)
        services = self.data / 'dbus-1/services'
        services.mkdir(parents=True)
        (services / 'io.github.OlehDzhumyk.Desk.service').write_text(
            f'[D-BUS Service]\nName=io.github.OlehDzhumyk.Desk\nExec={bindir}/deskd\n')
        (services / 'io.github.OlehDzhumyk.Themes.service').write_text(
            f'[D-BUS Service]\nName=io.github.OlehDzhumyk.Themes\nExec={bindir}/themes --dbus-service\n')
        # theme root: the repository's themes (read-only mount), state and caches in the session
        self.themes_root.mkdir(parents=True)
        (self.themes_root / 'themes').symlink_to(SRC / 'themes')

    def _environment(self):
        env = {
            'PATH': f'{self.home}/.local/bin:/usr/local/bin:/usr/bin:/bin',
            'HOME': str(self.home),
            'USER': os.environ.get('USER', 'root'),
            'LANG': 'C.UTF-8',
            'XDG_RUNTIME_DIR': str(self.runtime),
            'XDG_DATA_HOME': str(self.data),
            'XDG_CONFIG_HOME': str(self.config),
            'XDG_CACHE_HOME': str(self.home / '.cache'),
            'XDG_DATA_DIRS': '/usr/local/share:/usr/share',
            'XDG_SESSION_TYPE': 'wayland',
            'XDG_CURRENT_DESKTOP': 'GNOME',
            'WAYLAND_DISPLAY': 'wayland-e2e',
            'DESKTOP_THEMES_ROOT': str(self.themes_root),
            'DESKD_RUNTIME_DIR': str(self.runtime / 'deskd'),
            'LIBGL_ALWAYS_SOFTWARE': 'true',
            'GBM_ALWAYS_SOFTWARE': 'true',
            'NO_AT_BRIDGE': '1',
            'GTK_A11Y': 'none',
            'GSETTINGS_BACKEND': 'dconf',
        }
        return env

    def start(self):
        self.artifacts.mkdir(parents=True, exist_ok=True)
        self._install()
        self.env = self._environment()
        self._log = open(self.log_path, 'wb')
        sock = self.runtime / 'bus'
        self.bus_proc = subprocess.Popen(
            ['dbus-daemon', '--session', '--nofork', '--nopidfile', f'--address=unix:path={sock}',
             '--print-address=1'],
            env=self.env, stdout=subprocess.PIPE, stderr=self._log, stdin=subprocess.DEVNULL)
        address = self.bus_proc.stdout.readline().decode().strip()
        if not address:
            raise ShellError('dbus-daemon did not start')
        self.env['DBUS_SESSION_BUS_ADDRESS'] = address
        self.bus = Gio.DBusConnection.new_for_address_sync(
            address, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION)
        exts = ', '.join(f"'{e}'" for e in self.extensions) or '@as '
        exts = exts if exts != '@as ' else None
        self.dconf_load(f"""[org/gnome/shell]
enabled-extensions={f'[{exts}]' if exts else '@as []'}
disable-user-extensions=false
welcome-dialog-last-shown-version='999'

[org/gnome/desktop/sound]
event-sounds=false

[org/gnome/desktop/interface]
color-scheme='prefer-dark'
enable-animations=false
""" + self.dconf_init)
        self.shell_proc = subprocess.Popen(
            ['gnome-shell', '--headless', '--unsafe-mode', '--wayland', '--no-x11',
             '--wayland-display=wayland-e2e', '--virtual-monitor', '1920x1080'],
            env=self.env, stdout=self._log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        # org.gnome.Shell is owned before its objects are exported; the mount operation handler
        # is exported last (same signal ddterm waits for)
        self.wait_name('org.gtk.MountOperationHandler')
        self.wait_startup()
        # extensions are enabled asynchronously after start-up
        for uuid in self.extensions:
            if uuid == UUID:
                self.wait_ext_state({1}, uuid)
        self._baseline()
        return self

    def _baseline(self):
        """What scripts/bootstrap.sh does once on a real machine: the dconf snapshot `themes reset`
        returns to, and `fedora` as the current theme."""
        b = self.themes_root / 'baseline-e2e'
        b.mkdir()
        (b / 'full.dconf').write_text(self.run('dconf', 'dump', '/', check=True).stdout)
        (self.themes_root / 'baseline').symlink_to(b.name)
        (self.themes_root / 'current').write_text('fedora\n')

    def wait_name(self, name, timeout_s=SHELL_TIMEOUT):
        def setup(done):
            wid = Gio.bus_watch_name_on_connection(
                self.bus, name, Gio.BusNameWatcherFlags.NONE, lambda *a: done(True), None)
            return lambda: Gio.bus_unwatch_name(wid)
        return wait_for(setup, timeout_s, f'bus name {name}')

    def wait_startup(self):
        """Main.layoutManager finishes its start-up animation ('startup-complete')."""
        self.eval_js("""new Promise(r => { if (!Main.layoutManager._startingUp) r(true);
            else Main.layoutManager.connect('startup-complete', () => r(true)); })""")
        # the shell opens the overview at start when there are no windows; tests start from the
        # desktop (windows opened behind the overview would not get the focus)
        self.eval_js("""new Promise(r => { if (!Main.overview.visible) { r(true); return; }
            const id = Main.overview.connect('hidden', () => { Main.overview.disconnect(id); r(true); });
            Main.overview.hide(); })""")
        # the hidden overview can keep the stage key focus, and then shortcut presses get lost:
        # give the focus back to the (empty) desktop like after a click on it
        self.eval_js('global.stage.set_key_focus(null)')

    # ---- talking to the shell
    def call(self, name, path, iface, method, args=None, rtype=None, timeout_ms=30000):
        return self.bus.call_sync(name, path, iface, method, args,
                                  GLib.VariantType(rtype) if rtype else None,
                                  Gio.DBusCallFlags.NONE, timeout_ms, None)

    def eval_js(self, code, timeout_ms=30000):
        ok, out = self.call('org.gnome.Shell', '/org/gnome/Shell', 'org.gnome.Shell', 'Eval',
                            GLib.Variant('(s)', (code,)), '(bs)', timeout_ms).unpack()
        if not ok:
            raise ShellError(f'Eval failed: {out}')
        return json.loads(out) if out else None

    def hook(self, fn, *args, timeout_ms=30000):
        a = ', '.join(json.dumps(x) for x in args)
        return self.eval_js(f'import({json.dumps(HOOK.as_uri())}).then(m => m.{fn}({a}))', timeout_ms)

    def ext_info(self, uuid=UUID):
        (info,) = self.call('org.gnome.Shell', '/org/gnome/Shell', 'org.gnome.Shell.Extensions',
                            'GetExtensionInfo', GLib.Variant('(s)', (uuid,)), '(a{sv})').unpack()
        return info

    def ext_state(self, uuid=UUID):
        return int(self.ext_info(uuid).get('state', 0))

    def wait_ext_state(self, want, uuid=UUID, action=None, timeout_s=30):
        """Wait (on ExtensionStateChanged) until `uuid` is in one of the states `want` or broken
        (ERROR, OUT_OF_DATE); `action` runs after subscribing. Raises with the shell's errors."""
        final = set(want) | {3, 4}

        def setup(done):
            def on_signal(_c, _s, _p, _i, _n, params):
                u, info = params.unpack()
                if u == uuid and int(info.get('state', 0)) in final:
                    done(int(info['state']))
            sid = self.bus.signal_subscribe('org.gnome.Shell', 'org.gnome.Shell.Extensions',
                                            'ExtensionStateChanged', '/org/gnome/Shell', None,
                                            Gio.DBusSignalFlags.NONE, on_signal)
            if action:
                action()
            if self.ext_state(uuid) in set(want):
                done(self.ext_state(uuid))
            return lambda: self.bus.signal_unsubscribe(sid)
        state = wait_for(setup, timeout_s, f'{uuid} to reach state {sorted(want)}')
        if state not in want:
            (errors,) = self.call('org.gnome.Shell', '/org/gnome/Shell', 'org.gnome.Shell.Extensions',
                                  'GetExtensionErrors', GLib.Variant('(s)', (uuid,)), '(as)').unpack()
            raise ShellError(f'{uuid}: state {state}, errors {errors}')
        return state

    def set_extension(self, enabled, uuid=UUID):
        """Enable/disable through org.gnome.Shell.Extensions (as the Extensions app does)."""
        method = 'EnableExtension' if enabled else 'DisableExtension'

        def action():
            self.call('org.gnome.Shell', '/org/gnome/Shell', 'org.gnome.Shell.Extensions', method,
                      GLib.Variant('(s)', (uuid,)), '(b)')
        state = self.wait_ext_state({1} if enabled else {2}, uuid, action)
        self.hook('settle')
        return state

    def screenshot(self, name):
        path = self.artifacts / f'{self.name}-{name}.png'
        self.hook('screenshot', str(path))
        return path

    def keys(self, spec):
        self.hook('keys', spec)

    # ---- processes inside the session
    def run(self, *cmd, check=False, input=None, timeout=120):
        r = subprocess.run(list(cmd), env=self.env, input=input, capture_output=True, text=True,
                           timeout=timeout)
        if check and r.returncode != 0:
            raise ShellError(f'{cmd} failed ({r.returncode}):\n{r.stdout}\n{r.stderr}')
        return r

    def spawn(self, *cmd, **kw):
        return subprocess.Popen(list(cmd), env=self.env, **kw)

    def dconf_load(self, text, path='/'):
        self.run('dconf', 'load', path, input=text, check=True)

    def dconf_read(self, key):
        return self.run('dconf', 'read', key, check=True).stdout.strip()

    def dconf_write(self, key, value):
        self.run('dconf', 'write', key, value, check=True)

    # ---- tear-down and logs
    def stop(self):
        """End the session like a logout: the shell runs its normal shutdown (context terminate)."""
        if self.shell_proc and self.shell_proc.poll() is None:
            try:
                self.eval_js('global.context.terminate()', timeout_ms=5000)
            except Exception:  # noqa: BLE001 - the shell may already be gone
                self.shell_proc.send_signal(signal.SIGTERM)
            try:
                self.shell_proc.wait(timeout=SHELL_TIMEOUT)
            except subprocess.TimeoutExpired:
                self.shell_proc.kill()
                self.shell_proc.wait()
        if self.bus_proc and self.bus_proc.poll() is None:
            self.bus_proc.terminate()
            self.bus_proc.wait(timeout=SHELL_TIMEOUT)
        if self._log:
            self._log.close()
            self._log = None

    def log_bytes(self):
        if self._log:
            self._log.flush()
        return self.log_path.read_bytes() if self.log_path.exists() else b''

    def records(self):
        """GLib log records of the shell output: ddterm's logparser regex finds the record headers;
        each record also takes the lines up to the next blank line (GJS prints its
        '== Stack trace for context ==' block there, which the regex does not cover)."""
        data = self.log_bytes()
        matches = list(logparser.GLIB_MESSAGE_RE.finditer(data))
        out = []
        for i, m in enumerate(matches):
            end = matches[i + 1].start() if i + 1 < len(matches) else len(data)
            block = data[m.start():end]
            cut = block.find(b'\n\n')
            rec = logparser.parse(m)
            rec['block'] = (block[:cut] if cut >= 0 else block).decode(errors='backslashreplace')
            out.append(rec)
        return out

    def errors_from(self, fragments=(UUID,)):
        """JS errors and criticals whose message or stack mentions one of `fragments`."""
        bad = []
        for r in self.records():
            serious = 'JS ERROR' in r['message'] or r['levelname'] in ('ERROR', 'CRITICAL')
            if serious and any(f in r['block'] for f in fragments):
                bad.append(r['block'])
        return bad

    def gc_warnings(self):
        """Blocks of 'call back into JSAPI during the sweeping phase of GC' (with what was hit)."""
        text = self.log_bytes().decode(errors='replace')
        out = []
        for part in text.split('Attempting to ')[1:]:
            if 'garbage collection' in part[:200] or 'sweeping phase of GC' in part[:200]:
                lines = part.splitlines()
                out.append('\n'.join(lines[:12]))
        return out
