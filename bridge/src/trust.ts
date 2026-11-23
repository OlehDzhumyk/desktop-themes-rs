// SPDX-License-Identifier: GPL-3.0-or-later
// Is the owner of io.github.OlehDzhumyk.Desk the deskd that is installed (ADR 0008)? A process that
// owned the name would receive every copied text (Push) and decide what the bridge pastes into the
// focused window, terminals included. The bridge sends and pastes nothing until the owner passes:
//   - the same user (UnixUserID of GetConnectionCredentials);
//   - the same root directory as the shell (/proc/<pid>/root): sandboxed apps have their own;
//   - its executable is the file (device and inode, so links count) that the D-Bus activation file
//     of deskd names, or ~/.local/bin/deskd (the systemd unit's ExecStart).
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

export interface FileId {
    dev: number;
    ino: number;
}

export interface Peer {
    uid: number | null;
    exe: FileId | null;
    root: FileId | null;
}

export interface Policy {
    uid: number;
    root: FileId;
    exes: FileId[];
}

const same = (a: FileId | null, b: FileId): boolean => a !== null && a.dev === b.dev && a.ino === b.ino;

/** Why `peer` is not our deskd, or null when it is. */
export function untrustedReason(peer: Peer, policy: Policy): string | null {
    if (peer.uid !== policy.uid)
        return 'another user';
    if (!same(peer.root, policy.root))
        return 'sandboxed (another root directory)';
    if (!policy.exes.some(e => same(peer.exe, e)))
        return 'not the installed deskd executable';
    return null;
}

/** Device and inode of the file `path` leads to (links followed); null if it cannot be read. */
export function fileId(path: string): FileId | null {
    try {
        const info = Gio.File.new_for_path(path).query_info('unix::device,unix::inode',
            Gio.FileQueryInfoFlags.NONE, null);
        return {dev: info.get_attribute_uint32('unix::device'), ino: info.get_attribute_uint64('unix::inode')};
    } catch {
        return null;
    }
}

/** The deskd executables the session is set up to start. */
export function deskdPaths(busName: string): string[] {
    const out: string[] = [];
    for (const dir of [GLib.get_user_data_dir(), ...GLib.get_system_data_dirs()]) {
        try {
            const kf = new GLib.KeyFile();
            kf.load_from_file(`${dir}/dbus-1/services/${busName}.service`, GLib.KeyFileFlags.NONE);
            const [, argv] = GLib.shell_parse_argv(kf.get_string('D-BUS Service', 'Exec'));
            if (argv?.[0])
                out.push(argv[0]);
        } catch {
            // no such file, or no Exec line
        }
    }
    out.push(`${GLib.get_home_dir()}/.local/bin/deskd`);
    return out;
}

/** A method call on the session bus as a promise. */
export function dbusCall(dest: string, path: string, iface: string, method: string, params: GLib.Variant,
    replyType: string, flags = Gio.DBusCallFlags.NONE): Promise<GLib.Variant> {
    return new Promise((resolve, reject) => {
        Gio.DBus.session.call(dest, path, iface, method, params, new GLib.VariantType(replyType), flags, -1,
            null, (_c: Gio.DBusConnection | null, res: Gio.AsyncResult) => {
                try {
                    resolve(Gio.DBus.session.call_finish(res));
                } catch (e) {
                    reject(e instanceof Error ? e : new Error(String(e)));
                }
            });
    });
}

function busCall(method: string, params: GLib.Variant, replyType: string): Promise<GLib.Variant> {
    return dbusCall('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', method, params,
        replyType);
}

/** What the bus and /proc say about the process behind the unique name `owner`. */
export async function peerOf(owner: string): Promise<Peer> {
    const reply = await busCall('GetConnectionCredentials', new GLib.Variant('(s)', [owner]), '(a{sv})');
    const [creds] = reply.deepUnpack<[Record<string, GLib.Variant>]>();
    const uid = creds['UnixUserID']?.deepUnpack<number>() ?? null;
    const pid = creds['ProcessID']?.deepUnpack<number>() ?? null;
    if (pid === null)
        return {uid, exe: null, root: null};
    const dir = `/proc/${String(pid)}`;
    return {uid, exe: fileId(`${dir}/exe`), root: fileId(`${dir}/root`)};
}

/** What the owner must match, resolved now (an install replaces the binary). */
export function policyFor(busName: string): Policy | null {
    const root = fileId('/proc/self/root');
    const uid = new Gio.Credentials().get_unix_user();
    if (!root)
        return null;
    const exes = deskdPaths(busName).map(fileId).filter((f): f is FileId => f !== null);
    return {uid, root, exes};
}

/** The unique name owning `busName`; starts the service (D-Bus activation) when nobody owns it. */
export async function ownerOf(busName: string): Promise<string> {
    try {
        const [owner] = (await busCall('GetNameOwner', new GLib.Variant('(s)', [busName]), '(s)'))
            .deepUnpack<[string]>();
        return owner;
    } catch {
        await busCall('StartServiceByName', new GLib.Variant('(su)', [busName, 0]), '(u)');
        const [owner] = (await busCall('GetNameOwner', new GLib.Variant('(s)', [busName]), '(s)'))
            .deepUnpack<[string]>();
        return owner;
    }
}
