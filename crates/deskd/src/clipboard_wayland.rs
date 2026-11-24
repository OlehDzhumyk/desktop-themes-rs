// SPDX-License-Identifier: GPL-3.0-or-later
//! Clipboard capture on Wayland compositors other than GNOME Shell (labwc, sway, niri, Hyprland,
//! KWin, COSMIC), through `ext-data-control-v1` or, as a fallback, `wlr-data-control-unstable-v1`.
//!
//! Mutter offers neither protocol: under GNOME the bridge captures the clipboard, and this watcher
//! ends right after the registry round trip, with no retry and no log line.
//!
//! A small Wayland wire client on a [`UnixStream`], with no Wayland library: it binds `wl_seat` and
//! the data-control manager (both at version 1), makes a data device and, for each new clipboard
//! selection, reads one type of the offer through a pipe. The rules match the bridge
//! (`bridge/src/clipboard/watcher.ts`): an offer with a password-manager hint
//! ([`SECRET_HINTS`]) or our own [`desk_proto::INTERNAL_MIME`] is never read; text comes before
//! images; images only when the `images` setting is on. The read is bounded in size
//! ([`MAX_TEXT_BYTES`], `image-max-bytes`) and in time ([`RECEIVE_TIMEOUT`]), so a source app that
//! never writes cannot hold deskd. The thread waits with `poll(2)` on the socket and a stop pipe;
//! [`Watcher::stop`] closes the pipe and the thread ends at once.

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::clipboard::{MAX_TEXT_BYTES, Pushed, SECRET_HINTS};
use crate::service::{self, Shared};

/// The longest wait for the source app to write the clipboard content.
pub const RECEIVE_TIMEOUT: Duration = Duration::from_secs(3);

const EXT_MANAGER: &str = "ext_data_control_manager_v1";
const WLR_MANAGER: &str = "zwlr_data_control_manager_v1";
const SEAT: &str = "wl_seat";

/// Text types in order of preference (the same list as the bridge). All are stored as UTF-8 text.
const TEXT_TYPES: [&str; 4] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "STRING",
];
/// Raster types only, the same as `IMAGE_TYPES` in `clipboard.rs` (deskd also checks the bytes).
const IMAGE_TYPES: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/gif",
    "image/webp",
];
const STORED_TEXT: &str = "text/plain;charset=utf-8";

/// Types kept per offer; an offer with more is cut (a client cannot grow deskd's memory).
const MAX_MIMES: usize = 64;
/// Offers announced but not yet selected; the oldest is dropped beyond this.
const MAX_OFFERS: usize = 8;
/// File descriptors accepted in one message (the events we use carry none).
const MAX_FDS: u32 = 8;

// Object ids: the client allocates them from 2 up (1 is wl_display); the server's are 0xff000000+.
const DISPLAY: u32 = 1;
const REGISTRY: u32 = 2;
const SYNC_CALLBACK: u32 = 3;
const SEAT_ID: u32 = 4;
const MANAGER_ID: u32 = 5;
const DEVICE_ID: u32 = 6;

// Opcodes. Requests and events have separate numbers per interface.
const DISPLAY_SYNC: u16 = 0;
const DISPLAY_GET_REGISTRY: u16 = 1;
const DISPLAY_ERROR: u16 = 0;
const REGISTRY_BIND: u16 = 0;
const REGISTRY_GLOBAL: u16 = 0;
const CALLBACK_DONE: u16 = 0;
const MANAGER_GET_DATA_DEVICE: u16 = 1;
const DEVICE_DATA_OFFER: u16 = 0;
const DEVICE_SELECTION: u16 = 1;
const DEVICE_FINISHED: u16 = 2;
const OFFER_RECEIVE: u16 = 0;
const OFFER_DESTROY: u16 = 1;
const OFFER_OFFER: u16 = 0;

// ---------------------------------------------------------------------------------------------
// Wire format: a header of two 32-bit words (object id; size << 16 | opcode), then the arguments
// in native byte order. A string is its length with the NUL, the bytes, the NUL, padding to 4.
// ---------------------------------------------------------------------------------------------

/// One message to send (a request; the tests also build events with it).
struct Message {
    buf: Vec<u8>,
}

impl Message {
    fn new(obj: u32, op: u16) -> Message {
        let mut buf = Vec::with_capacity(32);
        buf.extend(obj.to_ne_bytes());
        buf.extend(u32::from(op).to_ne_bytes());
        Message { buf }
    }

    fn uint(mut self, v: u32) -> Message {
        self.buf.extend(v.to_ne_bytes());
        self
    }

    fn string(mut self, s: &str) -> Message {
        let len = u32::try_from(s.len() + 1).unwrap_or(u32::MAX);
        self.buf.extend(len.to_ne_bytes());
        self.buf.extend(s.as_bytes());
        self.buf.push(0);
        while !self.buf.len().is_multiple_of(4) {
            self.buf.push(0);
        }
        self
    }

    /// The bytes, with the size in the header. Our messages are far below the 64 KiB limit: the
    /// longest string is a type name that the compositor sent in one message.
    fn finish(mut self) -> Vec<u8> {
        let size = u32::try_from(self.buf.len()).unwrap_or(0) & 0xffff;
        let word =
            (size << 16) | u32::from_ne_bytes([self.buf[4], self.buf[5], self.buf[6], self.buf[7]]);
        self.buf[4..8].copy_from_slice(&word.to_ne_bytes());
        self.buf
    }
}

/// One received message.
#[derive(Debug)]
struct Event {
    obj: u32,
    op: u16,
    body: Vec<u8>,
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("Wayland: {what}"))
}

/// Take one whole message from the front of `buf`, if it is all there.
fn take_event(buf: &mut Vec<u8>) -> io::Result<Option<Event>> {
    if buf.len() < 8 {
        return Ok(None);
    }
    let obj = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let word = u32::from_ne_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let size = usize::from(u16::try_from(word >> 16).unwrap_or(0));
    let op = u16::try_from(word & 0xffff).unwrap_or(u16::MAX);
    if size < 8 || !size.is_multiple_of(4) {
        return Err(invalid("bad message size"));
    }
    if buf.len() < size {
        return Ok(None);
    }
    let body = buf[8..size].to_vec();
    buf.drain(..size);
    Ok(Some(Event { obj, op, body }))
}

/// Reads the arguments of an event in order.
struct Args<'a>(&'a [u8]);

impl Args<'_> {
    fn uint(&mut self) -> io::Result<u32> {
        let (head, rest) = self
            .0
            .split_at_checked(4)
            .ok_or_else(|| invalid("short message"))?;
        self.0 = rest;
        Ok(u32::from_ne_bytes([head[0], head[1], head[2], head[3]]))
    }

    /// A non-null string (a null one, length 0, is an error here: no event we read allows it).
    fn string(&mut self) -> io::Result<String> {
        let len = usize::try_from(self.uint()?).map_err(|_| invalid("bad string"))?;
        if len == 0 {
            return Err(invalid("null string"));
        }
        let padded = len.div_ceil(4) * 4;
        let (head, rest) = self
            .0
            .split_at_checked(padded)
            .ok_or_else(|| invalid("short string"))?;
        self.0 = rest;
        if head[len - 1] != 0 {
            return Err(invalid("string without NUL"));
        }
        Ok(String::from_utf8_lossy(&head[..len - 1]).into_owned())
    }
}

// ---------------------------------------------------------------------------------------------
// Sockets, pipes, poll
// ---------------------------------------------------------------------------------------------

/// Convert between the integer types of `msghdr` fields, which differ between C libraries.
fn conv<A, B: TryFrom<A>>(a: A) -> io::Result<B> {
    B::try_from(a).map_err(|_| invalid("integer out of range"))
}

/// Send all of `data`; `fd` goes with the first byte (`SCM_RIGHTS`).
fn send_msg(sock: BorrowedFd<'_>, data: &[u8], fd: Option<BorrowedFd<'_>>) -> io::Result<()> {
    let mut sent = 0;
    let mut fd = fd;
    while sent < data.len() {
        let rest = &data[sent..];
        let mut iov = libc::iovec {
            iov_base: rest.as_ptr().cast_mut().cast(),
            iov_len: rest.len(),
        };
        let mut cbuf = [0u64; 4];
        // SAFETY: an all-zero msghdr is valid (no buffers); the pointers set below outlive the
        // call. The control buffer is 32 bytes, 8-aligned, and CMSG_SPACE(4) is at most 24, so
        // the one header and the descriptor fit; CMSG_FIRSTHDR is not null for such a buffer.
        let n = unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &raw mut iov;
            msg.msg_iovlen = 1;
            if let Some(f) = fd {
                msg.msg_control = cbuf.as_mut_ptr().cast();
                msg.msg_controllen = conv(libc::CMSG_SPACE(4))?;
                let cmsg = libc::CMSG_FIRSTHDR(&raw const msg);
                (*cmsg).cmsg_level = libc::SOL_SOCKET;
                (*cmsg).cmsg_type = libc::SCM_RIGHTS;
                (*cmsg).cmsg_len = conv(libc::CMSG_LEN(4))?;
                std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>(), f.as_raw_fd());
            }
            libc::sendmsg(sock.as_raw_fd(), &raw const msg, libc::MSG_NOSIGNAL)
        };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        sent += conv::<isize, usize>(n)?;
        fd = None;
    }
    Ok(())
}

/// Receive bytes and any file descriptors sent with them (closed on exec).
fn recv_msg(sock: BorrowedFd<'_>, buf: &mut [u8]) -> io::Result<(usize, Vec<OwnedFd>)> {
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    let mut cbuf = [0u64; 16];
    let mut fds = Vec::new();
    // SAFETY: as in send_msg; the 128-byte control buffer holds CMSG_SPACE(4 * MAX_FDS) = 48
    // bytes. The kernel fills the headers; we read only the SCM_RIGHTS payloads it wrote, and each
    // descriptor in them is new and owned by this process (MSG_CMSG_CLOEXEC).
    unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &raw mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cbuf.as_mut_ptr().cast();
        msg.msg_controllen = conv(libc::CMSG_SPACE(4 * MAX_FDS))?;
        let n = loop {
            let n = libc::recvmsg(sock.as_raw_fd(), &raw mut msg, libc::MSG_CMSG_CLOEXEC);
            if n >= 0 {
                break n;
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        };
        let mut cmsg = libc::CMSG_FIRSTHDR(&raw const msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let total: usize = conv((*cmsg).cmsg_len)?;
                let head: usize = conv(libc::CMSG_LEN(0))?;
                let data = libc::CMSG_DATA(cmsg).cast::<RawFd>();
                for i in 0..total.saturating_sub(head) / size_of::<RawFd>() {
                    fds.push(OwnedFd::from_raw_fd(std::ptr::read_unaligned(data.add(i))));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&raw const msg, cmsg);
        }
        Ok((conv(n)?, fds))
    }
}

/// A pipe, both ends closed on exec: (read, write).
fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds: [RawFd; 2] = [0; 2];
    // SAFETY: pipe2 writes two descriptors into the array; on success both are new and ours.
    unsafe {
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])))
    }
}

enum Ready {
    Fd,
    Stop,
    Timeout,
}

/// Wait until `fd` can be read (or is closed), or `stop` is readable or closed, or `timeout`.
fn wait(fd: BorrowedFd<'_>, stop: BorrowedFd<'_>, timeout: Option<Duration>) -> io::Result<Ready> {
    let ms = timeout.map_or(-1, |t| i32::try_from(t.as_millis() + 1).unwrap_or(i32::MAX));
    let mut fds = [
        libc::pollfd {
            fd: stop.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // SAFETY: two valid pollfd entries, the length matches the array.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        if fds[0].revents != 0 {
            return Ok(Ready::Stop);
        }
        if fds[1].revents != 0 {
            return Ok(Ready::Fd);
        }
        return Ok(Ready::Timeout);
    }
}

// ---------------------------------------------------------------------------------------------
// Which type to read
// ---------------------------------------------------------------------------------------------

/// The settings that decide whether and what to read (from the clipboard engine's config).
#[derive(Clone, Copy, Debug)]
struct Policy {
    /// `enabled` and not private mode.
    capture: bool,
    images: bool,
    image_max: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum Choice<'a> {
    /// Do not read the offer.
    Skip,
    /// Read `mime` (the offered spelling), store it as `store`, at most `limit` bytes.
    Read {
        mime: &'a str,
        store: &'static str,
        limit: usize,
    },
}

fn choose<'a>(offered: &'a [String], p: Policy) -> Choice<'a> {
    let has = |hint: &str| offered.iter().any(|m| m.eq_ignore_ascii_case(hint));
    if !p.capture || SECRET_HINTS.iter().any(|h| has(h)) || has(desk_proto::INTERNAL_MIME) {
        return Choice::Skip;
    }
    let find = |want: &str| offered.iter().find(|m| m.eq_ignore_ascii_case(want));
    for t in TEXT_TYPES {
        if let Some(m) = find(t) {
            return Choice::Read {
                mime: m,
                store: STORED_TEXT,
                limit: MAX_TEXT_BYTES,
            };
        }
    }
    if p.images {
        for t in IMAGE_TYPES {
            if let Some(m) = find(t) {
                return Choice::Read {
                    mime: m,
                    store: t,
                    limit: p.image_max,
                };
            }
        }
    }
    Choice::Skip
}

// ---------------------------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------------------------

/// Where captured content goes.
pub struct Target {
    pub eng: Shared,
    /// Runs after a push that changed the history (save soon, tell the clients).
    pub on_change: Box<dyn Fn() + Send>,
}

impl Target {
    fn policy(&self) -> Policy {
        let e = service::lock(&self.eng);
        Policy {
            capture: e.cfg.enabled && !e.cfg.private,
            images: e.cfg.images,
            image_max: e.cfg.image_max_bytes,
        }
    }

    fn push(&self, mime: &str, data: &[u8], offered: &[String]) {
        let r = {
            let mut e = service::lock(&self.eng);
            let now = service::now();
            e.expire(now);
            // Wayland does not say which app owns the selection, so `excluded-apps` cannot apply.
            e.push(mime, data, offered, "", now)
        };
        if !matches!(r, Pushed::Skipped(_)) {
            (self.on_change)();
        }
    }
}

/// How the watcher ended.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Exit {
    /// The compositor offers no data-control protocol (GNOME Shell).
    Unsupported,
    /// [`Watcher::stop`].
    Stopped,
    /// The compositor ended the data device.
    Finished,
}

struct Conn {
    sock: UnixStream,
    inbuf: Vec<u8>,
}

impl Conn {
    fn send(&self, msg: Message, fd: Option<BorrowedFd<'_>>) -> io::Result<()> {
        send_msg(self.sock.as_fd(), &msg.finish(), fd)
    }

    /// The next event, or `None` when `stop` fires first.
    fn next(&mut self, stop: BorrowedFd<'_>) -> io::Result<Option<Event>> {
        loop {
            if let Some(ev) = take_event(&mut self.inbuf)? {
                return Ok(Some(ev));
            }
            match wait(self.sock.as_fd(), stop, None)? {
                Ready::Stop => return Ok(None),
                Ready::Timeout => continue,
                Ready::Fd => {}
            }
            let mut chunk = [0u8; 4096];
            // descriptors sent with an event are closed at once (none of our events has one)
            let (n, _fds) = recv_msg(self.sock.as_fd(), &mut chunk)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the compositor closed the connection",
                ));
            }
            self.inbuf.extend_from_slice(&chunk[..n]);
        }
    }
}

fn display_error(ev: &Event) -> io::Error {
    let mut a = Args(&ev.body);
    let (obj, code) = (a.uint().unwrap_or(0), a.uint().unwrap_or(0));
    let text = a.string().unwrap_or_default();
    io::Error::other(format!("Wayland error {code} on object {obj}: {text}"))
}

/// The globals we want, by registry name.
#[derive(Default, Debug)]
struct Globals {
    seat: Option<u32>,
    ext: Option<u32>,
    wlr: Option<u32>,
}

impl Globals {
    fn add(&mut self, body: &[u8]) -> io::Result<()> {
        let mut a = Args(body);
        let name = a.uint()?;
        let iface = a.string()?;
        let slot = match iface.as_str() {
            SEAT => &mut self.seat,
            EXT_MANAGER => &mut self.ext,
            WLR_MANAGER => &mut self.wlr,
            _ => return Ok(()),
        };
        // the first seat only (data control follows the default seat)
        slot.get_or_insert(name);
        Ok(())
    }

    /// (manager name, manager interface, seat name); `ext` is preferred.
    fn pick(&self) -> Option<(u32, &'static str, u32)> {
        let seat = self.seat?;
        match (self.ext, self.wlr) {
            (Some(m), _) => Some((m, EXT_MANAGER, seat)),
            (None, Some(m)) => Some((m, WLR_MANAGER, seat)),
            (None, None) => None,
        }
    }
}

fn bind(c: &Conn, name: u32, iface: &str, id: u32) -> io::Result<()> {
    c.send(
        Message::new(REGISTRY, REGISTRY_BIND)
            .uint(name)
            .string(iface)
            .uint(1)
            .uint(id),
        None,
    )
}

/// Speak to the compositor on `sock` until `stop` fires, the device ends, or an error.
fn run(sock: UnixStream, stop: BorrowedFd<'_>, target: &Target) -> io::Result<Exit> {
    let mut c = Conn {
        sock,
        inbuf: Vec::new(),
    };
    c.send(
        Message::new(DISPLAY, DISPLAY_GET_REGISTRY).uint(REGISTRY),
        None,
    )?;
    c.send(
        Message::new(DISPLAY, DISPLAY_SYNC).uint(SYNC_CALLBACK),
        None,
    )?;
    let mut globals = Globals::default();
    loop {
        let Some(ev) = c.next(stop)? else {
            return Ok(Exit::Stopped);
        };
        match (ev.obj, ev.op) {
            (REGISTRY, REGISTRY_GLOBAL) => globals.add(&ev.body)?,
            (SYNC_CALLBACK, CALLBACK_DONE) => break,
            (DISPLAY, DISPLAY_ERROR) => return Err(display_error(&ev)),
            _ => {}
        }
    }
    let Some((manager, iface, seat)) = globals.pick() else {
        return Ok(Exit::Unsupported);
    };
    bind(&c, seat, SEAT, SEAT_ID)?;
    bind(&c, manager, iface, MANAGER_ID)?;
    c.send(
        Message::new(MANAGER_ID, MANAGER_GET_DATA_DEVICE)
            .uint(DEVICE_ID)
            .uint(SEAT_ID),
        None,
    )?;

    // offers announced by data_offer, with their types, oldest first
    let mut offers: Vec<(u32, Vec<String>)> = Vec::new();
    loop {
        let Some(ev) = c.next(stop)? else {
            return Ok(Exit::Stopped);
        };
        match (ev.obj, ev.op) {
            (DISPLAY, DISPLAY_ERROR) => return Err(display_error(&ev)),
            (DEVICE_ID, DEVICE_DATA_OFFER) => {
                let id = Args(&ev.body).uint()?;
                if offers.len() >= MAX_OFFERS {
                    let (old, _) = offers.remove(0);
                    c.send(Message::new(old, OFFER_DESTROY), None)?;
                }
                offers.push((id, Vec::new()));
            }
            (DEVICE_ID, DEVICE_SELECTION) => {
                let id = Args(&ev.body).uint()?;
                // the offers before this selection are stale: only the selected one is read
                let mut selected = None;
                for (o, mimes) in offers.drain(..) {
                    if o == id {
                        selected = Some(mimes);
                    } else {
                        c.send(Message::new(o, OFFER_DESTROY), None)?;
                    }
                }
                if let Some(mimes) = selected {
                    let stopped = capture(&c, stop, target, id, &mimes);
                    c.send(Message::new(id, OFFER_DESTROY), None)?;
                    if stopped? {
                        return Ok(Exit::Stopped);
                    }
                }
            }
            (DEVICE_ID, DEVICE_FINISHED) => return Ok(Exit::Finished),
            (obj, OFFER_OFFER) => {
                if let Some((_, mimes)) = offers.iter_mut().find(|(o, _)| *o == obj)
                    && mimes.len() < MAX_MIMES
                {
                    mimes.push(Args(&ev.body).string()?);
                }
            }
            _ => {}
        }
    }
}

/// Read the selected offer into the history. `Ok(true)`: `stop` fired during the read.
fn capture(
    c: &Conn,
    stop: BorrowedFd<'_>,
    target: &Target,
    offer: u32,
    offered: &[String],
) -> io::Result<bool> {
    let Choice::Read { mime, store, limit } = choose(offered, target.policy()) else {
        return Ok(false);
    };
    let (r, w) = pipe()?;
    c.send(
        Message::new(offer, OFFER_RECEIVE).string(mime),
        Some(w.as_fd()),
    )?;
    drop(w);
    match read_bounded(File::from(r), stop, limit, RECEIVE_TIMEOUT)? {
        Bounded::Data(data) => target.push(store, &data, offered),
        Bounded::Stopped => return Ok(true),
        Bounded::TooLarge | Bounded::TimedOut => {}
    }
    Ok(false)
}

#[derive(Debug, PartialEq, Eq)]
enum Bounded {
    Data(Vec<u8>),
    TooLarge,
    TimedOut,
    Stopped,
}

/// Read `pipe` to its end: at most `limit` bytes, within `timeout`, unless `stop` fires.
fn read_bounded(
    mut pipe: File,
    stop: BorrowedFd<'_>,
    limit: usize,
    timeout: Duration,
) -> io::Result<Bounded> {
    let deadline = Instant::now() + timeout;
    let mut data = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(Bounded::TimedOut);
        }
        match wait(pipe.as_fd(), stop, Some(left))? {
            Ready::Stop => return Ok(Bounded::Stopped),
            Ready::Timeout => return Ok(Bounded::TimedOut),
            Ready::Fd => {}
        }
        let n = match pipe.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if n == 0 {
            return Ok(Bounded::Data(data));
        }
        if data.len() + n > limit {
            return Ok(Bounded::TooLarge);
        }
        data.extend_from_slice(&chunk[..n]);
    }
}

// ---------------------------------------------------------------------------------------------
// The thread
// ---------------------------------------------------------------------------------------------

/// `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` (or `$WAYLAND_DISPLAY` when it is an absolute path).
fn socket_path() -> Option<PathBuf> {
    let display = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").filter(|d| !d.is_empty())?);
    if display.is_absolute() {
        return Some(display);
    }
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    runtime.is_absolute().then(|| runtime.join(display))
}

/// The capture thread. Dropping it stops it too.
pub struct Watcher {
    /// The write end of the stop pipe: closing it wakes the thread's `poll`.
    stop: Option<OwnedFd>,
    thread: Option<JoinHandle<Option<Exit>>>,
}

impl Watcher {
    /// Start when this is a Wayland session; `None` without `WAYLAND_DISPLAY` or a socket.
    pub fn start(target: Target) -> Option<Watcher> {
        let sock = UnixStream::connect(socket_path()?).ok()?;
        match Watcher::start_on(sock, target) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("deskd: cannot start the Wayland clipboard watcher: {e}");
                None
            }
        }
    }

    fn start_on(sock: UnixStream, target: Target) -> io::Result<Watcher> {
        let (r, w) = pipe()?;
        let thread = std::thread::Builder::new()
            .name("deskd-wl-clipboard".into())
            .spawn(move || match run(sock, r.as_fd(), &target) {
                Ok(exit) => Some(exit),
                Err(e) => {
                    eprintln!("deskd: the Wayland clipboard watcher stopped: {e}");
                    None
                }
            })?;
        Ok(Watcher {
            stop: Some(w),
            thread: Some(thread),
        })
    }

    /// Stop the thread and wait for it; how it ended (`None` after an error or a second call).
    pub fn stop(&mut self) -> Option<Exit> {
        drop(self.stop.take());
        self.thread.take().and_then(|t| t.join().ok().flatten())
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::{Config, Engine};
    use std::io::Write;
    use std::sync::{Arc, Mutex, mpsc};

    const SERVER_OFFER: u32 = 0xff00_0000;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn strings_are_padded_and_parsed_back() {
        for (s, padded) in [
            ("", 4),
            ("abc", 4),
            ("abcd", 8),
            ("wl_seat", 8),
            ("wl_seat1", 12),
        ] {
            let bytes = Message::new(9, 3).string(s).uint(77).finish();
            assert_eq!(bytes.len(), 8 + 4 + padded + 4, "{s:?}");
            let mut buf = bytes.clone();
            let ev = take_event(&mut buf).unwrap().unwrap();
            assert!(buf.is_empty());
            assert_eq!((ev.obj, ev.op), (9, 3));
            let mut a = Args(&ev.body);
            assert_eq!(a.string().unwrap(), s);
            assert_eq!(a.uint().unwrap(), 77);
            assert!(a.uint().is_err(), "the body ends there");
        }
    }

    #[test]
    fn partial_and_bad_messages() {
        let whole = Message::new(2, 0).uint(5).finish();
        let mut buf = whole[..10].to_vec();
        assert!(take_event(&mut buf).unwrap().is_none(), "wait for the rest");
        buf.extend_from_slice(&whole[10..]);
        buf.extend_from_slice(&whole);
        assert!(take_event(&mut buf).unwrap().is_some());
        assert!(take_event(&mut buf).unwrap().is_some());
        assert!(buf.is_empty());

        let mut bad = Message::new(2, 0).finish();
        bad[4..8].copy_from_slice(&(6u32 << 16).to_ne_bytes());
        assert!(take_event(&mut bad).is_err(), "size below the header");
        let mut no_nul = Message::new(2, 0).uint(4).finish();
        no_nul.extend_from_slice(b"abcd");
        assert!(Args(&no_nul[8..]).string().is_err());
        assert!(Args(&0u32.to_ne_bytes()).string().is_err(), "null string");
    }

    #[test]
    fn registry_globals_prefer_ext_over_wlr() {
        let global = |name, iface: &str| {
            let mut b = Message::new(REGISTRY, REGISTRY_GLOBAL)
                .uint(name)
                .string(iface)
                .uint(2)
                .finish();
            take_event(&mut b).unwrap().unwrap().body
        };
        let mut g = Globals::default();
        for (n, i) in [
            (1, "wl_compositor"),
            (2, SEAT),
            (3, WLR_MANAGER),
            (9, "wl_seat"),
        ] {
            g.add(&global(n, i)).unwrap();
        }
        assert_eq!(
            g.pick(),
            Some((3, WLR_MANAGER, 2)),
            "first seat, wlroots fallback"
        );
        g.add(&global(7, EXT_MANAGER)).unwrap();
        assert_eq!(g.pick(), Some((7, EXT_MANAGER, 2)));

        let mut mutter = Globals::default();
        mutter.add(&global(1, SEAT)).unwrap();
        mutter.add(&global(2, "wl_data_device_manager")).unwrap();
        assert_eq!(mutter.pick(), None);
    }

    #[test]
    fn scm_rights_carries_a_pipe() {
        let (a, b) = UnixStream::pair().unwrap();
        let (r, w) = pipe().unwrap();
        send_msg(
            a.as_fd(),
            &Message::new(5, 0).string("x").finish(),
            Some(w.as_fd()),
        )
        .unwrap();
        drop(w);
        let mut buf = [0u8; 64];
        let (n, mut fds) = recv_msg(b.as_fd(), &mut buf).unwrap();
        assert_eq!((n, fds.len()), (16, 1));
        File::from(fds.remove(0))
            .write_all(b"through the socket")
            .unwrap();
        let mut got = String::new();
        File::from(r).read_to_string(&mut got).unwrap();
        assert_eq!(got, "through the socket");
    }

    const ON: Policy = Policy {
        capture: true,
        images: true,
        image_max: 1000,
    };

    #[test]
    fn choose_text_first_then_images() {
        let read = |mime, store, limit| Choice::Read { mime, store, limit };
        let o = strings(&["image/png", "TEXT", "text/plain", "UTF8_STRING"]);
        assert_eq!(
            choose(&o, ON),
            read("UTF8_STRING", STORED_TEXT, MAX_TEXT_BYTES)
        );
        let o = strings(&["image/png", "Text/Plain;Charset=UTF-8"]);
        assert_eq!(
            choose(&o, ON),
            read("Text/Plain;Charset=UTF-8", STORED_TEXT, MAX_TEXT_BYTES)
        );
        let o = strings(&["text/html", "image/webp", "image/png"]);
        assert_eq!(choose(&o, ON), read("image/png", "image/png", 1000));
        assert_eq!(
            choose(
                &o,
                Policy {
                    images: false,
                    ..ON
                }
            ),
            Choice::Skip
        );
        assert_eq!(
            choose(&strings(&["image/svg+xml", "text/html"]), ON),
            Choice::Skip
        );
        assert_eq!(choose(&[], ON), Choice::Skip);
    }

    #[test]
    fn choose_never_reads_secrets_internal_or_when_off() {
        for hint in SECRET_HINTS
            .iter()
            .copied()
            .chain([desk_proto::INTERNAL_MIME])
        {
            let o = strings(&["text/plain;charset=utf-8", &hint.to_uppercase()]);
            assert_eq!(choose(&o, ON), Choice::Skip, "{hint}");
        }
        let o = strings(&["text/plain"]);
        assert_eq!(
            choose(
                &o,
                Policy {
                    capture: false,
                    ..ON
                }
            ),
            Choice::Skip
        );
    }

    #[test]
    fn read_bounded_limits_size_and_stops() {
        let (stop_r, stop_w) = pipe().unwrap();
        let (r, w) = pipe().unwrap();
        File::from(w).write_all(b"12345").unwrap();
        assert_eq!(
            read_bounded(File::from(r), stop_r.as_fd(), 4, RECEIVE_TIMEOUT).unwrap(),
            Bounded::TooLarge
        );
        // a source that never writes nor closes: closing the stop pipe ends the read
        let (r, _w_held) = pipe().unwrap();
        drop(stop_w);
        assert_eq!(
            read_bounded(File::from(r), stop_r.as_fd(), 4, RECEIVE_TIMEOUT).unwrap(),
            Bounded::Stopped
        );
    }

    /// The compositor side of the socket.
    struct Mock {
        sock: UnixStream,
        buf: Vec<u8>,
        fds: Vec<OwnedFd>,
    }

    impl Mock {
        fn send(&self, m: Message) {
            send_msg(self.sock.as_fd(), &m.finish(), None).unwrap();
        }

        fn recv(&mut self) -> Event {
            loop {
                if let Some(ev) = take_event(&mut self.buf).unwrap() {
                    return ev;
                }
                let mut chunk = [0u8; 4096];
                let (n, fds) = recv_msg(self.sock.as_fd(), &mut chunk).unwrap();
                assert!(n > 0, "the client closed the socket");
                self.buf.extend_from_slice(&chunk[..n]);
                self.fds.extend(fds);
            }
        }

        /// Answer the registry round trip with `globals`; return the client's binds.
        fn handshake(&mut self, globals: &[(u32, &str)]) -> Vec<(u32, String, u32, u32)> {
            let ev = self.recv();
            assert_eq!(
                (ev.obj, ev.op, Args(&ev.body).uint().unwrap()),
                (DISPLAY, DISPLAY_GET_REGISTRY, REGISTRY)
            );
            let ev = self.recv();
            assert_eq!((ev.obj, ev.op), (DISPLAY, DISPLAY_SYNC));
            let cb = Args(&ev.body).uint().unwrap();
            for (name, iface) in globals {
                self.send(
                    Message::new(REGISTRY, REGISTRY_GLOBAL)
                        .uint(*name)
                        .string(iface)
                        .uint(3),
                );
            }
            self.send(Message::new(cb, CALLBACK_DONE).uint(0));
            let mut binds = vec![];
            if globals
                .iter()
                .any(|(_, i)| *i == EXT_MANAGER || *i == WLR_MANAGER)
            {
                for _ in 0..2 {
                    let ev = self.recv();
                    assert_eq!((ev.obj, ev.op), (REGISTRY, REGISTRY_BIND));
                    let mut a = Args(&ev.body);
                    binds.push((
                        a.uint().unwrap(),
                        a.string().unwrap(),
                        a.uint().unwrap(),
                        a.uint().unwrap(),
                    ));
                }
                let ev = self.recv();
                let mut a = Args(&ev.body);
                assert_eq!((ev.obj, ev.op), (MANAGER_ID, MANAGER_GET_DATA_DEVICE));
                assert_eq!((a.uint().unwrap(), a.uint().unwrap()), (DEVICE_ID, SEAT_ID));
            }
            binds
        }

        fn select(&self, offer: u32, mimes: &[&str]) {
            self.send(Message::new(DEVICE_ID, DEVICE_DATA_OFFER).uint(offer));
            for m in mimes {
                self.send(Message::new(offer, OFFER_OFFER).string(m));
            }
            self.send(Message::new(DEVICE_ID, DEVICE_SELECTION).uint(offer));
        }

        fn expect(&mut self, obj: u32, op: u16) -> Event {
            let ev = self.recv();
            assert_eq!((ev.obj, ev.op), (obj, op), "{ev:?}");
            ev
        }
    }

    fn setup(cfg: Config) -> (Watcher, Mock, Shared, mpsc::Receiver<()>) {
        let dir = std::env::temp_dir().join(format!("deskd-test-wl-{}", std::process::id()));
        let eng: Shared = Arc::new(Mutex::new(Engine::new(
            cfg,
            dir.join("data"),
            dir.join("run"),
        )));
        let (tx, rx) = mpsc::channel();
        let target = Target {
            eng: eng.clone(),
            on_change: Box::new(move || {
                let _ = tx.send(());
            }),
        };
        let (client, server) = UnixStream::pair().unwrap();
        let w = Watcher::start_on(client, target).unwrap();
        (
            w,
            Mock {
                sock: server,
                buf: vec![],
                fds: vec![],
            },
            eng,
            rx,
        )
    }

    #[test]
    fn mock_compositor_selection_to_history_and_stop() {
        let (mut w, mut m, eng, changed) = setup(Config::default());
        let binds = m.handshake(&[(1, SEAT), (2, WLR_MANAGER), (3, EXT_MANAGER)]);
        assert_eq!(
            binds,
            [
                (1, SEAT.to_string(), 1, SEAT_ID),
                (3, EXT_MANAGER.to_string(), 1, MANAGER_ID)
            ]
        );

        // a stale offer (never selected) is dropped; the selected one is read as UTF-8 text
        m.send(Message::new(DEVICE_ID, DEVICE_DATA_OFFER).uint(SERVER_OFFER + 9));
        m.select(SERVER_OFFER, &["TEXT", "text/plain;charset=utf-8"]);
        m.expect(SERVER_OFFER + 9, OFFER_DESTROY);
        let ev = m.expect(SERVER_OFFER, OFFER_RECEIVE);
        assert_eq!(Args(&ev.body).string().unwrap(), "text/plain;charset=utf-8");
        let mut fd = File::from(m.fds.pop().expect("the pipe came with receive"));
        fd.write_all("hello Wayland ✓".as_bytes()).unwrap();
        drop(fd);
        m.expect(SERVER_OFFER, OFFER_DESTROY);
        changed.recv().unwrap();
        {
            let e = service::lock(&eng);
            assert_eq!(e.entries()[0].text, "hello Wayland ✓");
            assert_eq!(e.entries()[0].mime, STORED_TEXT);
        }

        // a password manager's copy and our own paste: destroyed without receive
        m.select(
            SERVER_OFFER + 1,
            &["text/plain", "x-kde-passwordManagerHint"],
        );
        m.expect(SERVER_OFFER + 1, OFFER_DESTROY);
        m.select(SERVER_OFFER + 2, &["text/plain", desk_proto::INTERNAL_MIME]);
        m.expect(SERVER_OFFER + 2, OFFER_DESTROY);
        // private mode: not read either
        service::lock(&eng).cfg.private = true;
        m.select(SERVER_OFFER + 3, &["text/plain"]);
        m.expect(SERVER_OFFER + 3, OFFER_DESTROY);
        assert!(m.fds.is_empty());
        assert_eq!(service::lock(&eng).len(), 1);

        assert_eq!(w.stop(), Some(Exit::Stopped));
    }

    #[test]
    fn stop_ends_a_read_from_a_dead_source() {
        let (mut w, mut m, eng, _changed) = setup(Config::default());
        m.handshake(&[(1, SEAT), (2, WLR_MANAGER)]);
        m.select(SERVER_OFFER, &["text/plain"]);
        m.expect(SERVER_OFFER, OFFER_RECEIVE);
        let _held = m.fds.pop().unwrap(); // the source never writes nor closes
        assert_eq!(w.stop(), Some(Exit::Stopped));
        assert!(service::lock(&eng).is_empty());
    }

    #[test]
    fn no_data_control_ends_at_once_and_finished_device_ends() {
        let (mut w, mut m, _eng, _changed) = setup(Config::default());
        m.handshake(&[(1, SEAT), (2, "wl_data_device_manager")]);
        // the thread ends by itself; stop only joins it
        let mut rest = [0u8; 16];
        assert_eq!(
            recv_msg(m.sock.as_fd(), &mut rest).unwrap().0,
            0,
            "socket closed"
        );
        assert_eq!(w.stop(), Some(Exit::Unsupported));

        let (mut w, mut m, _eng, _changed) = setup(Config::default());
        m.handshake(&[(1, SEAT), (2, WLR_MANAGER)]);
        m.send(Message::new(DEVICE_ID, DEVICE_FINISHED));
        assert_eq!(
            recv_msg(m.sock.as_fd(), &mut rest).unwrap().0,
            0,
            "socket closed"
        );
        assert_eq!(w.stop(), Some(Exit::Finished));
    }
}
