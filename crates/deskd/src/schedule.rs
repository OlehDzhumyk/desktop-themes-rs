// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared one-shot wake scheduler for time-driven `deskd` engines (automatic light/dark,
//! focus schedules).
//!
//! No polling loops: the worker thread sleeps on a [`Condvar`] until either the next scheduled
//! transition deadline arrives or an external trigger (settings change, D-Bus `Recompute`,
//! system resume from suspend, timezone change, shutdown) wakes it immediately.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use futures_util::StreamExt;

#[derive(Default)]
struct State {
    next_wake: Option<Instant>,
    dirty: bool,
    shutdown: bool,
}

/// Handle to a one-shot wake scheduler thread.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<(Mutex<State>, Condvar)>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Scheduler {
    /// Spawn a scheduler thread named `name`.
    ///
    /// `on_wake` is called once immediately after start, whenever [`Scheduler::trigger`] is
    /// called, and whenever the previously returned delay elapses. Returning `None` disarms the
    /// timer (zero idle wakeups until triggered again).
    pub fn spawn(name: &str, on_wake: impl Fn() -> Option<Duration> + Send + 'static) -> Scheduler {
        let inner = Arc::new((
            Mutex::new(State {
                next_wake: None,
                dirty: true,
                shutdown: false,
            }),
            Condvar::new(),
        ));
        let worker_inner = inner.clone();
        let label = name.to_string();
        let handle = std::thread::Builder::new()
            .name(label.clone())
            .spawn(move || run_loop(&label, &worker_inner, on_wake));
        let thread = match handle {
            Ok(h) => Some(h),
            Err(e) => {
                eprintln!("deskd: {name}: cannot start scheduler thread: {e}");
                None
            }
        };
        Scheduler {
            inner,
            thread: Arc::new(Mutex::new(thread)),
        }
    }

    /// A scheduler that runs no background thread (for unit tests).
    #[cfg(test)]
    pub fn inert() -> Scheduler {
        Scheduler {
            inner: Arc::new((Mutex::new(State::default()), Condvar::new())),
            thread: Arc::new(Mutex::new(None)),
        }
    }

    /// Wake the scheduler immediately to recompute and re-arm its timer.
    pub fn trigger(&self) {
        let (lock, cvar) = &*self.inner;
        let mut st = lock.lock().unwrap_or_else(|p| p.into_inner());
        st.dirty = true;
        cvar.notify_all();
    }

    /// Stop the scheduler thread cleanly and wait for it to exit.
    pub fn shutdown(&self) {
        let (lock, cvar) = &*self.inner;
        {
            let mut st = lock.lock().unwrap_or_else(|p| p.into_inner());
            st.shutdown = true;
            cvar.notify_all();
        }
        let handle = self.thread.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

fn invoke_safe(name: &str, on_wake: &impl Fn() -> Option<Duration>) -> Option<Duration> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(on_wake)) {
        Ok(dur) => dur,
        Err(_) => {
            eprintln!("deskd: {name}: scheduler callback panicked");
            None
        }
    }
}

fn run_loop(name: &str, inner: &(Mutex<State>, Condvar), on_wake: impl Fn() -> Option<Duration>) {
    let (lock, cvar) = inner;
    loop {
        let should_run = {
            let mut st = lock.lock().unwrap_or_else(|p| p.into_inner());
            if st.shutdown {
                break;
            }
            if st.dirty {
                st.dirty = false;
                true
            } else if let Some(deadline) = st.next_wake {
                let now = Instant::now();
                if deadline <= now {
                    st.next_wake = None;
                    true
                } else {
                    let wait = deadline.saturating_duration_since(now);
                    let (mut guard, res) = cvar
                        .wait_timeout(st, wait)
                        .unwrap_or_else(|p| p.into_inner());
                    if guard.shutdown {
                        break;
                    }
                    if guard.dirty {
                        guard.dirty = false;
                        true
                    } else if res.timed_out()
                        || guard.next_wake.is_some_and(|d| d <= Instant::now())
                    {
                        guard.next_wake = None;
                        true
                    } else {
                        false
                    }
                }
            } else {
                let _guard = cvar.wait(st).unwrap_or_else(|p| p.into_inner());
                false
            }
        };

        if should_run {
            let delay = invoke_safe(name, &on_wake);
            let mut st = lock.lock().unwrap_or_else(|p| p.into_inner());
            st.next_wake = delay.map(|d| Instant::now() + d);
        }
    }
}

/// Watch system-bus events that affect wall-clock schedules:
/// - `org.freedesktop.login1.Manager.PrepareForSleep(false)` (resume from suspend)
/// - `org.freedesktop.timedate1` `Timezone` property change
///
/// Fails gracefully without panicking when the system bus is unavailable (containers, tests).
pub fn watch_system_events(on_event: impl Fn() + Send + Sync + 'static) {
    let spawned = std::thread::Builder::new()
        .name("deskd-sys-watch".into())
        .spawn(move || {
            zbus::block_on(async move {
                let Ok(conn) = zbus::Connection::system().await else {
                    return;
                };
                let cb = Arc::new(on_event);
                futures_util::future::join(
                    watch_prepare_for_sleep(&conn, cb.clone()),
                    watch_timezone_changes(&conn, cb),
                )
                .await;
            });
        });
    if let Err(e) = spawned {
        eprintln!("deskd: cannot start system event watcher: {e}");
    }
}

async fn watch_prepare_for_sleep(conn: &zbus::Connection, on_event: Arc<dyn Fn() + Send + Sync>) {
    let Ok(proxy) = zbus::Proxy::new(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    else {
        return;
    };
    let Ok(mut stream) = proxy.receive_signal("PrepareForSleep").await else {
        return;
    };
    while let Some(sig) = stream.next().await {
        if sig.body().deserialize::<bool>().is_ok_and(|going| !going) {
            on_event();
        }
    }
}

async fn watch_timezone_changes(conn: &zbus::Connection, on_event: Arc<dyn Fn() + Send + Sync>) {
    let Ok(proxy) = zbus::Proxy::new(
        conn,
        "org.freedesktop.timedate1",
        "/org/freedesktop/timedate1",
        "org.freedesktop.DBus.Properties",
    )
    .await
    else {
        return;
    };
    let Ok(mut stream) = proxy.receive_signal("PropertiesChanged").await else {
        return;
    };
    while let Some(sig) = stream.next().await {
        if let Ok((_iface, changed, _invalidated)) =
            sig.body()
                .deserialize::<(String, HashMap<String, zbus::OwnedVariant>, Vec<String>)>()
            && changed.contains_key("Timezone")
        {
            on_event();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn trigger_wakes_immediately_and_shutdown_exits_cleanly() {
        let (tx, rx) = mpsc::channel::<u32>();
        let count = Arc::new(Mutex::new(0u32));
        let c2 = count.clone();
        let sched = Scheduler::spawn("test-sched", move || {
            let mut n = c2.lock().unwrap();
            *n += 1;
            let _ = tx.send(*n);
            // Return None: disarmed, zero idle wakeups.
            None
        });
        assert_eq!(rx.recv().unwrap(), 1);
        sched.trigger();
        assert_eq!(rx.recv().unwrap(), 2);
        sched.trigger();
        assert_eq!(rx.recv().unwrap(), 3);
        sched.shutdown();
    }
}
