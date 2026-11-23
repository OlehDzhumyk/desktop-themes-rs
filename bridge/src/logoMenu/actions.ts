// SPDX-License-Identifier: GPL-3.0-or-later
// Actions triggered by the macOS-skin logo menu in desk-bridge.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {confirm} from '../confirmDialog.js';
import {_, fmt} from '../i18n.js';
import {errorText, focusWindow} from '../shellTypes.js';

function callDbus(
    bus: Gio.DBusConnection,
    dest: string,
    path: string,
    iface: string,
    method: string,
    params: GLib.Variant | null,
): void {
    bus.call(
        dest,
        path,
        iface,
        method,
        params,
        null,
        Gio.DBusCallFlags.NONE,
        -1,
        null,
        (_conn: Gio.DBusConnection | null, res: Gio.AsyncResult) => {
            try {
                bus.call_finish(res);
            } catch (e) {
                console.warn(`desk-bridge: logo-menu: ${method}: ${errorText(e)}`);
            }
        },
    );
}

/** Open the System / About page in GNOME Settings. */
export function openAbout(): void {
    try {
        Gio.Subprocess.new(['gnome-control-center', 'system'], Gio.SubprocessFlags.NONE);
    } catch (e) {
        Main.notifyError(_('Logo menu'), errorText(e));
    }
}

/** Open GNOME Settings. */
export function openSettings(): void {
    try {
        const app = Shell.AppSystem.get_default().lookup_app('org.gnome.Settings.desktop') as Shell.App | null;
        if (app !== null) {
            app.activate();
            return;
        }
        Gio.Subprocess.new(['gnome-control-center'], Gio.SubprocessFlags.NONE);
    } catch (e) {
        Main.notifyError(_('Logo menu'), errorText(e));
    }
}

/** Show the Applications grid in the GNOME overview. */
export function showApps(): void {
    Main.overview.showApps();
}

/** Ask for confirmation and kill the currently focused window. */
export async function forceQuitFocused(): Promise<void> {
    const win = focusWindow();
    if (!win)
        return;
    const maybeTitle = win.get_title() as string | null;
    const raw = maybeTitle !== null ? maybeTitle.trim() : '';
    const title = raw.length > 0 ? raw : _('this window');
    const ok = await confirm(
        fmt(_('Force quit “{title}”?'), {title}),
        _('Unsaved changes in this window will be lost.'),
        _('Force Quit'),
    );
    if (ok)
        win.kill();
}

/** Suspend the machine via logind. */
export function suspend(): void {
    callDbus(
        Gio.DBus.system,
        'org.freedesktop.login1',
        '/org/freedesktop/login1',
        'org.freedesktop.login1.Manager',
        'Suspend',
        new GLib.Variant('(b)', [true]),
    );
}

/** Reboot via GNOME SessionManager (shows the confirmation dialog). */
export function restart(): void {
    callDbus(
        Gio.DBus.session,
        'org.gnome.SessionManager',
        '/org/gnome/SessionManager',
        'org.gnome.SessionManager',
        'Reboot',
        null,
    );
}

/** Shut down via GNOME SessionManager (shows the confirmation dialog). */
export function shutDown(): void {
    callDbus(
        Gio.DBus.session,
        'org.gnome.SessionManager',
        '/org/gnome/SessionManager',
        'org.gnome.SessionManager',
        'Shutdown',
        null,
    );
}

/** Lock the screen via GNOME ScreenSaver. */
export function lockScreen(): void {
    callDbus(
        Gio.DBus.session,
        'org.gnome.ScreenSaver',
        '/org/gnome/ScreenSaver',
        'org.gnome.ScreenSaver',
        'Lock',
        null,
    );
}

/** Log out via GNOME SessionManager (mode 0 = normal, with confirmation). */
export function logOut(): void {
    callDbus(
        Gio.DBus.session,
        'org.gnome.SessionManager',
        '/org/gnome/SessionManager',
        'org.gnome.SessionManager',
        'Logout',
        new GLib.Variant('(u)', [0]),
    );
}
