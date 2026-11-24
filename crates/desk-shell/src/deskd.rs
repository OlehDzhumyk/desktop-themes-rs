// SPDX-License-Identifier: GPL-3.0-or-later
//! Calls to `deskd` on the session bus (GIO, asynchronous, never blocking the shell).
//!
//! deskd owns the logic and the storage (ADR 0002); this shell only forwards. When deskd is not
//! running, every call logs one line and the shell keeps working. Calls go only to an owner of
//! deskd's name that passes the reverse check of ADR 0008 (see [`untrusted_reason`]).

use std::cell::RefCell;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use desk_proto as proto;
use desk_search::{ResultKind, SearchResult};
use gtk::gio;
use gtk::glib::{self, Variant};
use gtk::prelude::*;

/// One clipboard history item, as `List` returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipRow {
    pub id: u64,
    pub kind: String,
    pub text: String,
    pub pinned: bool,
}

/// One audio device, as `Mixer.ListDevices` returns it.
#[derive(Clone, Debug, PartialEq)]
pub struct MixerDevice {
    pub id: u32,
    pub description: String,
    pub icon: String,
    pub volume: f64,
    pub muted: bool,
    pub is_default: bool,
}

/// One application that plays sound, as `Mixer.ListApps` returns it.
#[derive(Clone, Debug, PartialEq)]
pub struct MixerApp {
    pub key: String,
    pub name: String,
    pub icon: String,
    pub volume: f64,
    pub muted: bool,
}

type DeviceItem = (u32, String, String, String, String, f64, bool, bool);
type AppItem = (
    String,
    String,
    String,
    String,
    f64,
    bool,
    bool,
    u32,
    Vec<u32>,
);

type ListItem = (u64, String, String, String, String, bool, i64, u64);

/// The rows of a `List` answer (`(a(tssssbxt))`); `None` for another type.
pub fn clip_rows(reply: &Variant) -> Option<Vec<ClipRow>> {
    let (items,) = reply.get::<(Vec<ListItem>,)>()?;
    Some(
        items
            .into_iter()
            .map(|(id, kind, text, _, _, pinned, ..)| ClipRow {
                id,
                kind,
                text,
                pinned,
            })
            .collect(),
    )
}

/// The devices of a `ListDevices` answer (`(a(ussssdbb))`); `None` for another type.
pub fn mixer_devices(reply: &Variant) -> Option<Vec<MixerDevice>> {
    let (items,) = reply.get::<(Vec<DeviceItem>,)>()?;
    Some(
        items
            .into_iter()
            .map(
                |(id, _, description, icon, _, volume, muted, is_default)| MixerDevice {
                    id,
                    description,
                    icon,
                    volume,
                    muted,
                    is_default,
                },
            )
            .collect(),
    )
}

/// The applications of a `ListApps` answer (`(a(ssssdbbuau))`); `None` for another type.
pub fn mixer_apps(reply: &Variant) -> Option<Vec<MixerApp>> {
    let (items,) = reply.get::<(Vec<AppItem>,)>()?;
    Some(
        items
            .into_iter()
            .map(|(key, name, icon, _, volume, muted, ..)| MixerApp {
                key,
                name,
                icon,
                volume,
                muted,
            })
            .collect(),
    )
}

/// The default output: the device marked default, else the first one.
pub fn default_device(devices: &[MixerDevice]) -> Option<&MixerDevice> {
    devices
        .iter()
        .find(|d| d.is_default)
        .or_else(|| devices.first())
}

/// A slider volume (cubic, 1.0 = 100 %) as whole percent, never negative.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is finite and in 0.0..=150.0 before the cast"
)]
pub fn volume_percent(volume: f64) -> u32 {
    if volume.is_finite() && volume > 0.0 {
        (volume.min(1.5) * 100.0).round() as u32
    } else {
        0
    }
}

/// The symbolic icon for a volume.
pub fn volume_icon(volume: f64, muted: bool) -> &'static str {
    match volume_percent(volume) {
        _ if muted => "audio-volume-muted-symbolic",
        0 => "audio-volume-muted-symbolic",
        1..=33 => "audio-volume-low-symbolic",
        34..=66 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

/// A focus mode is active in a `Focus.Status` answer (its first value is not empty).
pub fn focus_active(reply: &Variant) -> Option<bool> {
    if !reply.is_container() || reply.n_children() == 0 {
        return None;
    }
    Some(!reply.child_value(0).str()?.is_empty())
}

/// The scheme of an `Appearance.Status` answer is dark.
pub fn scheme_is_dark(reply: &Variant) -> Option<bool> {
    if !reply.is_container() || reply.n_children() < 2 {
        return None;
    }
    Some(reply.child_value(1).str()? == "dark")
}

/// The boolean in a `Properties.Get` answer (`(v)`).
pub fn property_bool(reply: &Variant) -> Option<bool> {
    if !reply.is_container() || reply.n_children() == 0 {
        return None;
    }
    let value = reply.child_value(0);
    if !value.is_type(glib::VariantTy::VARIANT) {
        return None;
    }
    value.as_variant()?.get::<bool>()
}

/// One emoji, symbol or text face, as `Emoji.Search`, `Category` and `Recents` return it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmojiRow {
    pub id: String,
    pub glyph: String,
    pub name: String,
}

type EmojiItem = (String, String, String, String, String, bool);

/// The items of an `Emoji` list answer (`(a(sssssb))`); `None` for another type.
pub fn emoji_rows(reply: &Variant) -> Option<Vec<EmojiRow>> {
    let (items,) = reply.get::<(Vec<EmojiItem>,)>()?;
    Some(
        items
            .into_iter()
            .map(|(id, glyph, name, ..)| EmojiRow { id, glyph, name })
            .collect(),
    )
}

/// The one string of an answer such as `Emoji.Pick` or `Capture.SaveScreenshot` (`(s)`).
pub fn single_string(reply: &Variant) -> Option<String> {
    reply.get::<(String,)>().map(|(s,)| s)
}

/// What `Ocr.Recognize` found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OcrText {
    pub text: String,
    pub preview: String,
    pub missing_langs: Vec<String>,
}

/// The answer of `Ocr.Recognize` (`(ssas)`); `None` for another type.
pub fn ocr_text(reply: &Variant) -> Option<OcrText> {
    let (text, preview, missing_langs) = reply.get::<(String, String, Vec<String>)>()?;
    Some(OcrText {
        text,
        preview,
        missing_langs,
    })
}

/// A recorded colour, as `ColorPicker.Record` returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorRecord {
    pub formatted: String,
    pub hex: String,
    pub name: String,
}

/// The answer of `ColorPicker.Record` (`(sss)`); `None` for another type.
pub fn color_record(reply: &Variant) -> Option<ColorRecord> {
    let (formatted, hex, name) = reply.get::<(String, String, String)>()?;
    Some(ColorRecord {
        formatted,
        hex,
        name,
    })
}

/// One line of at most `max` characters for a menu: whitespace runs collapsed, `…` when cut.
pub fn preview(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

// ---------------------------------------------------------------------------------------------
// ADR 0008 in reverse: is the owner of io.github.OlehDzhumyk.Desk the deskd that is installed?
//
// A process that owned the name would receive what this shell sends (clipboard pins, search
// queries) and decide what it shows and puts on the clipboard. Calls therefore go to the unique
// name of an owner that passed these checks (as `bridge/src/trust.ts` does for the bridge):
//   - the same user (`UnixUserID` of `GetConnectionCredentials`);
//   - the same root directory as this shell (`/proc/<pid>/root`): sandboxed apps have their own;
//   - its executable is the same file (device and inode, so links count) as the `deskd` next to
//     `desk-shell` or `/usr/bin/deskd`.
// The connection and the verified name are cached; `NameOwnerChanged` for the name, or an error
// that says the owner is gone, drops the name and the next call checks the new owner.
// ---------------------------------------------------------------------------------------------

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

/// What the bus and `/proc` say about the owner. `None`: unknown (and then not trusted).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Peer {
    pub uid: Option<u32>,
    pub exe: Option<FileId>,
    pub root: Option<FileId>,
}

/// What a trusted owner must match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub uid: u32,
    pub root: FileId,
    pub exes: Vec<FileId>,
}

/// Why `peer` is not our deskd, or `None` when it is.
pub fn untrusted_reason(peer: &Peer, policy: &Policy) -> Option<&'static str> {
    if peer.uid != Some(policy.uid) {
        return Some("another user");
    }
    if peer.root != Some(policy.root) {
        return Some("sandboxed (another root directory)");
    }
    match peer.exe {
        Some(e) if policy.exes.contains(&e) => None,
        Some(_) => Some("not the installed deskd executable"),
        None => Some("unknown executable"),
    }
}

/// The deskd executables this shell trusts: the one next to `self_exe`, then `/usr/bin/deskd`.
pub fn deskd_paths(self_exe: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = self_exe
        .and_then(Path::parent)
        .map(|d| d.join("deskd"))
        .into_iter()
        .collect();
    let system = PathBuf::from("/usr/bin/deskd");
    if !out.contains(&system) {
        out.push(system);
    }
    out
}

/// The policy now (resolved per check: an install replaces the binaries).
fn policy() -> Option<Policy> {
    let me = Path::new("/proc/self");
    let uid = std::fs::metadata(me).ok()?.uid();
    let root = FileId::of(&me.join("root"))?;
    let exe = std::env::current_exe().ok();
    let exes = deskd_paths(exe.as_deref())
        .iter()
        .filter_map(|p| FileId::of(p))
        .collect();
    Some(Policy { uid, root, exes })
}

/// The pid in a `/proc/self/fdinfo/<pidfd>` text (`Pid: -1` once the process has exited).
pub fn parse_fdinfo_pid(info: &str) -> Option<u32> {
    let v = info.lines().find_map(|l| l.strip_prefix("Pid:"))?;
    let pid: i64 = v.trim().parse().ok()?;
    u32::try_from(pid).ok().filter(|p| *p > 0)
}

fn pidfd_pid(fd: RawFd) -> Option<u32> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}")).ok()?;
    parse_fdinfo_pid(&info)
}

/// Executable and root directory of `pid` under `proc`; with a pidfd, the pid must still be
/// that process after `/proc` was read (a reused pid cannot lend another identity).
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

/// `(UnixUserID, ProcessID, ProcessFD handle index)` of a `GetConnectionCredentials` answer.
pub fn credentials(reply: &Variant) -> (Option<u32>, Option<u32>, Option<i32>) {
    if !reply.is_container() || reply.n_children() == 0 {
        return (None, None, None);
    }
    let dict = glib::VariantDict::new(Some(&reply.child_value(0)));
    let uid = dict.lookup::<u32>("UnixUserID").ok().flatten();
    let pid = dict.lookup::<u32>("ProcessID").ok().flatten();
    let fd = dict
        .lookup_value("ProcessFD", Some(glib::VariantTy::HANDLE))
        .and_then(|v| v.get::<glib::variant::Handle>())
        .map(|h| h.0);
    (uid, pid, fd)
}

const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
/// Every call to deskd and the bus waits at most this long (ms).
const TIMEOUT_MS: i32 = 5000;
/// After a failed check, calls fail at once for this long (µs), unless the owner changes.
const RETRY_USEC: i64 = 5_000_000;

/// The cached session bus connection and what is known about deskd on it.
struct Link {
    bus: gio::DBusConnection,
    /// The unique name of the verified deskd.
    owner: Option<String>,
    /// When the last check failed (monotonic µs).
    failed_at: Option<i64>,
    /// The last refused owner (logged once).
    refused: Option<String>,
    /// `NameOwnerChanged` and the signal watches, alive as long as the link.
    _subs: Vec<gio::SignalSubscription>,
}

thread_local! {
    static LINK: RefCell<Option<Link>> = const { RefCell::new(None) };
}

/// Forget the verified owner (it left, or it failed a call).
fn invalidate() {
    LINK.with_borrow_mut(|l| {
        if let Some(l) = l {
            l.owner = None;
            l.failed_at = None;
        }
    });
}

/// The error says the owner is gone or the bus is broken: check again next time.
fn owner_gone(e: &glib::Error) -> bool {
    use gio::DBusError as D;
    [
        D::ServiceUnknown,
        D::NameHasNoOwner,
        D::NoReply,
        D::Disconnected,
        D::UnknownObject,
        D::UnknownInterface,
        D::UnknownMethod,
    ]
    .into_iter()
    .any(|d| e.matches(d))
        || e.matches(gio::IOErrorEnum::Closed)
}

/// The session bus (cached); `None` when there is none.
fn with_bus(done: impl FnOnce(Option<gio::DBusConnection>) + 'static) {
    let cached = LINK.with_borrow(|l| {
        l.as_ref()
            .filter(|l| !l.bus.is_closed())
            .map(|l| l.bus.clone())
    });
    if let Some(bus) = cached {
        return done(Some(bus));
    }
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, move |bus| {
        let bus = match bus {
            Ok(b) => b,
            Err(e) => {
                eprintln!("desk-shell: no session bus: {e}");
                return done(None);
            }
        };
        // two calls can race here before the first link exists: keep the first (and its watches)
        if LINK.with_borrow(|l| {
            l.as_ref()
                .is_some_and(|l| l.bus == bus && !l.bus.is_closed())
        }) {
            return done(Some(bus));
        }
        let watch = bus.subscribe_to_signal(
            Some(DBUS_NAME),
            Some(DBUS_NAME),
            Some("NameOwnerChanged"),
            Some(DBUS_PATH),
            Some(proto::BUS_NAME),
            gio::DBusSignalFlags::NONE,
            |_| invalidate(),
        );
        LINK.with_borrow_mut(|l| {
            *l = Some(Link {
                bus: bus.clone(),
                owner: None,
                failed_at: None,
                refused: None,
                _subs: vec![watch],
            });
        });
        done(Some(bus));
    });
}

fn bus_call(
    bus: &gio::DBusConnection,
    method: &'static str,
    args: Variant,
    done: impl FnOnce(Option<Variant>) + 'static,
) {
    bus.call(
        Some(DBUS_NAME),
        DBUS_PATH,
        DBUS_NAME,
        method,
        Some(&args),
        None,
        gio::DBusCallFlags::NONE,
        TIMEOUT_MS,
        gio::Cancellable::NONE,
        move |r| done(r.ok()),
    );
}

/// The unique name owning deskd's name; starts deskd (D-Bus activation) when nobody owns it.
fn name_owner(bus: &gio::DBusConnection, done: impl FnOnce(Option<String>) + 'static) {
    let get = |bus: &gio::DBusConnection, done: Box<dyn FnOnce(Option<String>)>| {
        bus_call(
            bus,
            "GetNameOwner",
            (proto::BUS_NAME,).to_variant(),
            move |r| done(r.and_then(|v| v.get::<(String,)>()).map(|(o,)| o)),
        );
    };
    let again = bus.clone();
    get(
        bus,
        Box::new(move |owner| {
            if owner.is_some() {
                return done(owner);
            }
            let bus = again.clone();
            bus_call(
                &again,
                "StartServiceByName",
                (proto::BUS_NAME, 0_u32).to_variant(),
                move |started| {
                    if started.is_none() {
                        return done(None);
                    }
                    get(&bus, Box::new(done));
                },
            );
        }),
    );
}

/// Check `owner` and cache it when it is our deskd.
fn verify(
    bus: gio::DBusConnection,
    owner: String,
    done: impl FnOnce(Option<(gio::DBusConnection, String)>) + 'static,
) {
    bus.clone().call_with_unix_fd_list(
        Some(DBUS_NAME),
        DBUS_PATH,
        DBUS_NAME,
        "GetConnectionCredentials",
        Some(&(owner.as_str(),).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        TIMEOUT_MS,
        gio::UnixFDList::NONE,
        gio::Cancellable::NONE,
        move |reply| {
            let peer = match &reply {
                Ok((v, fds)) => {
                    let (uid, pid, fd_index) = credentials(v);
                    let pidfd = fd_index.and_then(|i| fds.as_ref()?.get(i).ok());
                    pid.map_or_else(Peer::default, |pid| {
                        probe(
                            Path::new("/proc"),
                            pid,
                            uid,
                            pidfd.as_ref().map(AsRawFd::as_raw_fd),
                        )
                    })
                }
                Err(e) => {
                    eprintln!("desk-shell: credentials of deskd ({owner}): {e}");
                    Peer::default()
                }
            };
            let why = match policy() {
                Some(p) => untrusted_reason(&peer, &p),
                None => Some("cannot read this shell's own process data"),
            };
            let trusted = why.is_none();
            LINK.with_borrow_mut(|l| {
                let Some(l) = l else { return };
                if let Some(why) = why {
                    if l.refused.as_deref() != Some(owner.as_str()) {
                        eprintln!(
                            "desk-shell: not using {owner} as deskd: {why} (ADR 0008); \
                             install deskd next to desk-shell"
                        );
                        l.refused = Some(owner.clone());
                    }
                    l.owner = None;
                    l.failed_at = Some(glib::monotonic_time());
                } else {
                    l.owner = Some(owner.clone());
                    l.failed_at = None;
                }
            });
            done(trusted.then_some((bus, owner)));
        },
    );
}

/// The bus and the verified unique name of deskd; `None` when deskd is missing or refused.
fn with_owner(done: impl FnOnce(Option<(gio::DBusConnection, String)>) + 'static) {
    with_bus(move |bus| {
        let Some(bus) = bus else { return done(None) };
        let (owner, failed_at) = LINK.with_borrow(|l| {
            l.as_ref()
                .map_or((None, None), |l| (l.owner.clone(), l.failed_at))
        });
        if let Some(owner) = owner {
            return done(Some((bus, owner)));
        }
        if failed_at.is_some_and(|t| glib::monotonic_time() - t < RETRY_USEC) {
            return done(None);
        }
        let b = bus.clone();
        name_owner(&bus, move |owner| match owner {
            Some(owner) => verify(b, owner, done),
            None => {
                LINK.with_borrow_mut(|l| {
                    if let Some(l) = l {
                        l.failed_at = Some(glib::monotonic_time());
                    }
                });
                eprintln!("desk-shell: deskd is not running");
                done(None);
            }
        });
    });
}

/// Run `on_signal` for each `member` signal of deskd's `iface` at `path` (no content: the
/// handler asks the verified owner for the new state).
pub fn watch_signal(
    path: &'static str,
    iface: &'static str,
    member: &'static str,
    on_signal: impl Fn() + 'static,
) {
    with_bus(move |bus| {
        let Some(bus) = bus else { return };
        let sub = bus.subscribe_to_signal(
            Some(proto::BUS_NAME),
            Some(iface),
            Some(member),
            Some(path),
            None,
            gio::DBusSignalFlags::NONE,
            move |_| on_signal(),
        );
        LINK.with_borrow_mut(|l| {
            if let Some(l) = l {
                l._subs.push(sub);
            }
        });
    });
}

/// Why a call to deskd gave no answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// No session bus, deskd is not running, or its owner failed the ADR 0008 check.
    Unavailable,
    /// deskd answered with an error: its D-Bus name (when it has one) and its message.
    Remote {
        name: Option<String>,
        message: String,
    },
}

/// The [`CallError`] of a failed call.
fn call_error(e: &glib::Error) -> CallError {
    if owner_gone(e) {
        return CallError::Unavailable;
    }
    let mut stripped = e.clone();
    gio::DBusError::strip_remote_error(&mut stripped);
    CallError::Remote {
        name: gio::DBusError::remote_error(e).map(|n| n.to_string()),
        message: stripped.message().to_owned(),
    }
}

/// Call deskd with a `timeout_ms`; `done` gets the answer or why there is none (logged).
fn call_full(
    path: &'static str,
    iface: &'static str,
    method: &'static str,
    args: Option<Variant>,
    timeout_ms: i32,
    done: impl FnOnce(Result<Variant, CallError>) + 'static,
) {
    with_owner(move |link| {
        let Some((bus, owner)) = link else {
            return done(Err(CallError::Unavailable));
        };
        bus.call(
            Some(&owner),
            path,
            iface,
            method,
            args.as_ref(),
            None,
            gio::DBusCallFlags::NONE,
            timeout_ms,
            gio::Cancellable::NONE,
            move |reply| match reply {
                Ok(v) => done(Ok(v)),
                Err(e) => {
                    if owner_gone(&e) {
                        invalidate();
                    }
                    eprintln!("desk-shell: deskd {iface}.{method}: {e}");
                    done(Err(call_error(&e)));
                }
            },
        );
    });
}

/// Call deskd; `done` gets the answer, or `None` (logged) when deskd or the bus is missing.
fn try_call(
    path: &'static str,
    iface: &'static str,
    method: &'static str,
    args: Option<Variant>,
    done: impl FnOnce(Option<Variant>) + 'static,
) {
    call_full(path, iface, method, args, TIMEOUT_MS, |r| done(r.ok()));
}

/// Call deskd and wait for the answer, running the default main context meanwhile: for the
/// one-shot commands (`--capture-region`, ...), which have no GTK main loop. The checks of
/// ADR 0008 apply as for every other call.
pub fn call_blocking(
    path: &'static str,
    iface: &'static str,
    method: &'static str,
    args: Option<Variant>,
    timeout_ms: i32,
) -> Result<Variant, CallError> {
    let main_loop = glib::MainLoop::new(None, false);
    let slot: std::rc::Rc<RefCell<Option<Result<Variant, CallError>>>> = std::rc::Rc::default();
    let (ml, out) = (main_loop.clone(), std::rc::Rc::clone(&slot));
    call_full(path, iface, method, args, timeout_ms, move |r| {
        *out.borrow_mut() = Some(r);
        ml.quit();
    });
    // the answer may have come already (a refused owner answers at once): then do not wait
    if slot.borrow().is_none() {
        main_loop.run();
    }
    slot.borrow_mut()
        .take()
        .unwrap_or(Err(CallError::Unavailable))
}

/// Call deskd; `done` runs only on success.
fn call(
    path: &'static str,
    iface: &'static str,
    method: &'static str,
    args: Option<Variant>,
    done: impl FnOnce(Variant) + 'static,
) {
    try_call(path, iface, method, args, |reply| {
        if let Some(v) = reply {
            done(v);
        }
    });
}

/// Count a launch in the search usage (`Search.RecordLaunch`).
pub fn record_launch(item_id: &str) {
    call(
        proto::SEARCH_PATH,
        proto::SEARCH_IFACE,
        "RecordLaunch",
        Some((item_id,).to_variant()),
        |_| {},
    );
}

const PROPS: &str = "org.freedesktop.DBus.Properties";

/// The kind of a result in a `Search.Query` answer.
pub fn result_kind(kind: &str) -> Option<ResultKind> {
    Some(match kind {
        "calculator" => ResultKind::Calculator,
        "app" => ResultKind::App,
        "action" => ResultKind::Action,
        "file" => ResultKind::File,
        "clipboard" => ResultKind::Clipboard,
        "emoji" => ResultKind::Emoji,
        "web" => ResultKind::Web,
        _ => return None,
    })
}

/// The results of a `Search.Query` answer (a JSON list); `None` when it is not one. Items of an
/// unknown kind (a newer deskd) are left out.
pub fn search_results(json: &str) -> Option<Vec<SearchResult>> {
    let items: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    let text = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Some(
        items
            .iter()
            .filter_map(|v| {
                Some(SearchResult {
                    id: text(v, "id"),
                    kind: result_kind(v.get("kind")?.as_str()?)?,
                    title: text(v, "title"),
                    subtitle: text(v, "subtitle"),
                    icon: text(v, "icon"),
                    activate: text(v, "activate"),
                    score: v
                        .get("score")
                        .and_then(serde_json::Value::as_u64)
                        .map_or(0, |s| u32::try_from(s).unwrap_or(u32::MAX)),
                })
            })
            .collect(),
    )
}

/// Ask deskd's search engine (`Search.Query`: applications, actions, calculator, files,
/// clipboard, emoji, usage ranking). `None`: deskd is missing, refused or answered no list;
/// the caller then searches in process.
pub fn search(text: &str, done: impl FnOnce(Option<Vec<SearchResult>>) + 'static) {
    try_call(
        proto::SEARCH_PATH,
        proto::SEARCH_IFACE,
        "Query",
        // 0: deskd's max-results setting
        Some((text, 0_u32).to_variant()),
        move |reply| {
            done(
                reply
                    .and_then(|r| r.get::<(String,)>())
                    .and_then(|(json,)| search_results(&json)),
            );
        },
    );
}

/// Run `changed` whenever deskd's appearance (scheme, schedule, accent) changes.
pub fn watch_appearance(changed: impl Fn() + 'static) {
    watch_signal(
        proto::APPEARANCE_PATH,
        proto::APPEARANCE_INTERFACE,
        "Changed",
        changed,
    );
}

/// Whether the scheme is dark now (`None`: deskd is not running).
pub fn dark_mode(done: impl FnOnce(Option<bool>) + 'static) {
    try_call(
        proto::APPEARANCE_PATH,
        proto::APPEARANCE_INTERFACE,
        "Status",
        None,
        |reply| done(reply.as_ref().and_then(scheme_is_dark)),
    );
}

/// Set the scheme to dark or light (`Appearance.SetScheme`).
pub fn set_dark_mode(dark: bool) {
    call(
        proto::APPEARANCE_PATH,
        proto::APPEARANCE_INTERFACE,
        "SetScheme",
        Some((if dark { "dark" } else { "light" },).to_variant()),
        |_| {},
    );
}

/// Switch light ↔ dark.
pub fn toggle_dark_mode() {
    dark_mode(|dark| {
        if let Some(dark) = dark {
            set_dark_mode(!dark);
        }
    });
}

/// Whether a focus mode is active (`None`: deskd is not running).
pub fn focus_status(done: impl FnOnce(Option<bool>) + 'static) {
    try_call(
        proto::FOCUS_PATH,
        proto::FOCUS_INTERFACE,
        "Status",
        None,
        |reply| done(reply.as_ref().and_then(focus_active)),
    );
}

/// Start do not disturb until turned off, or end the active focus mode.
pub fn set_focus(on: bool) {
    let (method, args) = if on {
        ("Start", Some(("dnd", 0_i64).to_variant()))
    } else {
        ("Stop", None)
    };
    call(
        proto::FOCUS_PATH,
        proto::FOCUS_INTERFACE,
        method,
        args,
        |_| {},
    );
}

/// Whether the clipboard private mode is on (`None`: deskd is not running).
pub fn clipboard_private(done: impl FnOnce(Option<bool>) + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        PROPS,
        "Get",
        Some((proto::CLIPBOARD_IFACE, "Private").to_variant()),
        |reply| done(reply.as_ref().and_then(property_bool)),
    );
}

/// Turn the clipboard private mode on or off.
pub fn set_clipboard_private(on: bool) {
    call(
        proto::CLIPBOARD_PATH,
        PROPS,
        "Set",
        Some((proto::CLIPBOARD_IFACE, "Private", on.to_variant()).to_variant()),
        |_| {},
    );
}

/// Flip the clipboard `Private` property.
pub fn toggle_clipboard_private() {
    clipboard_private(|on| {
        if let Some(on) = on {
            set_clipboard_private(!on);
        }
    });
}

/// The most recent clipboard items (`None`: deskd is not running).
pub fn clipboard_list(limit: u32, done: impl FnOnce(Option<Vec<ClipRow>>) + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "List",
        Some(("", limit).to_variant()),
        move |reply| done(reply.as_ref().and_then(clip_rows)),
    );
}

/// Pin or unpin an item; `done` runs after the answer (also on failure).
pub fn clipboard_pin(id: u64, pinned: bool, done: impl FnOnce() + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Pin",
        Some((id, pinned).to_variant()),
        move |_| done(),
    );
}

/// Delete an item; `done` runs after the answer (also on failure).
pub fn clipboard_delete(id: u64, done: impl FnOnce() + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Delete",
        Some((id,).to_variant()),
        move |_| done(),
    );
}

/// Delete every item that is not pinned; `done` runs after the answer (also on failure).
pub fn clipboard_clear(done: impl FnOnce() + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Clear",
        Some((true,).to_variant()),
        move |_| done(),
    );
}

/// Make an item the most recent one and put its content on the clipboard of this session;
/// `done` runs after that (also when deskd does not answer).
pub fn clipboard_activate(id: u64, done: impl FnOnce() + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Activate",
        Some((id,).to_variant()),
        move |reply| {
            if let Some((mime, data)) = reply.and_then(|r| r.get::<(String, Vec<u8>)>())
                && let Some(display) = gtk::gdk::Display::default()
            {
                let clipboard = display.clipboard();
                if mime.starts_with("text/") {
                    clipboard.set_text(&String::from_utf8_lossy(&data));
                } else {
                    match gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(data)) {
                        Ok(texture) => clipboard.set_texture(&texture),
                        Err(e) => eprintln!("desk-shell: clipboard item {id}: {e}"),
                    }
                }
            }
            done();
        },
    );
}

/// The output devices (`None`: deskd is not running).
pub fn mixer_outputs(done: impl FnOnce(Option<Vec<MixerDevice>>) + 'static) {
    try_call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "ListDevices",
        Some(("output",).to_variant()),
        move |reply| done(reply.as_ref().and_then(mixer_devices)),
    );
}

/// The applications that play sound now (`None`: deskd is not running).
pub fn mixer_playing_apps(done: impl FnOnce(Option<Vec<MixerApp>>) + 'static) {
    try_call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "ListApps",
        Some((false,).to_variant()),
        move |reply| done(reply.as_ref().and_then(mixer_apps)),
    );
}

/// Volume of a device (`Mixer.SetVolume`).
pub fn set_device_volume(id: u32, volume: f64) {
    let args = (id, volume).to_variant();
    call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "SetVolume",
        Some(args),
        |_| {},
    );
}

/// Mute of a device (`Mixer.SetMute`).
pub fn set_device_mute(id: u32, mute: bool) {
    let args = (id, mute).to_variant();
    call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "SetMute",
        Some(args),
        |_| {},
    );
}

/// Volume of every stream of an application (`Mixer.SetAppVolume`).
pub fn set_app_volume(key: &str, volume: f64) {
    let args = (key, volume).to_variant();
    call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "SetAppVolume",
        Some(args),
        |_| {},
    );
}

/// Mute of every stream of an application (`Mixer.SetAppMute`).
pub fn set_app_mute(key: &str, mute: bool) {
    let args = (key, mute).to_variant();
    call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "SetAppMute",
        Some(args),
        |_| {},
    );
}

/// Run `changed` on each `Mixer.Changed` of deskd (devices, applications, defaults).
pub fn watch_mixer(changed: impl Fn() + 'static) {
    watch_signal(proto::MIXER_PATH, proto::MIXER_IFACE, "Changed", changed);
}

fn emoji_list(
    method: &'static str,
    args: Option<Variant>,
    done: impl FnOnce(Option<Vec<EmojiRow>>) + 'static,
) {
    try_call(
        proto::EMOJI_PATH,
        proto::EMOJI_IFACE,
        method,
        args,
        move |r| {
            done(r.as_ref().and_then(emoji_rows));
        },
    );
}

/// Emoji that match `query` in English and Ukrainian, names in `lang` (`None`: deskd is not
/// running).
pub fn emoji_search(
    query: &str,
    lang: &str,
    limit: u32,
    done: impl FnOnce(Option<Vec<EmojiRow>>) + 'static,
) {
    emoji_list("Search", Some((query, lang, limit).to_variant()), done);
}

/// The emoji of a category (`smileys`, `people`, ...; limit 0 = all).
pub fn emoji_category(
    category: &str,
    limit: u32,
    done: impl FnOnce(Option<Vec<EmojiRow>>) + 'static,
) {
    emoji_list("Category", Some((category, limit).to_variant()), done);
}

/// The recently picked emoji, newest first.
pub fn emoji_recents(done: impl FnOnce(Option<Vec<EmojiRow>>) + 'static) {
    emoji_list("Recents", None, done);
}

/// Pick an emoji (deskd records it in the recents and applies the default skin tone); `done`
/// gets the text to insert (`None`: deskd did not answer).
pub fn emoji_pick(id: &str, done: impl FnOnce(Option<String>) + 'static) {
    try_call(
        proto::EMOJI_PATH,
        proto::EMOJI_IFACE,
        "Pick",
        Some((id, "").to_variant()),
        move |r| done(r.as_ref().and_then(single_string)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_replies_are_parsed() {
        let items: Vec<ListItem> = vec![
            (
                7,
                "text".into(),
                "hello".into(),
                "text/plain".into(),
                String::new(),
                false,
                1,
                5,
            ),
            (
                3,
                "image".into(),
                "PNG".into(),
                "image/png".into(),
                "/tmp/a.png".into(),
                true,
                2,
                9,
            ),
        ];
        let rows = clip_rows(&(items,).to_variant()).unwrap();
        assert_eq!(
            rows,
            [
                ClipRow {
                    id: 7,
                    kind: "text".into(),
                    text: "hello".into(),
                    pinned: false,
                },
                ClipRow {
                    id: 3,
                    kind: "image".into(),
                    text: "PNG".into(),
                    pinned: true,
                },
            ]
        );
        assert!(clip_rows(&("wrong",).to_variant()).is_none());
    }

    #[test]
    fn mixer_replies_are_parsed() {
        let devices: Vec<DeviceItem> = vec![
            (
                40,
                "alsa_output.usb".into(),
                "Headset".into(),
                "audio-headset".into(),
                "usb".into(),
                0.5,
                false,
                false,
            ),
            (
                41,
                "alsa_output.pci".into(),
                "Speakers".into(),
                "audio-speakers".into(),
                "pci".into(),
                0.8,
                true,
                true,
            ),
        ];
        let devices = mixer_devices(&(devices,).to_variant()).unwrap();
        assert_eq!(devices.len(), 2);
        let main = default_device(&devices).unwrap();
        assert_eq!((main.id, main.description.as_str()), (41, "Speakers"));
        assert!(main.muted);
        // without a default, the first device; without devices, none
        assert_eq!(default_device(&devices[..1]).unwrap().id, 40);
        assert!(default_device(&[]).is_none());
        assert!(mixer_devices(&("wrong",).to_variant()).is_none());

        let apps: Vec<AppItem> = vec![(
            "firefox".into(),
            "Firefox".into(),
            "firefox".into(),
            "Video".into(),
            0.25,
            false,
            true,
            41,
            vec![7, 8],
        )];
        assert_eq!(
            mixer_apps(&(apps,).to_variant()).unwrap(),
            [MixerApp {
                key: "firefox".into(),
                name: "Firefox".into(),
                icon: "firefox".into(),
                volume: 0.25,
                muted: false,
            }]
        );
        assert!(mixer_apps(&("wrong",).to_variant()).is_none());
    }

    #[test]
    fn volumes_show_as_percent_and_icons() {
        assert_eq!(volume_percent(0.5), 50);
        assert_eq!(volume_percent(1.0), 100);
        assert_eq!(volume_percent(0.666), 67);
        // out of range and broken values stay in 0..=150
        assert_eq!(volume_percent(9.0), 150);
        assert_eq!(volume_percent(-1.0), 0);
        assert_eq!(volume_percent(f64::NAN), 0);
        assert_eq!(volume_icon(0.0, false), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.9, true), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.2, false), "audio-volume-low-symbolic");
        assert_eq!(volume_icon(0.5, false), "audio-volume-medium-symbolic");
        assert_eq!(volume_icon(1.2, false), "audio-volume-high-symbolic");
    }

    #[test]
    fn status_replies_are_read() {
        let off = (
            "",
            "",
            "",
            "",
            0_i64,
            0_i64,
            "",
            0_i64,
            Vec::<String>::new(),
        )
            .to_variant();
        assert_eq!(focus_active(&off), Some(false));
        let on = (
            "dnd",
            "Do not disturb",
            "",
            "manual",
            0_i64,
            0_i64,
            "",
            0_i64,
            Vec::<String>::new(),
        )
            .to_variant();
        assert_eq!(focus_active(&on), Some(true));
        assert_eq!(focus_active(&(7_u32,).to_variant()), None);
        assert_eq!(focus_active(&7_u32.to_variant()), None);

        let dark = ("auto", "dark", "", "", false, false).to_variant();
        assert_eq!(scheme_is_dark(&dark), Some(true));
        let light = ("manual", "light", "", "", false, false).to_variant();
        assert_eq!(scheme_is_dark(&light), Some(false));
        assert_eq!(scheme_is_dark(&("x",).to_variant()), None);

        assert_eq!(
            property_bool(&(true.to_variant(),).to_variant()),
            Some(true)
        );
        assert_eq!(property_bool(&("s".to_variant(),).to_variant()), None);
        assert_eq!(property_bool(&(true,).to_variant()), None);
    }

    fn policy() -> Policy {
        Policy {
            uid: 1000,
            root: FileId { dev: 1, ino: 2 },
            exes: vec![FileId { dev: 3, ino: 4 }, FileId { dev: 3, ino: 5 }],
        }
    }

    #[test]
    fn only_the_installed_deskd_of_this_user_and_root_is_trusted() {
        let good = Peer {
            uid: Some(1000),
            exe: Some(FileId { dev: 3, ino: 5 }),
            root: Some(FileId { dev: 1, ino: 2 }),
        };
        assert_eq!(untrusted_reason(&good, &policy()), None);
        let cases = [
            (
                Peer {
                    uid: Some(0),
                    ..good.clone()
                },
                "another user",
            ),
            (
                Peer {
                    uid: None,
                    ..good.clone()
                },
                "another user",
            ),
            (
                Peer {
                    root: Some(FileId { dev: 9, ino: 2 }),
                    ..good.clone()
                },
                "sandboxed (another root directory)",
            ),
            (
                Peer {
                    root: None,
                    ..good.clone()
                },
                "sandboxed (another root directory)",
            ),
            (
                Peer {
                    exe: Some(FileId { dev: 3, ino: 6 }),
                    ..good.clone()
                },
                "not the installed deskd executable",
            ),
            (
                Peer {
                    exe: None,
                    ..good.clone()
                },
                "unknown executable",
            ),
        ];
        for (peer, why) in cases {
            assert_eq!(untrusted_reason(&peer, &policy()), Some(why), "{peer:?}");
        }
        // an unknown process is never trusted
        assert!(untrusted_reason(&Peer::default(), &policy()).is_some());
    }

    #[test]
    fn deskd_is_looked_for_next_to_the_shell_then_in_usr_bin() {
        assert_eq!(
            deskd_paths(Some(Path::new("/home/user/.local/bin/desk-shell"))),
            [
                PathBuf::from("/home/user/.local/bin/deskd"),
                PathBuf::from("/usr/bin/deskd")
            ]
        );
        assert_eq!(
            deskd_paths(Some(Path::new("/usr/bin/desk-shell"))),
            [PathBuf::from("/usr/bin/deskd")]
        );
        assert_eq!(deskd_paths(None), [PathBuf::from("/usr/bin/deskd")]);
    }

    #[test]
    fn pidfd_info_and_credentials_are_read() {
        assert_eq!(
            parse_fdinfo_pid("pos:\t0\nPid:\t4242\nNSpid:\t4242\n"),
            Some(4242)
        );
        assert_eq!(parse_fdinfo_pid("Pid:\t-1\n"), None);
        assert_eq!(parse_fdinfo_pid("Pid:\t0\n"), None);
        assert_eq!(parse_fdinfo_pid("flags: 02\n"), None);

        let dict = glib::VariantDict::new(None);
        dict.insert("UnixUserID", 1000_u32);
        dict.insert("ProcessID", 77_u32);
        dict.insert_value("ProcessFD", &glib::variant::Handle(0).to_variant());
        let reply = Variant::tuple_from_iter([dict.end()]);
        assert_eq!(credentials(&reply), (Some(1000), Some(77), Some(0)));
        let empty = Variant::tuple_from_iter([glib::VariantDict::new(None).end()]);
        assert_eq!(credentials(&empty), (None, None, None));
        assert_eq!(credentials(&7_u32.to_variant()), (None, None, None));
    }

    #[test]
    fn this_process_is_probed_like_a_peer() {
        // this test process: its own exe and root, as /proc shows them
        let pid = std::process::id();
        let peer = probe(Path::new("/proc"), pid, Some(1), None);
        if Path::new("/proc/self/exe").exists() {
            assert_eq!(peer.exe, FileId::of(Path::new("/proc/self/exe")));
            assert_eq!(peer.root, FileId::of(Path::new("/proc/self/root")));
        }
        // a pid that does not exist gives nothing
        let none = probe(Path::new("/nonexistent-proc"), pid, Some(1), None);
        assert_eq!((none.exe, none.root), (None, None));
    }

    #[test]
    fn search_answers_are_parsed_and_unknown_kinds_left_out() {
        let json = r#"[
            {"id":"app:org.gnome.Nautilus.desktop","kind":"app","title":"Files","subtitle":"",
             "icon":"org.gnome.Nautilus","activate":"org.gnome.Nautilus.desktop","score":900},
            {"id":"clipboard:7","kind":"clipboard","title":"hello","subtitle":"Clipboard",
             "icon":"edit-paste-symbolic","activate":"7","score":5000000000},
            {"id":"x","kind":"hologram","title":"?","activate":"","score":1}
        ]"#;
        let r = search_results(json).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].kind, ResultKind::App);
        assert_eq!(r[0].activate, "org.gnome.Nautilus.desktop");
        assert_eq!((r[1].kind, r[1].score), (ResultKind::Clipboard, u32::MAX));
        assert_eq!(search_results("[]"), Some(Vec::new()));
        assert_eq!(search_results("{}"), None);
        assert_eq!(search_results("not json"), None);
        for k in [
            "calculator",
            "app",
            "action",
            "file",
            "clipboard",
            "emoji",
            "web",
        ] {
            assert!(result_kind(k).is_some(), "{k}");
        }
    }

    #[test]
    fn previews_are_one_short_line() {
        assert_eq!(preview("  a\n\tb  c ", 10), "a b c");
        assert_eq!(preview("abcdefghij", 5), "abcd…");
        assert_eq!(preview("abcde", 5), "abcde");
        assert_eq!(preview("", 5), "");
    }

    #[test]
    fn emoji_capture_ocr_and_color_replies_are_parsed() {
        let items: Vec<EmojiItem> = vec![
            (
                "grinning".into(),
                "😀".into(),
                "Grinning face".into(),
                "smileys".into(),
                "smile happy".into(),
                false,
            ),
            (
                "thumbs-up".into(),
                "👍".into(),
                "Thumbs up".into(),
                "people".into(),
                "ok".into(),
                true,
            ),
        ];
        assert_eq!(
            emoji_rows(&(items,).to_variant()).unwrap(),
            [
                EmojiRow {
                    id: "grinning".into(),
                    glyph: "😀".into(),
                    name: "Grinning face".into(),
                },
                EmojiRow {
                    id: "thumbs-up".into(),
                    glyph: "👍".into(),
                    name: "Thumbs up".into(),
                },
            ]
        );
        assert_eq!(
            emoji_rows(&(Vec::<EmojiItem>::new(),).to_variant()),
            Some(vec![])
        );
        assert!(emoji_rows(&("wrong",).to_variant()).is_none());

        // Emoji.Pick and Capture.SaveScreenshot answer one string
        assert_eq!(single_string(&("👍🏽",).to_variant()).as_deref(), Some("👍🏽"));
        assert_eq!(
            single_string(&("/home/user/Pictures/Screenshots/a.png",).to_variant()).as_deref(),
            Some("/home/user/Pictures/Screenshots/a.png")
        );
        assert!(single_string(&(7_u32,).to_variant()).is_none());
        assert!(single_string(&"bare".to_variant()).is_none());

        let ocr = ("Hello\nworld", "Hello", vec!["ukr".to_owned()]).to_variant();
        assert_eq!(
            ocr_text(&ocr).unwrap(),
            OcrText {
                text: "Hello\nworld".into(),
                preview: "Hello".into(),
                missing_langs: vec!["ukr".into()],
            }
        );
        assert!(ocr_text(&("a", "b").to_variant()).is_none());

        let color = ("rgb(255, 0, 0)", "#FF0000", "red").to_variant();
        assert_eq!(
            color_record(&color).unwrap(),
            ColorRecord {
                formatted: "rgb(255, 0, 0)".into(),
                hex: "#FF0000".into(),
                name: "red".into(),
            }
        );
        assert!(color_record(&("a", "b").to_variant()).is_none());
    }

    #[test]
    fn remote_errors_keep_their_d_bus_name() {
        let e = glib::Error::new(
            gio::IOErrorEnum::DbusError,
            "GDBus.Error:io.github.OlehDzhumyk.Desk.Ocr.Error.MissingEngine: tesseract is not installed",
        );
        assert_eq!(
            call_error(&e),
            CallError::Remote {
                name: Some("io.github.OlehDzhumyk.Desk.Ocr.Error.MissingEngine".into()),
                message: "tesseract is not installed".into(),
            }
        );
        // an owner that left is "not available", not a remote error
        let gone = glib::Error::new(gio::DBusError::ServiceUnknown, "gone");
        assert_eq!(call_error(&gone), CallError::Unavailable);
    }
}
