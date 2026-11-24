// SPDX-License-Identifier: GPL-3.0-or-later
//! The running windows of the session: a small, pure-Rust client of the Wayland protocol
//! `zwlr_foreign_toplevel_management_v1` (labwc, sway, Hyprland, niri, Wayfire), with no
//! Wayland library.
//!
//! A worker thread opens its own connection to the compositor (`$WAYLAND_DISPLAY`), binds
//! `wl_seat` and `zwlr_foreign_toplevel_manager_v1`, and reports every window as an [`Update`]
//! when the compositor sends `done`, and when it closes. It ends without a message when the
//! compositor does not have the protocol (GNOME, KWin). The thread waits in `poll(2)` on the
//! socket and on a wake-up socket of its handle ([`Toplevels`]), so it never sleeps or polls;
//! [`Toplevels::send`] asks it to activate, minimize, maximize or close a window.
//!
//! Only the messages of the protocols above are decoded (no file descriptors are sent by them).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

/// The interface of the manager global.
pub const MANAGER_INTERFACE: &str = "zwlr_foreign_toplevel_manager_v1";
const SEAT_INTERFACE: &str = "wl_seat";
/// The highest manager version this client knows (v3 adds `parent`, v2 `fullscreen`).
const MANAGER_VERSION: u32 = 3;

/// Object ids: `wl_display` is 1; the client picks the others from 2 up.
const DISPLAY: u32 = 1;
const REGISTRY: u32 = 2;
const SYNC_CALLBACK: u32 = 3;
const SEAT: u32 = 4;
const MANAGER: u32 = 5;

/// The largest message the wire format allows (the size field is 16 bits).
const MAX_MESSAGE: usize = 0xffff;

mod op {
    // wl_display requests and events
    pub const DISPLAY_SYNC: u16 = 0;
    pub const DISPLAY_GET_REGISTRY: u16 = 1;
    pub const DISPLAY_ERROR: u16 = 0;
    // wl_registry
    pub const REGISTRY_BIND: u16 = 0;
    pub const REGISTRY_GLOBAL: u16 = 0;
    // wl_callback
    pub const CALLBACK_DONE: u16 = 0;
    // zwlr_foreign_toplevel_manager_v1
    pub const MANAGER_STOP: u16 = 0;
    pub const MANAGER_TOPLEVEL: u16 = 0;
    pub const MANAGER_FINISHED: u16 = 1;
    // zwlr_foreign_toplevel_handle_v1 requests
    pub const SET_MAXIMIZED: u16 = 0;
    pub const UNSET_MAXIMIZED: u16 = 1;
    pub const SET_MINIMIZED: u16 = 2;
    pub const UNSET_MINIMIZED: u16 = 3;
    pub const ACTIVATE: u16 = 4;
    pub const CLOSE: u16 = 5;
    pub const DESTROY: u16 = 7;
    // zwlr_foreign_toplevel_handle_v1 events
    pub const TITLE: u16 = 0;
    pub const APP_ID: u16 = 1;
    pub const STATE: u16 = 4;
    pub const DONE: u16 = 5;
    pub const CLOSED: u16 = 6;
}

/// The `zwlr_foreign_toplevel_handle_v1.state` values.
const STATE_MAXIMIZED: u32 = 0;
const STATE_MINIMIZED: u32 = 1;
const STATE_ACTIVATED: u32 = 2;
const STATE_FULLSCREEN: u32 = 3;

/// The state of a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub maximized: bool,
    pub minimized: bool,
    pub activated: bool,
    pub fullscreen: bool,
}

impl State {
    /// Decode the `state` array (32-bit values in host byte order); unknown values are skipped.
    pub fn from_array(bytes: &[u8]) -> State {
        let mut s = State::default();
        for chunk in bytes.as_chunks::<4>().0 {
            match u32::from_ne_bytes(*chunk) {
                STATE_MAXIMIZED => s.maximized = true,
                STATE_MINIMIZED => s.minimized = true,
                STATE_ACTIVATED => s.activated = true,
                STATE_FULLSCREEN => s.fullscreen = true,
                _ => {}
            }
        }
        s
    }

    /// Encode as a `state` array (the compositor side, for the mock compositor of the tests).
    #[cfg(test)]
    pub fn to_array(self) -> Vec<u8> {
        [
            (self.maximized, STATE_MAXIMIZED),
            (self.minimized, STATE_MINIMIZED),
            (self.activated, STATE_ACTIVATED),
            (self.fullscreen, STATE_FULLSCREEN),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .flat_map(|(_, v)| v.to_ne_bytes())
        .collect()
    }
}

/// One window, as the compositor described it at its last `done`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Toplevel {
    /// The protocol object id of the window (stable while it is open).
    pub id: u32,
    pub title: String,
    pub app_id: String,
    pub state: State,
}

/// What the worker thread reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// A new window, or new title, app id or state of a known one.
    Changed(Toplevel),
    /// The window closed.
    Closed(u32),
    /// No more updates: the manager stopped or the connection ended. Forget every window.
    Finished,
}

/// What the shell can ask for a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Bring the window to the front and focus it (restores it when minimized).
    Activate(u32),
    ToggleMinimize(u32),
    ToggleMaximize(u32),
    /// Ask the application to close the window.
    Close(u32),
}

/// A message that does not follow the wire format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    /// The size in the header is below 8, not a multiple of 4.
    BadSize(usize),
    /// An argument goes past the end of the message.
    Truncated,
    /// The compositor sent `wl_display.error`.
    Protocol {
        object: u32,
        code: u32,
        message: String,
    },
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::BadSize(n) => write!(f, "bad message size {n}"),
            WireError::Truncated => write!(f, "truncated message"),
            WireError::Protocol {
                object,
                code,
                message,
            } => write!(f, "protocol error {code} on object {object}: {message}"),
        }
    }
}

impl From<WireError> for io::Error {
    fn from(e: WireError) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, e.to_string())
    }
}

/// One Wayland message: the object, the opcode and the encoded arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub object: u32,
    pub opcode: u16,
    pub args: Vec<u8>,
}

/// Pad `n` up to a multiple of 4.
fn padded(n: usize) -> usize {
    n.div_ceil(4) * 4
}

impl Message {
    pub fn new(object: u32, opcode: u16) -> Message {
        Message {
            object,
            opcode,
            args: Vec::new(),
        }
    }

    /// Append a `uint`, `int`, `object` or `new_id` argument.
    #[must_use]
    pub fn uint(mut self, v: u32) -> Message {
        self.args.extend_from_slice(&v.to_ne_bytes());
        self
    }

    /// Append a `string` argument (length with the closing NUL, then padding).
    #[must_use]
    pub fn string(self, s: &str) -> Message {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        self.array(&bytes)
    }

    /// Append an `array` argument (length, bytes, padding).
    #[must_use]
    pub fn array(mut self, bytes: &[u8]) -> Message {
        let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        self.args.extend_from_slice(&len.to_ne_bytes());
        self.args.extend_from_slice(bytes);
        self.args
            .resize(self.args.len() + padded(bytes.len()) - bytes.len(), 0);
        self
    }

    /// The message on the wire: object id, `size << 16 | opcode`, arguments.
    pub fn encode(&self) -> Vec<u8> {
        let size = u32::try_from(8 + self.args.len()).unwrap_or(u32::MAX);
        let mut out = Vec::with_capacity(8 + self.args.len());
        out.extend_from_slice(&self.object.to_ne_bytes());
        out.extend_from_slice(&((size << 16) | u32::from(self.opcode)).to_ne_bytes());
        out.extend_from_slice(&self.args);
        out
    }

    /// Split the first complete message off `buf`: `None` until all its bytes are there.
    pub fn parse(buf: &[u8]) -> Result<Option<(Message, usize)>, WireError> {
        if buf.len() < 8 {
            return Ok(None);
        }
        let object = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let word = u32::from_ne_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let size = (word >> 16) as usize;
        if size < 8 || !size.is_multiple_of(4) {
            return Err(WireError::BadSize(size));
        }
        if buf.len() < size {
            return Ok(None);
        }
        let opcode = u16::try_from(word & 0xffff).unwrap_or(u16::MAX);
        let msg = Message {
            object,
            opcode,
            args: buf[8..size].to_vec(),
        };
        Ok(Some((msg, size)))
    }

    /// A reader of the arguments.
    pub fn reader(&self) -> ArgReader<'_> {
        ArgReader {
            buf: &self.args,
            pos: 0,
        }
    }
}

/// Reads the arguments of a [`Message`] in order.
pub struct ArgReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ArgReader<'a> {
    pub fn uint(&mut self) -> Result<u32, WireError> {
        let b = self
            .buf
            .get(self.pos..self.pos + 4)
            .ok_or(WireError::Truncated)?;
        self.pos += 4;
        Ok(u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn array(&mut self) -> Result<&'a [u8], WireError> {
        let len = self.uint()? as usize;
        let bytes = self
            .buf
            .get(self.pos..self.pos + len)
            .ok_or(WireError::Truncated)?;
        self.pos += padded(len);
        if self.pos > self.buf.len() {
            return Err(WireError::Truncated);
        }
        Ok(bytes)
    }

    /// A `string`: invalid UTF-8 is replaced, a null string (length 0) is empty.
    pub fn string(&mut self) -> Result<String, WireError> {
        let bytes = self.array()?;
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// Buffers the bytes read from the socket and hands out complete messages.
#[derive(Default)]
struct Reader {
    buf: Vec<u8>,
}

impl Reader {
    /// Read once; `Ok(0)` at the end of the stream.
    fn fill(&mut self, stream: &mut impl Read) -> io::Result<usize> {
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk)?;
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(n)
    }

    fn next(&mut self) -> Result<Option<Message>, WireError> {
        match Message::parse(&self.buf)? {
            Some((msg, used)) => {
                self.buf.drain(..used);
                Ok(Some(msg))
            }
            None => Ok(None),
        }
    }

    /// Read until a message is complete.
    fn next_blocking(&mut self, stream: &mut impl Read) -> io::Result<Message> {
        loop {
            if let Some(m) = self.next()? {
                return Ok(m);
            }
            if self.buf.len() > MAX_MESSAGE {
                return Err(WireError::BadSize(self.buf.len()).into());
            }
            if self.fill(stream)? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
    }
}

/// The error of a `wl_display.error` event.
fn display_error(msg: &Message) -> WireError {
    let mut r = msg.reader();
    let object = r.uint().unwrap_or(0);
    let code = r.uint().unwrap_or(0);
    let message = r.string().unwrap_or_default();
    WireError::Protocol {
        object,
        code,
        message,
    }
}

/// The globals the client needs: `(name, version)` of each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Globals {
    seat: Option<(u32, u32)>,
    manager: Option<(u32, u32)>,
}

/// Ask for the globals and wait for the compositor's answer (one roundtrip).
fn read_globals(stream: &mut UnixStream, reader: &mut Reader) -> io::Result<Globals> {
    let mut out = Message::new(DISPLAY, op::DISPLAY_GET_REGISTRY)
        .uint(REGISTRY)
        .encode();
    out.extend(
        Message::new(DISPLAY, op::DISPLAY_SYNC)
            .uint(SYNC_CALLBACK)
            .encode(),
    );
    stream.write_all(&out)?;
    let mut globals = Globals::default();
    loop {
        let msg = reader.next_blocking(stream)?;
        match (msg.object, msg.opcode) {
            (DISPLAY, op::DISPLAY_ERROR) => return Err(display_error(&msg).into()),
            (REGISTRY, op::REGISTRY_GLOBAL) => {
                let mut r = msg.reader();
                let (name, interface, version) = (r.uint()?, r.string()?, r.uint()?);
                match interface.as_str() {
                    SEAT_INTERFACE if globals.seat.is_none() => {
                        globals.seat = Some((name, version));
                    }
                    MANAGER_INTERFACE => globals.manager = Some((name, version)),
                    _ => {}
                }
            }
            (SYNC_CALLBACK, op::CALLBACK_DONE) => return Ok(globals),
            _ => {}
        }
    }
}

/// `wl_registry.bind` of an interface the compositor announced.
fn bind(name: u32, interface: &str, version: u32, id: u32) -> Vec<u8> {
    Message::new(REGISTRY, op::REGISTRY_BIND)
        .uint(name)
        .string(interface)
        .uint(version)
        .uint(id)
        .encode()
}

/// The protocol state of the worker thread, without any I/O.
struct Client {
    seat: Option<u32>,
    /// Every open window, by object id, as the events describe it so far.
    windows: BTreeMap<u32, Toplevel>,
    /// Requests waiting to be written.
    out: Vec<u8>,
}

/// What an event means for the loop.
#[derive(Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Finished,
}

impl Client {
    fn new(seat: Option<u32>) -> Client {
        Client {
            seat,
            windows: BTreeMap::new(),
            out: Vec::new(),
        }
    }

    fn request(&mut self, msg: &Message) {
        self.out.extend(msg.encode());
    }

    fn event(&mut self, msg: &Message, sink: &mut dyn FnMut(Update)) -> Result<Flow, WireError> {
        match (msg.object, msg.opcode) {
            (DISPLAY, op::DISPLAY_ERROR) => return Err(display_error(msg)),
            (MANAGER, op::MANAGER_TOPLEVEL) => {
                let id = msg.reader().uint()?;
                self.windows.insert(
                    id,
                    Toplevel {
                        id,
                        ..Toplevel::default()
                    },
                );
            }
            (MANAGER, op::MANAGER_FINISHED) => return Ok(Flow::Finished),
            (id, opcode) => self.handle_event(id, opcode, msg, sink)?,
        }
        Ok(Flow::Continue)
    }

    /// An event of a window (other objects are ignored).
    fn handle_event(
        &mut self,
        id: u32,
        opcode: u16,
        msg: &Message,
        sink: &mut dyn FnMut(Update),
    ) -> Result<(), WireError> {
        let Some(w) = self.windows.get_mut(&id) else {
            return Ok(());
        };
        match opcode {
            op::TITLE => w.title = msg.reader().string()?,
            op::APP_ID => w.app_id = msg.reader().string()?,
            op::STATE => w.state = State::from_array(msg.reader().array()?),
            op::DONE => sink(Update::Changed(w.clone())),
            op::CLOSED => {
                self.windows.remove(&id);
                self.request(&Message::new(id, op::DESTROY));
                sink(Update::Closed(id));
            }
            _ => {}
        }
        Ok(())
    }

    fn command(&mut self, cmd: Command) {
        let (Command::Activate(id)
        | Command::ToggleMinimize(id)
        | Command::ToggleMaximize(id)
        | Command::Close(id)) = cmd;
        let Some(state) = self.windows.get(&id).map(|w| w.state) else {
            return;
        };
        match cmd {
            Command::Activate(_) => self.activate(id, state),
            Command::ToggleMinimize(_) if state.minimized => self.activate(id, state),
            Command::ToggleMinimize(_) => self.request(&Message::new(id, op::SET_MINIMIZED)),
            Command::ToggleMaximize(_) => {
                let opcode = if state.maximized {
                    op::UNSET_MAXIMIZED
                } else {
                    op::SET_MAXIMIZED
                };
                self.request(&Message::new(id, opcode));
            }
            Command::Close(_) => self.request(&Message::new(id, op::CLOSE)),
        }
    }

    /// Restore a minimized window, then activate it (activation needs the seat).
    fn activate(&mut self, id: u32, state: State) {
        if state.minimized {
            self.request(&Message::new(id, op::UNSET_MINIMIZED));
        }
        if let Some(seat) = self.seat {
            self.request(&Message::new(id, op::ACTIVATE).uint(seat));
        }
    }

    /// Ask the compositor to stop sending; it answers with `finished`.
    fn stop(&mut self) {
        self.request(&Message::new(MANAGER, op::MANAGER_STOP));
    }
}

/// What the handle sends to the worker thread.
enum Request {
    Command(Command),
    Stop,
}

/// Wait until `a` or `b` (when given) can be read: `(a, b)`.
fn wait_readable(a: RawFd, b: Option<RawFd>) -> io::Result<(bool, bool)> {
    let ready = libc::POLLIN | libc::POLLHUP | libc::POLLERR;
    let mut fds = [
        libc::pollfd {
            fd: a,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            // a negative fd is ignored by poll(2)
            fd: b.unwrap_or(-1),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // SAFETY: `fds` is a valid array of two pollfd for the duration of the call.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if n >= 0 {
            return Ok((fds[0].revents & ready != 0, fds[1].revents & ready != 0));
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Read everything from the non-blocking wake-up socket: `false` once the handle is gone.
fn drain(wake: &mut UnixStream) -> bool {
    let mut buf = [0u8; 64];
    loop {
        match wake.read(&mut buf) {
            Ok(0) => return false,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return true,
        }
    }
}

/// The worker thread: bind, then report updates until the manager finishes.
fn run(
    mut stream: UnixStream,
    mut wake: UnixStream,
    requests: &mpsc::Receiver<Request>,
    sink: &mut dyn FnMut(Update),
) -> io::Result<()> {
    let mut reader = Reader::default();
    let globals = read_globals(&mut stream, &mut reader)?;
    let Some((manager_name, manager_version)) = globals.manager else {
        // GNOME, KWin: no list of windows, and nothing to report
        return Ok(());
    };
    let mut out = Vec::new();
    if let Some((name, _)) = globals.seat {
        out.extend(bind(name, SEAT_INTERFACE, 1, SEAT));
    }
    out.extend(bind(
        manager_name,
        MANAGER_INTERFACE,
        manager_version.min(MANAGER_VERSION),
        MANAGER,
    ));
    stream.write_all(&out)?;
    let mut client = Client::new(globals.seat.map(|_| SEAT));
    let result = serve(
        &mut stream,
        &mut wake,
        &mut reader,
        &mut client,
        requests,
        sink,
    );
    sink(Update::Finished);
    result
}

/// Forward requests and report events until the manager finishes or the stream ends.
fn serve(
    stream: &mut UnixStream,
    wake: &mut UnixStream,
    reader: &mut Reader,
    client: &mut Client,
    requests: &mpsc::Receiver<Request>,
    sink: &mut dyn FnMut(Update),
) -> io::Result<()> {
    let mut handle_alive = true;
    let mut stopping = false;
    loop {
        if !client.out.is_empty() {
            stream.write_all(&client.out)?;
            client.out.clear();
        }
        let wake_fd = handle_alive.then(|| wake.as_raw_fd());
        let (socket_ready, wake_ready) = wait_readable(stream.as_raw_fd(), wake_fd)?;
        if wake_ready {
            handle_alive = drain(wake);
            let mut stop = !handle_alive;
            while let Ok(req) = requests.try_recv() {
                match req {
                    Request::Command(cmd) => client.command(cmd),
                    Request::Stop => stop = true,
                }
            }
            if stop && !stopping {
                stopping = true;
                client.stop();
            }
        }
        if socket_ready {
            if reader.fill(stream)? == 0 {
                return Ok(());
            }
            while let Some(msg) = reader.next()? {
                if client.event(&msg, sink)? == Flow::Finished {
                    return Ok(());
                }
            }
        }
    }
}

/// The socket path of the compositor: `$WAYLAND_DISPLAY` (default `wayland-0`), relative to
/// `$XDG_RUNTIME_DIR` unless absolute.
pub fn socket_path(display: Option<OsString>, runtime_dir: Option<OsString>) -> Option<PathBuf> {
    let display = PathBuf::from(display.unwrap_or_else(|| "wayland-0".into()));
    if display.is_absolute() {
        return Some(display);
    }
    let runtime = runtime_dir.filter(|d| !d.is_empty())?;
    Some(Path::new(&runtime).join(display))
}

/// The handle of the worker thread. Dropping it stops the thread (without waiting).
pub struct Toplevels {
    requests: mpsc::Sender<Request>,
    wake: UnixStream,
    thread: Option<thread::JoinHandle<()>>,
}

impl Toplevels {
    /// Connect to the session's compositor. `None` when it cannot be reached; when it has no
    /// toplevel manager, the thread ends at once and `sink` gets no update.
    pub fn start(sink: impl FnMut(Update) + Send + 'static) -> Option<Toplevels> {
        let path = socket_path(
            std::env::var_os("WAYLAND_DISPLAY"),
            std::env::var_os("XDG_RUNTIME_DIR"),
        )?;
        let stream = UnixStream::connect(path).ok()?;
        Toplevels::start_on(stream, sink).ok()
    }

    /// Run the client on a connected `stream` (the compositor, or a test's mock).
    pub fn start_on(
        stream: UnixStream,
        mut sink: impl FnMut(Update) + Send + 'static,
    ) -> io::Result<Toplevels> {
        let (wake, worker_wake) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        worker_wake.set_nonblocking(true)?;
        let (requests, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("desk-shell-toplevels".into())
            .spawn(move || {
                if let Err(e) = run(stream, worker_wake, &receiver, &mut sink) {
                    eprintln!("desk-shell: window list: {e}");
                }
            })?;
        Ok(Toplevels {
            requests,
            wake,
            thread: Some(thread),
        })
    }

    fn post(&self, req: Request) {
        if self.requests.send(req).is_ok() {
            // a full socket already wakes the thread; a closed one means it ended
            let _ = (&self.wake).write(&[1]);
        }
    }

    /// Ask for `cmd` (dropped when the window is gone or the thread ended).
    pub fn send(&self, cmd: Command) {
        self.post(Request::Command(cmd));
    }

    /// Stop the manager and wait for the thread to end (the shell itself only drops the
    /// handle, at exit).
    #[cfg(test)]
    pub fn stop(mut self) {
        self.post(Request::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Toplevels {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.post(Request::Stop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_and_arrays_are_padded_to_four_bytes() {
        let m = Message::new(7, 2).string("abc");
        assert_eq!(m.args[..4], 4u32.to_ne_bytes());
        assert_eq!(m.args[4..], *b"abc\0");
        let m = Message::new(7, 2).string("abcd");
        assert_eq!(m.args.len(), 4 + 8);
        assert_eq!(m.args[..4], 5u32.to_ne_bytes());
        assert_eq!(Message::new(7, 2).string("").args.len(), 8);
        assert_eq!(Message::new(7, 2).array(&[]).args, 0u32.to_ne_bytes());
        // the reader skips the padding and drops the closing NUL
        let m = Message::new(7, 2)
            .string("Files — «Документи»")
            .uint(42)
            .array(&[1, 2, 3])
            .string("");
        let mut r = m.reader();
        assert_eq!(r.string().unwrap(), "Files — «Документи»");
        assert_eq!(r.uint().unwrap(), 42);
        assert_eq!(r.array().unwrap(), [1, 2, 3]);
        assert_eq!(r.string().unwrap(), "");
        assert_eq!(r.uint(), Err(WireError::Truncated));
    }

    #[test]
    fn a_null_or_broken_string_does_not_fail() {
        // a null string has length 0
        let null = Message::new(1, 0).uint(0);
        assert_eq!(null.reader().string().unwrap(), "");
        let bad = Message::new(1, 0).array(&[0xff, b'x', 0]);
        assert_eq!(bad.reader().string().unwrap(), "\u{fffd}x");
        // a length past the end is an error, not a panic
        let short = Message::new(1, 0).uint(100).uint(0);
        assert_eq!(short.reader().array(), Err(WireError::Truncated));
    }

    #[test]
    fn messages_round_trip_and_wait_for_all_their_bytes() {
        let m = Message::new(0xff00_0001, op::STATE).array(
            &State {
                activated: true,
                ..State::default()
            }
            .to_array(),
        );
        let wire = m.encode();
        assert_eq!(wire.len(), 8 + 4 + 4);
        let size = u32::from_ne_bytes([wire[4], wire[5], wire[6], wire[7]]) >> 16;
        assert_eq!(size as usize, wire.len());
        for cut in 0..wire.len() {
            assert_eq!(Message::parse(&wire[..cut]), Ok(None), "{cut}");
        }
        let mut two = wire.clone();
        two.extend(Message::new(5, op::MANAGER_FINISHED).encode());
        let (first, used) = Message::parse(&two).unwrap().unwrap();
        assert_eq!((first, used), (m, wire.len()));
        let (second, _) = Message::parse(&two[used..]).unwrap().unwrap();
        assert_eq!((second.object, second.opcode), (5, op::MANAGER_FINISHED));
    }

    #[test]
    fn a_bad_size_is_an_error() {
        for size in [0u32, 4, 10] {
            let mut wire = 1u32.to_ne_bytes().to_vec();
            wire.extend((size << 16).to_ne_bytes());
            wire.resize(16, 0);
            assert_eq!(
                Message::parse(&wire),
                Err(WireError::BadSize(size as usize))
            );
        }
    }

    #[test]
    fn state_arrays_decode_and_skip_unknown_values() {
        let all = State {
            maximized: true,
            minimized: true,
            activated: true,
            fullscreen: true,
        };
        assert_eq!(State::from_array(&all.to_array()), all);
        assert_eq!(State::from_array(&[]), State::default());
        let mut bytes = 99u32.to_ne_bytes().to_vec();
        bytes.extend(STATE_ACTIVATED.to_ne_bytes());
        bytes.push(7); // a stray byte is ignored
        assert_eq!(
            State::from_array(&bytes),
            State {
                activated: true,
                ..State::default()
            }
        );
    }

    fn client_with(state: State) -> Client {
        let mut c = Client::new(Some(SEAT));
        c.windows.insert(
            9,
            Toplevel {
                id: 9,
                state,
                ..Toplevel::default()
            },
        );
        c
    }

    fn requests(c: &mut Client) -> Vec<(u32, u16, Vec<u8>)> {
        let mut reader = Reader {
            buf: std::mem::take(&mut c.out),
        };
        let mut out = Vec::new();
        while let Some(m) = reader.next().unwrap() {
            out.push((m.object, m.opcode, m.args));
        }
        out
    }

    #[test]
    fn commands_become_requests() {
        let seat = SEAT.to_ne_bytes().to_vec();
        let mut c = client_with(State::default());
        c.command(Command::ToggleMinimize(9));
        c.command(Command::ToggleMaximize(9));
        c.command(Command::Activate(9));
        c.command(Command::Close(9));
        assert_eq!(
            requests(&mut c),
            [
                (9, op::SET_MINIMIZED, vec![]),
                (9, op::SET_MAXIMIZED, vec![]),
                (9, op::ACTIVATE, seat.clone()),
                (9, op::CLOSE, vec![]),
            ]
        );
        let mut c = client_with(State {
            minimized: true,
            maximized: true,
            ..State::default()
        });
        c.command(Command::ToggleMinimize(9));
        c.command(Command::ToggleMaximize(9));
        assert_eq!(
            requests(&mut c),
            [
                (9, op::UNSET_MINIMIZED, vec![]),
                (9, op::ACTIVATE, seat),
                (9, op::UNSET_MAXIMIZED, vec![]),
            ]
        );
        // an unknown window, and activation without a seat, send nothing
        c.command(Command::Close(10));
        assert!(requests(&mut c).is_empty());
        let mut c = client_with(State::default());
        c.seat = None;
        c.command(Command::Activate(9));
        assert!(requests(&mut c).is_empty());
    }

    #[test]
    fn the_socket_path_follows_wayland_display() {
        let p = |d: Option<&str>, r: Option<&str>| {
            socket_path(d.map(OsString::from), r.map(OsString::from))
        };
        assert_eq!(
            p(Some("wayland-1"), Some("/run/user/1000")),
            Some(PathBuf::from("/run/user/1000/wayland-1"))
        );
        assert_eq!(
            p(None, Some("/run/user/1000")),
            Some(PathBuf::from("/run/user/1000/wayland-0"))
        );
        assert_eq!(p(Some("/tmp/w"), None), Some(PathBuf::from("/tmp/w")));
        assert_eq!(p(Some("wayland-1"), None), None);
        assert_eq!(p(Some("wayland-1"), Some("")), None);
    }

    /// The compositor side of a test: reads requests, writes events.
    struct Mock {
        stream: UnixStream,
        reader: Reader,
    }

    impl Mock {
        fn expect(&mut self, object: u32, opcode: u16) -> Message {
            let m = self.reader.next_blocking(&mut self.stream).unwrap();
            assert_eq!((m.object, m.opcode), (object, opcode), "{m:?}");
            m
        }

        fn send(&mut self, msgs: &[Message]) {
            let bytes: Vec<u8> = msgs.iter().flat_map(Message::encode).collect();
            self.stream.write_all(&bytes).unwrap();
        }

        /// Answer the registry roundtrip with `globals` (`name`, interface, version).
        fn globals(&mut self, globals: &[(u32, &str, u32)]) {
            assert_eq!(
                self.expect(DISPLAY, op::DISPLAY_GET_REGISTRY).args,
                REGISTRY.to_ne_bytes()
            );
            let sync = self
                .expect(DISPLAY, op::DISPLAY_SYNC)
                .reader()
                .uint()
                .unwrap();
            let mut events: Vec<Message> = globals
                .iter()
                .map(|(name, iface, version)| {
                    Message::new(REGISTRY, op::REGISTRY_GLOBAL)
                        .uint(*name)
                        .string(iface)
                        .uint(*version)
                })
                .collect();
            events.push(Message::new(sync, op::CALLBACK_DONE).uint(1));
            // wl_display.delete_id of the callback: ignored by the client
            events.push(Message::new(DISPLAY, 1).uint(sync));
            self.send(&events);
        }

        fn expect_bind(&mut self, name: u32, iface: &str, version: u32, id: u32) {
            let m = self.expect(REGISTRY, op::REGISTRY_BIND);
            let mut r = m.reader();
            assert_eq!(
                (r.uint().unwrap(), r.string().unwrap().as_str()),
                (name, iface)
            );
            assert_eq!((r.uint().unwrap(), r.uint().unwrap()), (version, id));
        }
    }

    fn start() -> (Mock, Toplevels, mpsc::Receiver<Update>) {
        let (server, client) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel();
        let tl = Toplevels::start_on(client, move |u| tx.send(u).unwrap()).unwrap();
        let mock = Mock {
            stream: server,
            reader: Reader::default(),
        };
        (mock, tl, rx)
    }

    #[test]
    fn a_window_opens_changes_is_activated_closes_and_the_client_stops() {
        let (mut mock, tl, rx) = start();
        mock.globals(&[
            (1, "wl_compositor", 6),
            (2, SEAT_INTERFACE, 9),
            (3, MANAGER_INTERFACE, 7),
        ]);
        // the seat at v1 (only activation needs it), the manager at the version this client knows
        mock.expect_bind(2, SEAT_INTERFACE, 1, SEAT);
        mock.expect_bind(3, MANAGER_INTERFACE, MANAGER_VERSION, MANAGER);
        let win = 0xff00_0000;
        let active = State {
            activated: true,
            ..State::default()
        };
        mock.send(&[
            Message::new(MANAGER, op::MANAGER_TOPLEVEL).uint(win),
            Message::new(win, op::TITLE).string("Home"),
            Message::new(win, op::APP_ID).string("org.gnome.Nautilus"),
            Message::new(win, op::STATE).array(&active.to_array()),
            Message::new(win, op::DONE),
        ]);
        let mut want = Toplevel {
            id: win,
            title: "Home".into(),
            app_id: "org.gnome.Nautilus".into(),
            state: active,
        };
        assert_eq!(rx.recv().unwrap(), Update::Changed(want.clone()));

        let minimized = State {
            minimized: true,
            ..State::default()
        };
        mock.send(&[
            Message::new(win, op::STATE).array(&minimized.to_array()),
            Message::new(win, op::TITLE).string("Documents"),
            Message::new(win, op::DONE),
        ]);
        want.state = minimized;
        want.title = "Documents".into();
        assert_eq!(rx.recv().unwrap(), Update::Changed(want));

        // activating a minimized window restores it first
        tl.send(Command::Activate(win));
        mock.expect(win, op::UNSET_MINIMIZED);
        let seat = mock.expect(win, op::ACTIVATE).reader().uint().unwrap();
        assert_eq!(seat, SEAT);
        tl.send(Command::Close(win));
        mock.expect(win, op::CLOSE);

        mock.send(&[Message::new(win, op::CLOSED)]);
        mock.expect(win, op::DESTROY);
        assert_eq!(rx.recv().unwrap(), Update::Closed(win));
        // a command for the closed window sends nothing; the stop request comes next
        tl.send(Command::Close(win));

        let stopper = thread::spawn(move || tl.stop());
        mock.expect(MANAGER, op::MANAGER_STOP);
        mock.send(&[Message::new(MANAGER, op::MANAGER_FINISHED)]);
        assert_eq!(rx.recv().unwrap(), Update::Finished);
        stopper.join().unwrap();
        // the thread ended and dropped its end of the update channel
        assert!(rx.recv().is_err());
    }

    #[test]
    fn without_the_manager_the_thread_ends_silently() {
        let (mut mock, tl, rx) = start();
        mock.globals(&[(1, SEAT_INTERFACE, 9), (2, "wl_output", 4)]);
        // no update at all, then the channel closes when the thread ends
        assert!(rx.recv().is_err());
        tl.stop();
        // nothing was bound
        let mut rest = Vec::new();
        mock.stream.read_to_end(&mut rest).unwrap();
        assert!(rest.is_empty());
    }

    #[test]
    fn a_protocol_error_or_a_lost_compositor_ends_the_updates() {
        let (mut mock, tl, rx) = start();
        mock.globals(&[(3, MANAGER_INTERFACE, 2)]);
        // an older manager is bound at its own version; no seat is bound
        mock.expect_bind(3, MANAGER_INTERFACE, 2, MANAGER);
        mock.send(&[Message::new(DISPLAY, op::DISPLAY_ERROR)
            .uint(MANAGER)
            .uint(1)
            .string("bad request")]);
        assert_eq!(rx.recv().unwrap(), Update::Finished);
        assert!(rx.recv().is_err());
        tl.stop();

        let (mut mock, tl, rx) = start();
        mock.globals(&[(3, MANAGER_INTERFACE, 3)]);
        mock.expect_bind(3, MANAGER_INTERFACE, 3, MANAGER);
        drop(mock);
        assert_eq!(rx.recv().unwrap(), Update::Finished);
        tl.stop();
    }
}
