// SPDX-License-Identifier: GPL-3.0-or-later
//! The volume mixer engine: the graph, the coalesced `Changed` signal, the PipeWire backend with
//! its reconnects, and the GSettings watcher.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use desk_proto::MIXER_PATH;
use desk_settings::accessors::MixerSettings;
use gio::glib;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::mixer::{self, Mixer};
use crate::settings;
use crate::trust::Gate;

pub struct MixerFeature {
    mixer: Arc<Mixer>,
    gate: Gate,
    /// The `enabled` setting at start-up.
    on: bool,
    settings: Option<MixerSettings>,
}

impl MixerFeature {
    /// Load the remembered volumes and start the notifier; it sends `Changed` once the bus exists.
    pub fn new(setup: &Setup) -> MixerFeature {
        let settings = settings::mixer_settings();
        let (on, cfg) = settings
            .as_ref()
            .map_or((true, Default::default()), settings::read_mixer);
        let store = mixer::store::default_path();
        let mut graph = mixer::model::Graph::new(cfg, {
            let dirs = mixer::icons::data_dirs();
            Box::new(move |c: &[String]| mixer::icons::resolve(&dirs, c))
        });
        graph.memory = mixer::store::load(&store);
        let graph = Arc::new(Mutex::new(graph));
        let notifier = mixer::notify::Notifier::spawn(
            {
                let g = graph.clone();
                move |now| mixer::lock(&g).next_expiry(now)
            },
            {
                let bus = setup.bus.clone();
                move |flags| {
                    if let Some(conn) = bus.get() {
                        zbus::block_on(mixer::service::emit(conn, flags));
                    }
                }
            },
        );
        MixerFeature {
            mixer: Arc::new(Mixer::new(graph, notifier, Some(store))),
            gate: setup.gate.clone(),
            on,
            settings,
        }
    }
}

impl Engine for MixerFeature {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            MIXER_PATH,
            mixer::service::MixerIface {
                mixer: self.mixer.clone(),
                gate: self.gate.clone(),
            },
        )
    }

    fn start(&mut self, _ctx: &mut RuntimeCtx<'_>) {
        if self.on {
            start_mixer(&self.mixer);
        }
        reconnect_on_request(&self.mixer);
        if let Some(s) = &self.settings {
            watch_settings(&self.mixer, s);
        }
    }

    fn shutdown(&mut self) {
        self.mixer.set_backend(None);
        self.mixer.flush();
    }
}

/// A client asked while disconnected: try again, at most every few seconds.
fn reconnect_on_request(mixer: &Arc<Mixer>) {
    let weak = Arc::downgrade(mixer);
    let last: Arc<Mutex<Option<Instant>>> = Arc::default();
    mixer.on_reconnect(move || {
        let now = Instant::now();
        let mut l = last.lock().unwrap_or_else(|p| p.into_inner());
        if l.is_some_and(|t| now.duration_since(t) < Duration::from_secs(5)) {
            return;
        }
        *l = Some(now);
        let weak = weak.clone();
        glib::MainContext::default().invoke(move || {
            if let Some(m) = weak.upgrade()
                && m.has_backend()
                && !mixer::lock(&m.graph).connected
            {
                start_mixer(&m);
            }
        });
    });
}

fn watch_settings(mixer: &Arc<Mixer>, s: &MixerSettings) {
    let m = mixer.clone();
    let typed = s.clone();
    s.0.connect_changed(None, move |_, key| {
        let (on, cfg) = settings::read_mixer(&typed);
        match key {
            "enabled" if on => start_mixer(&m),
            "enabled" => m.set_backend(None),
            "remember-app-volume" | "hide-apps" => {
                let forget = !cfg.remember;
                mixer::lock(&m.graph).cfg = cfg;
                if forget {
                    m.forget();
                }
                m.notifier.touch(mixer::model::dirty::APPS);
            }
            _ => {}
        }
    });
}

/// Connect the mixer to PipeWire (again). After a lost connection one retry follows two seconds
/// later; after that, the next client request tries (see [`Mixer::wanted`]).
#[cfg(feature = "pipewire")]
fn start_mixer(m: &Arc<Mixer>) {
    if m.backend_alive() {
        return;
    }
    let weak = Arc::downgrade(m);
    let backend = mixer::pw::PwBackend::spawn(m.graph.clone(), m.notifier.clone(), move || {
        glib::MainContext::default().invoke(move || {
            glib::timeout_add_seconds_local_once(2, move || {
                if let Some(m) = weak.upgrade()
                    && !mixer::lock(&m.graph).connected
                    && m.has_backend()
                {
                    start_mixer(&m);
                }
            });
        });
    });
    m.set_backend(Some(Box::new(backend)));
}

#[cfg(not(feature = "pipewire"))]
fn start_mixer(_m: &Arc<Mixer>) {}
