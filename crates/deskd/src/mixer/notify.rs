// SPDX-License-Identifier: GPL-3.0-or-later
//! Coalesced change notification: a burst of PipeWire events (a device appearing brings dozens,
//! a slider drag one per step) becomes one `Changed` signal after a short quiet period, never one
//! per event and never one per audio buffer. Nothing runs while nothing changes: the thread blocks
//! on its channel, and a deadline exists only while a change is pending or a paused stream is
//! about to leave the list.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::persist::Debounce;

/// Quiet period after the last event before `Changed` is sent.
pub const QUIET: Duration = Duration::from_millis(40);
/// Longest a change waits, however busy the graph is (a slider dragged for seconds).
pub const MAX_WAIT: Duration = Duration::from_millis(150);

/// Which changes are pending and when to report them: pure, the clock is passed in.
#[derive(Debug)]
pub struct Coalesce {
    debounce: Debounce,
    flags: u8,
}

impl Coalesce {
    pub fn new(quiet: Duration, max_wait: Duration) -> Self {
        Coalesce {
            debounce: Debounce::new(quiet, max_wait),
            flags: 0,
        }
    }

    pub fn touch(&mut self, flags: u8, now: Instant) {
        if flags == 0 {
            return;
        }
        self.flags |= flags;
        self.debounce.touch(now);
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.debounce.deadline()
    }

    /// The flags to report, when it is time.
    pub fn take_due(&mut self, now: Instant) -> Option<u8> {
        self.debounce
            .take_due(now)
            .then(|| std::mem::take(&mut self.flags))
    }
}

/// Handle to the notification thread.
#[derive(Clone)]
pub struct Notifier {
    tx: Sender<u8>,
}

impl Notifier {
    /// `expiry(now)`: when the application list changes by itself next (a paused stream leaves it);
    /// `emit(flags)`: send the signal.
    pub fn spawn(
        expiry: impl Fn(Instant) -> Option<Instant> + Send + 'static,
        emit: impl Fn(u8) + Send + 'static,
    ) -> Notifier {
        let (tx, rx) = mpsc::channel::<u8>();
        std::thread::Builder::new()
            .name("deskd-mixer-notify".into())
            .spawn(move || {
                let mut c = Coalesce::new(QUIET, MAX_WAIT);
                loop {
                    let now = Instant::now();
                    let next = [c.deadline(), expiry(now)].into_iter().flatten().min();
                    let msg = match next {
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                        Some(at) => rx.recv_timeout(at.saturating_duration_since(now)),
                    };
                    match msg {
                        Ok(flags) => c.touch(flags, Instant::now()),
                        Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {
                            let now = Instant::now();
                            if let Some(flags) = c.take_due(now) {
                                emit(flags);
                            } else if next.is_some_and(|at| at <= now) {
                                emit(super::model::dirty::APPS);
                            }
                        }
                    }
                }
            })
            .expect("spawn the mixer notification thread");
        Notifier { tx }
    }

    /// A notifier that drops everything (tests).
    #[cfg(test)]
    pub fn inert() -> Notifier {
        let (tx, _rx) = mpsc::channel();
        Notifier { tx }
    }

    pub fn touch(&self, flags: u8) {
        if flags != 0 {
            let _ = self.tx.send(flags);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::model::dirty;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Many events in a burst give one report with the union of what changed; a long drag is still
    /// reported every MAX_WAIT; nothing is due without events.
    #[test]
    fn bursts_become_one_report() {
        let t0 = Instant::now();
        let mut c = Coalesce::new(ms(40), ms(150));
        assert_eq!(c.deadline(), None);
        c.touch(0, t0);
        assert_eq!(
            c.deadline(),
            None,
            "an event that changed nothing is not pending"
        );
        for i in 0..20 {
            c.touch(dirty::DEVICES, t0 + ms(i));
        }
        c.touch(dirty::DEFAULTS, t0 + ms(20));
        assert_eq!(c.take_due(t0 + ms(59)), None);
        assert_eq!(
            c.take_due(t0 + ms(60)),
            Some(dirty::DEVICES | dirty::DEFAULTS)
        );
        assert_eq!(c.take_due(t0 + ms(1000)), None);
        // a drag: an event every 10 ms for a second
        let t1 = t0 + ms(2000);
        let mut reports = 0;
        for i in 0..100 {
            let now = t1 + ms(i * 10);
            if c.take_due(now).is_some() {
                reports += 1;
            }
            c.touch(dirty::APPS, now);
        }
        assert!((6..=7).contains(&reports), "{reports} reports in 1 s");
    }
}
