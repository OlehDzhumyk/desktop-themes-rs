// SPDX-License-Identifier: GPL-3.0-or-later
//! The volume mixer engine (`io.github.OlehDzhumyk.Desk.Mixer`, docs/spec/features/mixer.md).
//!
//! * [`model`]: the audio graph and every rule, pure (tested with mocked graphs);
//! * [`pw`]: the PipeWire connection on its own thread (cargo feature `pipewire`, on by default);
//! * [`notify`]: coalesced `Changed` signals;
//! * [`service`]: the D-Bus interface;
//! * [`store`]: the per-application volume memory;
//! * [`icons`]: application icons from desktop entries.
//!
//! Event-driven only: the PipeWire thread sleeps in its loop until the server sends something, the
//! notification thread until there is something to report. No audio data is read.

pub mod icons;
pub mod model;
pub mod notify;
#[cfg(feature = "pipewire")]
pub mod pw;
pub mod service;
pub mod store;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use model::{Graph, MixError, Op};
use notify::Notifier;

use crate::persist::Saver;

pub type SharedGraph = Arc<Mutex<Graph>>;

pub fn lock(g: &SharedGraph) -> MutexGuard<'_, Graph> {
    g.lock().unwrap_or_else(|p| p.into_inner())
}

/// Carries out [`Op`]s on the audio server.
pub trait Backend: Send {
    fn exec(&self, ops: Vec<Op>);
    /// Still connected or connecting (false once its thread has ended).
    fn alive(&self) -> bool {
        true
    }
}

/// The engine: the graph, the connection that feeds it, and the side effects of requests.
pub struct Mixer {
    pub graph: SharedGraph,
    backend: Mutex<Option<Box<dyn Backend>>>,
    pub notifier: Notifier,
    saver: Saver,
    /// Called when a client asks while there is no connection (connect on demand).
    reconnect: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Mixer {
    pub fn new(graph: SharedGraph, notifier: Notifier, store: Option<PathBuf>) -> Mixer {
        let saver = match store {
            Some(path) => {
                let g = graph.clone();
                Saver::spawn(
                    move || {
                        let memory = lock(&g).memory.clone();
                        if let Err(e) = store::save(&path, &memory) {
                            eprintln!("deskd: cannot save the mixer memory: {e}");
                        }
                    },
                    std::time::Duration::from_secs(2),
                    std::time::Duration::from_secs(10),
                )
            }
            None => Saver::noop(),
        };
        Mixer {
            graph,
            backend: Mutex::new(None),
            notifier,
            saver,
            reconnect: Mutex::new(None),
        }
    }

    pub fn on_reconnect(&self, f: impl Fn() + Send + 'static) {
        *self.reconnect.lock().unwrap_or_else(|p| p.into_inner()) = Some(Box::new(f));
    }

    /// A client is looking: if the server is not connected, try again (it may have started since).
    pub fn wanted(&self) {
        if lock(&self.graph).connected {
            return;
        }
        if let Some(f) = &*self.reconnect.lock().unwrap_or_else(|p| p.into_inner()) {
            f();
        }
    }

    /// Connect (or reconnect) through `backend`; `None` disconnects.
    pub fn set_backend(&self, backend: Option<Box<dyn Backend>>) {
        let old = std::mem::replace(
            &mut *self.backend.lock().unwrap_or_else(|p| p.into_inner()),
            backend,
        );
        // dropping a PipeWire backend joins its thread: outside the lock
        drop(old);
    }

    /// The mixer is switched on (it has a backend, live or ended).
    pub fn has_backend(&self) -> bool {
        self.backend
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// A backend is connected or still connecting: never replace it (its end would reset the
    /// graph under the new one).
    pub fn backend_alive(&self) -> bool {
        self.backend
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|b| b.alive())
    }

    fn exec(&self, ops: Vec<Op>) -> Result<(), MixError> {
        if ops.is_empty() {
            return Ok(());
        }
        match &*self.backend.lock().unwrap_or_else(|p| p.into_inner()) {
            Some(b) => {
                b.exec(ops);
                Ok(())
            }
            None => Err(MixError::Unavailable),
        }
    }

    /// Plan a request on the graph, then carry it out; save the memory when it changed.
    pub fn request(
        &self,
        plan: impl FnOnce(&mut Graph) -> Result<(Vec<Op>, bool), MixError>,
    ) -> Result<(), MixError> {
        self.wanted();
        let (ops, remembered) = plan(&mut lock(&self.graph))?;
        if remembered {
            self.saver.touch();
        }
        self.exec(ops)
    }

    /// Forget every remembered application volume (and delete the file, soon).
    pub fn forget(&self) {
        let had = {
            let mut g = lock(&self.graph);
            let had = !g.memory.is_empty();
            g.memory.clear();
            had
        };
        if had {
            self.saver.touch();
        }
    }

    /// Write a pending memory change now (shutdown).
    pub fn flush(&self) {
        self.saver.flush();
    }
}

#[cfg(test)]
pub mod testing {
    //! A backend that records ops and plays the server: it applies them to the graph as PipeWire
    //! would echo them back (volumes, mute, metadata).
    use super::*;

    pub struct Echo {
        pub graph: SharedGraph,
        pub log: Arc<Mutex<Vec<Op>>>,
    }

    impl Backend for Echo {
        fn exec(&self, ops: Vec<Op>) {
            let mut g = lock(&self.graph);
            for op in &ops {
                match op.clone() {
                    Op::NodeVolumes { node, volumes } => {
                        g.node_params(node, Some(volumes), None);
                    }
                    Op::NodeMute { node, mute } => {
                        g.node_params(node, None, Some(mute));
                    }
                    Op::Metadata {
                        subject,
                        key,
                        value,
                        ..
                    } => {
                        // the session manager answers a configured default with the effective one
                        let key = key.replace("default.configured.", "default.");
                        g.metadata(subject, Some(&key), value.as_deref());
                    }
                    Op::Route { .. } => {}
                }
            }
            self.log.lock().unwrap().extend(ops);
        }
    }
}
