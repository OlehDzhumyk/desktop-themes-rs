// SPDX-License-Identifier: GPL-3.0-or-later
//! Who may read the clipboard history and drive the mixer (ADR 0008).
//!
//! deskd is on the session bus, which every process of the user can reach, sandboxed apps with
//! bus access included. Methods that return or change user content therefore check the caller:
//!
//! 1. the caller runs as the same user (`UnixUserID` of `GetConnectionCredentials`);
//! 2. it sees the same root directory as deskd (`/proc/<pid>/root`): Flatpak, Snap and other
//!    sandboxes give the app its own root, and an app behind `xdg-dbus-proxy` reaches the bus
//!    through the proxy, which lives in the sandbox too;
//! 3. it is the current owner of `org.gnome.Shell` (the bridge runs in the shell), or its
//!    executable is one of our own binaries: the same file (device and inode, so links and
//!    renamed copies do not matter) as deskd itself or the `themes` binary next to it.
//!
//! Everything is resolved per call: the owner of `org.gnome.Shell` is asked each time, so a
//! restarted shell (or a process that took the name over) is followed without a watcher.
//! The pid comes with a pidfd when the bus offers one (`ProcessFD`); the process must still be
//! that pid after `/proc` was read, so a reused pid cannot borrow another process's identity.
//! Same-user processes outside a sandbox are not a boundary we can hold (they can read
//! `~/.local/share/deskd` directly); see SECURITY.md.

use std::collections::HashSet;
use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use zbus::message::Header;

/// The bus name of GNOME Shell, which hosts the bridge.
pub const SHELL_NAME: &str = "org.gnome.Shell";
/// At most this many refused senders are logged (a misbehaving client cannot flood the journal).
const MAX_LOGGED: usize = 64;

/// A file by device and inode (what `stat` follows a link to).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileId {
    pub dev: u64,
    pub ino: u64,
}

impl FileId {
    pub fn of(p: &Path) -> Option<FileId> {
        std::fs::metadata(p).ok().map(|m| FileId {
            dev: m.dev(),
            ino: m.ino(),
        })
    }
}

/// What deskd learned about a caller. `None`: unknown (and then not trusted).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Peer {
    pub uid: Option<u32>,
    pub exe: Option<FileId>,
    pub root: Option<FileId>,
}

/// What a trusted caller must match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub uid: u32,
    pub root: FileId,
    /// Our own binaries.
    pub exes: Vec<FileId>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The current owner of `org.gnome.Shell`.
    Shell,
    /// One of our binaries.
    OwnBinary,
    Denied(&'static str),
}

/// The rule of the module docs, without any I/O.
pub fn decide(peer: &Peer, is_shell: bool, policy: &Policy) -> Verdict {
    if peer.uid != Some(policy.uid) {
        return Verdict::Denied("another user");
    }
    if peer.root != Some(policy.root) {
        return Verdict::Denied("sandboxed (another root directory)");
    }
    if is_shell {
        return Verdict::Shell;
    }
    match peer.exe {
        Some(e) if policy.exes.contains(&e) => Verdict::OwnBinary,
        Some(_) => Verdict::Denied("not a desktop-themes-rs binary"),
        None => Verdict::Denied("unknown executable"),
    }
}

/// The pid a pidfd refers to, from `/proc/self/fdinfo` (`Pid: -1` once the process has exited).
fn pidfd_pid(fd: RawFd) -> Option<u32> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}")).ok()?;
    parse_fdinfo_pid(&info)
}

fn parse_fdinfo_pid(info: &str) -> Option<u32> {
    let v = info.lines().find_map(|l| l.strip_prefix("Pid:"))?;
    let pid: i64 = v.trim().parse().ok()?;
    u32::try_from(pid).ok().filter(|p| *p > 0)
}

/// Executable and root directory of `pid` (as seen from `proc`, normally `/proc`).
fn probe(proc: &Path, pid: u32, uid: Option<u32>, pidfd: Option<RawFd>) -> Peer {
    let same = || pidfd.is_none_or(|fd| pidfd_pid(fd) == Some(pid));
    if !same() {
        return Peer::default();
    }
    let dir = proc.join(pid.to_string());
    let peer = Peer {
        uid,
        exe: FileId::of(&dir.join("exe")),
        root: FileId::of(&dir.join("root")),
    };
    if !same() {
        return Peer::default();
    }
    peer
}

/// The caller check (one per deskd, shared by the interfaces).
pub struct Trust {
    /// Trust deskd's own executable.
    own_exe: bool,
    /// Further binaries we trust (resolved at each check: an install replaces the files).
    binaries: Vec<PathBuf>,
    logged: Mutex<HashSet<String>>,
}

impl Trust {
    /// deskd itself and the `themes` binary installed next to it.
    pub fn installed() -> Trust {
        let sibling = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("themes")));
        Trust {
            own_exe: true,
            binaries: sibling.into_iter().collect(),
            logged: Mutex::default(),
        }
    }

    /// Only the shell owner and `binaries` (tests: the test binary itself is not trusted).
    #[cfg(test)]
    pub fn with_binaries(binaries: Vec<PathBuf>) -> Trust {
        Trust {
            own_exe: false,
            binaries,
            logged: Mutex::default(),
        }
    }

    fn policy(&self) -> Option<Policy> {
        let me = Path::new("/proc/self");
        let uid = std::fs::metadata(me).ok()?.uid();
        let root = FileId::of(&me.join("root"))?;
        let mut exes: Vec<FileId> = self.binaries.iter().filter_map(|p| FileId::of(p)).collect();
        if self.own_exe {
            exes.extend(FileId::of(&me.join("exe")));
        }
        Some(Policy { uid, root, exes })
    }

    /// Ok if the sender of `hdr` may use content methods; `AccessDenied` otherwise.
    pub async fn check(&self, conn: &zbus::Connection, hdr: &Header<'_>) -> zbus::fdo::Result<()> {
        let member = hdr.member().map_or("?", |m| m.as_str());
        let Some(sender) = hdr.sender() else {
            return Err(denied(member, "no sender"));
        };
        let dbus = zbus::fdo::DBusProxy::builder(conn)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?;
        let shell = zbus::names::BusName::try_from(SHELL_NAME)?;
        let is_shell = dbus
            .get_name_owner(shell)
            .await
            .is_ok_and(|o| o.as_str() == sender.as_str());
        let creds = dbus
            .get_connection_credentials(zbus::names::BusName::Unique(sender.clone()))
            .await?;
        let peer = match creds.process_id() {
            Some(pid) => probe(
                Path::new("/proc"),
                pid,
                creds.unix_user_id(),
                creds.process_fd().map(|f| f.as_fd().as_raw_fd()),
            ),
            None => Peer::default(),
        };
        let verdict = match self.policy() {
            Some(p) => decide(&peer, is_shell, &p),
            None => Verdict::Denied("cannot read deskd's own process data"),
        };
        match verdict {
            Verdict::Denied(why) => {
                self.log_once(sender.as_str(), member, creds.process_id(), why);
                Err(denied(member, why))
            }
            _ => Ok(()),
        }
    }

    fn log_once(&self, sender: &str, member: &str, pid: Option<u32>, why: &str) {
        let mut l = self.logged.lock().unwrap_or_else(|p| p.into_inner());
        if l.len() < MAX_LOGGED && l.insert(sender.to_string()) {
            let pid = pid.map_or_else(|| "?".to_string(), |p| p.to_string());
            eprintln!("deskd: refused {member} from {sender} (pid {pid}): {why}");
        }
    }
}

fn denied(member: &str, why: &str) -> zbus::fdo::Error {
    zbus::fdo::Error::AccessDenied(format!(
        "{member}: only GNOME Shell and desktop-themes-rs may call this ({why})"
    ))
}

/// What an interface holds: the check, or (tests over peer-to-peer connections, which have no
/// bus to ask) nothing.
#[derive(Clone)]
pub struct Gate(Option<Arc<Trust>>);

impl Gate {
    pub fn new(t: Arc<Trust>) -> Gate {
        Gate(Some(t))
    }

    #[cfg(test)]
    pub fn open() -> Gate {
        Gate(None)
    }

    pub async fn check(
        &self,
        conn: &zbus::Connection,
        hdr: Option<&Header<'_>>,
    ) -> zbus::fdo::Result<()> {
        match (&self.0, hdr) {
            (None, _) => Ok(()),
            (Some(t), Some(h)) => t.check(conn, h).await,
            (Some(_), None) => Err(denied("?", "no message header")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: FileId = FileId { dev: 1, ino: 2 };
    const DESKD: FileId = FileId { dev: 3, ino: 10 };
    const THEMES: FileId = FileId { dev: 3, ino: 11 };
    const OTHER: FileId = FileId { dev: 3, ino: 99 };

    fn policy() -> Policy {
        Policy {
            uid: 1000,
            root: ROOT,
            exes: vec![DESKD, THEMES],
        }
    }

    fn peer(uid: u32, exe: FileId, root: FileId) -> Peer {
        Peer {
            uid: Some(uid),
            exe: Some(exe),
            root: Some(root),
        }
    }

    #[test]
    fn decide_table() {
        let flatpak_root = FileId { dev: 40, ino: 1 };
        let cases: &[(&str, Peer, bool, Verdict)] = &[
            ("shell", peer(1000, OTHER, ROOT), true, Verdict::Shell),
            ("deskd", peer(1000, DESKD, ROOT), false, Verdict::OwnBinary),
            (
                "themes",
                peer(1000, THEMES, ROOT),
                false,
                Verdict::OwnBinary,
            ),
            (
                "busctl",
                peer(1000, OTHER, ROOT),
                false,
                Verdict::Denied("not a desktop-themes-rs binary"),
            ),
            (
                "other user, even as shell",
                peer(1001, OTHER, ROOT),
                true,
                Verdict::Denied("another user"),
            ),
            (
                "flatpak app owning org.gnome.Shell",
                peer(1000, OTHER, flatpak_root),
                true,
                Verdict::Denied("sandboxed (another root directory)"),
            ),
            (
                "our binary run inside a sandbox",
                peer(1000, DESKD, flatpak_root),
                false,
                Verdict::Denied("sandboxed (another root directory)"),
            ),
            (
                "process gone",
                Peer::default(),
                true,
                Verdict::Denied("another user"),
            ),
            (
                "exe unreadable",
                Peer {
                    exe: None,
                    ..peer(1000, OTHER, ROOT)
                },
                false,
                Verdict::Denied("unknown executable"),
            ),
        ];
        for (name, p, shell, want) in cases {
            assert_eq!(&decide(p, *shell, &policy()), want, "{name}");
        }
    }

    #[test]
    fn fdinfo_pid() {
        assert_eq!(
            parse_fdinfo_pid("pos:\t0\nflags:\t02000002\nPid:\t4242\nNSpid:\t4242\n"),
            Some(4242)
        );
        assert_eq!(parse_fdinfo_pid("Pid:\t-1\n"), None, "exited");
        assert_eq!(parse_fdinfo_pid("pos:\t0\n"), None);
    }

    /// The probe against a fake /proc: the executable is compared by file, so a link to our
    /// binary counts, and a missing process gives an unknown peer.
    #[test]
    fn probe_follows_links() {
        let d = std::env::temp_dir().join(format!("deskd-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("proc/77");
        std::fs::create_dir_all(&p).unwrap();
        let bin = d.join("deskd");
        std::fs::write(&bin, b"x").unwrap();
        std::os::unix::fs::symlink(&bin, p.join("exe")).unwrap();
        std::os::unix::fs::symlink("/", p.join("root")).unwrap();
        let got = probe(&d.join("proc"), 77, Some(1000), None);
        assert_eq!(got.exe, FileId::of(&bin));
        assert_eq!(got.root, FileId::of(Path::new("/")));
        assert_eq!(got.uid, Some(1000));
        let gone = probe(&d.join("proc"), 78, Some(1000), None);
        assert_eq!((gone.exe, gone.root), (None, None));
        let _ = std::fs::remove_dir_all(d);
    }

    /// deskd's own policy: its executable and root resolve (the test binary stands in for deskd).
    #[test]
    fn own_policy_resolves() {
        let p = Trust::installed().policy().unwrap();
        assert!(
            p.exes
                .contains(&FileId::of(Path::new("/proc/self/exe")).unwrap())
        );
        let me = probe(Path::new("/proc"), std::process::id(), Some(p.uid), None);
        assert_eq!(decide(&me, false, &p), Verdict::OwnBinary);
        assert_eq!(
            decide(&me, false, &Trust::with_binaries(vec![]).policy().unwrap()),
            Verdict::Denied("not a desktop-themes-rs binary")
        );
    }
}
