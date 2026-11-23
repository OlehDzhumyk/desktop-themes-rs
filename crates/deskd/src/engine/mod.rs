// SPDX-License-Identifier: GPL-3.0-or-later
//! The engine lifecycle of deskd. Every feature (clipboard, emoji, mixer, appearance, focus) is an
//! [`Engine`]; `main` only owns the bus, the main loop and the process signals.
//!
//! ```text
//! new (no bus yet) → register (connection builder) → start (name owned) → shutdown (loop left)
//! ```
//!
//! [`Engines`] starts the engines in list order and shuts them down in reverse order. The engines
//! drop (GSettings objects, watches) only after every engine has shut down.

pub mod appearance;
pub mod clipboard;
pub mod emoji;
pub mod focus;
pub mod mixer;

use std::sync::{Arc, OnceLock};

use zbus::connection::Builder;

use crate::trust::Gate;

/// The session bus connection, set once the bus name is owned. Threads that start before that
/// (the schedulers, the mixer notifier) read it to send signals, and send nothing while it is empty.
pub type LateBus = Arc<OnceLock<zbus::Connection>>;

/// What the engines share when they are made, before the bus connection exists.
pub struct Setup {
    pub bus: LateBus,
    /// Content and changes only for the shell and our binaries (ADR 0008).
    pub gate: Gate,
}

/// One feature engine of deskd.
pub trait Engine {
    /// Add the D-Bus interfaces of the engine to the connection builder.
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a>;

    /// Connect the watchers, timers and backends. The bus name is owned; this runs in the GLib
    /// main thread before the main loop runs.
    fn start(&mut self, ctx: &mut RuntimeCtx<'_>);

    /// Stop the background work and write the pending state. The main loop has stopped.
    fn shutdown(&mut self) {}
}

/// What [`Engine::start`] gets.
pub struct RuntimeCtx<'a> {
    /// The session bus connection; the bus name is owned.
    pub conn: &'a zbus::Connection,
    /// Callbacks for the system events that move wall-clock schedules.
    pub wake: &'a mut SystemWake,
}

/// The callbacks to run when the wall clock can have jumped (resume from suspend, a time zone
/// change). One system bus watcher ([`crate::schedule::watch_system_events`]) serves all engines.
#[derive(Default)]
pub struct SystemWake {
    hooks: Vec<Box<dyn Fn() + Send + Sync>>,
}

impl SystemWake {
    /// Run `f` on each system wake event.
    pub fn on_wake(&mut self, f: impl Fn() + Send + Sync + 'static) {
        self.hooks.push(Box::new(f));
    }

    /// Run all callbacks, in the order they were added.
    pub fn fire(&self) {
        for hook in &self.hooks {
            hook();
        }
    }

    /// Watch the system bus and run the callbacks on each event. Without callbacks, nothing.
    pub fn watch(self) {
        if self.hooks.is_empty() {
            return;
        }
        crate::schedule::watch_system_events(move || self.fire());
    }
}

/// The engines of this process, in start order.
pub struct Engines(Vec<Box<dyn Engine>>);

impl Engines {
    pub fn new(engines: Vec<Box<dyn Engine>>) -> Engines {
        Engines(engines)
    }

    /// Add the D-Bus interfaces of all engines, in start order.
    pub fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        self.0.iter_mut().fold(builder, |b, e| e.register(b))
    }

    /// Start all engines in order. The result collects their system wake callbacks.
    pub fn start(&mut self, conn: &zbus::Connection) -> SystemWake {
        let mut wake = SystemWake::default();
        let mut ctx = RuntimeCtx {
            conn,
            wake: &mut wake,
        };
        for e in &mut self.0 {
            e.start(&mut ctx);
        }
        wake
    }

    /// Shut all engines down in reverse start order, then drop them in start order.
    pub fn shutdown(mut self) {
        for e in self.0.iter_mut().rev() {
            e.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    type Log = Arc<Mutex<Vec<String>>>;

    /// An engine that writes each lifecycle step to a shared log.
    struct Probe {
        name: &'static str,
        log: Log,
    }

    impl Probe {
        fn boxed(name: &'static str, log: &Log) -> Box<dyn Engine> {
            Box::new(Probe {
                name,
                log: log.clone(),
            })
        }

        fn note(&self, step: &str) {
            self.log
                .lock()
                .unwrap()
                .push(format!("{step} {}", self.name));
        }
    }

    impl Engine for Probe {
        fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
            self.note("register");
            builder
        }

        fn start(&mut self, ctx: &mut RuntimeCtx<'_>) {
            self.note("start");
            let (name, log) = (self.name, self.log.clone());
            ctx.wake
                .on_wake(move || log.lock().unwrap().push(format!("wake {name}")));
        }

        fn shutdown(&mut self) {
            self.note("shutdown");
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.note("drop");
        }
    }

    fn take(log: &Log) -> Vec<String> {
        std::mem::take(&mut *log.lock().unwrap())
    }

    /// A private peer-to-peer connection: [`Engines::start`] needs one, no bus daemon does.
    async fn p2p_connection(engines: &mut Engines) -> (zbus::Connection, zbus::Connection) {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = engines.register(Builder::unix_stream(a).server(zbus::Guid::generate()).p2p());
        let client = Builder::unix_stream(b).p2p().build();
        futures_util::future::try_join(server.build(), client)
            .await
            .unwrap()
    }

    #[test]
    fn engines_start_in_order_and_shut_down_in_reverse() {
        let log = Log::default();
        let mut engines = Engines::new(vec![
            Probe::boxed("a", &log),
            Probe::boxed("b", &log),
            Probe::boxed("c", &log),
        ]);
        let (conn, _client) = zbus::block_on(p2p_connection(&mut engines));
        assert_eq!(take(&log), ["register a", "register b", "register c"]);

        let wake = engines.start(&conn);
        assert_eq!(take(&log), ["start a", "start b", "start c"]);

        engines.shutdown();
        let steps = take(&log);
        assert_eq!(
            steps,
            [
                "shutdown c",
                "shutdown b",
                "shutdown a",
                "drop a",
                "drop b",
                "drop c"
            ]
        );
        drop(wake);
    }

    #[test]
    fn system_wake_runs_every_callback_once_per_event() {
        let log = Log::default();
        let mut engines = Engines::new(vec![Probe::boxed("a", &log), Probe::boxed("b", &log)]);
        let (conn, _client) = zbus::block_on(p2p_connection(&mut engines));
        let wake = engines.start(&conn);
        take(&log);

        wake.fire();
        assert_eq!(take(&log), ["wake a", "wake b"]);
        wake.fire();
        assert_eq!(take(&log), ["wake a", "wake b"]);
        engines.shutdown();
    }

    #[test]
    fn system_wake_without_callbacks_does_nothing() {
        let wake = SystemWake::default();
        wake.fire();
        // no callbacks: no system bus watcher thread starts
        wake.watch();
    }
}
