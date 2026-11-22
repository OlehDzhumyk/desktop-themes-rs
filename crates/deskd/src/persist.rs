// SPDX-License-Identifier: GPL-3.0-or-later
//! Debounced saving (the clipboard history, the mixer memory): a burst of changes (copying many
//! times, pinning, deleting, dragging a slider) writes the file once, after a quiet period, instead
//! of once per change. Shutdown flushes
//! whatever is pending ([`Saver::flush`], called on SIGTERM/SIGINT/SIGHUP and when the main loop
//! ends), so nothing is lost on logout.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// Quiet period after the last change before the history is written.
pub const QUIET: Duration = Duration::from_secs(2);
/// Longest a change may wait, however busy it is (a crash loses at most this much).
pub const MAX_WAIT: Duration = Duration::from_secs(10);

/// When to save: pure, with the clock passed in, so that it needs no sleeping to test.
#[derive(Debug)]
pub struct Debounce {
    quiet: Duration,
    max_wait: Duration,
    /// (first change, last change) of the pending batch.
    pending: Option<(Instant, Instant)>,
}

impl Debounce {
    pub fn new(quiet: Duration, max_wait: Duration) -> Self {
        Self {
            quiet,
            max_wait,
            pending: None,
        }
    }

    /// A change happened at `now`.
    pub fn touch(&mut self, now: Instant) {
        self.pending = Some(match self.pending {
            Some((first, _)) => (first, now),
            None => (now, now),
        });
    }

    /// When the pending batch must be written, if there is one.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending
            .map(|(first, last)| (last + self.quiet).min(first + self.max_wait))
    }

    /// True (and the batch is closed) when it is time to write.
    pub fn take_due(&mut self, now: Instant) -> bool {
        match self.deadline() {
            Some(d) if d <= now => {
                self.pending = None;
                true
            }
            _ => false,
        }
    }

    /// True (and the batch is closed) when anything is pending, due or not: for shutdown.
    pub fn take_now(&mut self) -> bool {
        self.pending.take().is_some()
    }
}

enum Msg {
    Touch,
    Flush(Sender<()>),
}

/// Handle to the saving thread.
#[derive(Clone)]
pub struct Saver {
    tx: Sender<Msg>,
}

impl Saver {
    /// Start the thread that calls `save` after changes; it ends when every handle is dropped.
    pub fn spawn(save: impl Fn() + Send + 'static, quiet: Duration, max_wait: Duration) -> Saver {
        let (tx, rx) = mpsc::channel::<Msg>();
        std::thread::spawn(move || {
            let mut d = Debounce::new(quiet, max_wait);
            loop {
                let msg = match d.deadline() {
                    None => rx.recv().ok(),
                    Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now()))
                    {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => {
                            if d.take_due(Instant::now()) {
                                save();
                            }
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => None,
                    },
                };
                match msg {
                    Some(Msg::Touch) => d.touch(Instant::now()),
                    Some(Msg::Flush(ack)) => {
                        if d.take_now() {
                            save();
                        }
                        let _ = ack.send(());
                    }
                    None => {
                        if d.take_now() {
                            save();
                        }
                        break;
                    }
                }
            }
        });
        Saver { tx }
    }

    /// A saver that writes nothing (tests, and features with nothing to keep).
    pub fn noop() -> Saver {
        let (tx, _rx) = mpsc::channel();
        Saver { tx }
    }

    /// The history changed: write it soon.
    pub fn touch(&self) {
        let _ = self.tx.send(Msg::Touch);
    }

    /// Write now if anything is pending, and return once it is on disk.
    pub fn flush(&self) {
        let (ack, done) = mpsc::channel();
        if self.tx.send(Msg::Flush(ack)).is_ok() {
            let _ = done.recv();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn waits_for_a_quiet_period_but_not_forever() {
        let t0 = Instant::now();
        let mut d = Debounce::new(secs(2), secs(10));
        assert_eq!(d.deadline(), None);
        assert!(!d.take_due(t0));
        d.touch(t0);
        assert!(!d.take_due(t0 + secs(1)));
        // another change pushes the deadline back
        d.touch(t0 + secs(1));
        assert!(!d.take_due(t0 + secs(2)));
        assert!(d.take_due(t0 + secs(3)));
        // the batch is closed: nothing due until the next change
        assert!(!d.take_due(t0 + secs(100)));
        // constant activity cannot postpone the write beyond max_wait
        let t1 = t0 + secs(200);
        d.touch(t1);
        for i in 1..=9 {
            d.touch(t1 + secs(i));
        }
        assert_eq!(d.deadline(), Some(t1 + secs(10)));
        assert!(d.take_due(t1 + secs(10)));
    }

    #[test]
    fn shutdown_takes_a_pending_batch_even_when_not_due() {
        let t0 = Instant::now();
        let mut d = Debounce::new(secs(2), secs(10));
        assert!(!d.take_now());
        d.touch(t0);
        assert!(d.take_now());
        assert!(!d.take_now());
    }

    /// A change that is not due yet is on disk once `flush` returns (what SIGTERM does), and a
    /// flush without a change writes nothing.
    #[test]
    fn flush_writes_the_pending_history() {
        use crate::clipboard::{Config, Engine};
        use crate::service::{Shared, lock};
        use std::sync::{Arc, Mutex};

        let dir = std::env::temp_dir().join(format!("deskd-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = dir.join("data");
        let eng = Engine::new(Config::default(), data.clone(), dir.join("run"));
        let eng: Shared = Arc::new(Mutex::new(eng));
        // far-away deadlines: only the flush can write
        let e2 = eng.clone();
        let saver = Saver::spawn(move || lock(&e2).save().unwrap(), secs(3600), secs(3600));
        saver.flush();
        assert!(!data.join("history.json").exists());
        lock(&eng).push("text/plain;charset=utf-8", b"keep me", &[], "", 1);
        saver.touch();
        saver.flush();
        let saved = std::fs::read_to_string(data.join("history.json")).unwrap();
        assert!(saved.contains("keep me"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
