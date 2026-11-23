// SPDX-License-Identifier: GPL-3.0-or-later
//! The focus modes engine: the state machine, its reconciliation with the live GNOME keys, the
//! one-shot schedule timer and the GSettings watchers.

use std::sync::{Arc, Mutex};

use desk_proto::FOCUS_PATH;
use desk_settings::accessors::FocusSettings;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::focus::{self, FocusEngine};
use crate::schedule::Scheduler;
use crate::settings;
use crate::trust::Gate;

pub struct FocusFeature {
    eng: focus::Shared,
    scheduler: Scheduler,
    gate: Gate,
    settings: Option<FocusSettings>,
    /// `org.gnome.desktop.notifications`: GNOME's own do-not-disturb switch.
    notifications: Option<gio::Settings>,
    /// `org.gnome.desktop.sound`: the event sounds switch.
    sound: Option<gio::Settings>,
}

impl FocusFeature {
    /// Read the settings and the saved state, and start the scheduler; it sends signals once the
    /// bus exists.
    pub fn new(setup: &Setup) -> FocusFeature {
        let settings = settings::focus_settings();
        let notifications = settings::notifications_settings();
        let sound = settings::sound_settings();
        let store_path = focus::default_store_path();
        let eng: focus::Shared = Arc::new(Mutex::new(FocusEngine::new(
            settings
                .as_ref()
                .map_or_else(Default::default, settings::read_focus),
            focus::load_store(&store_path),
            notifications
                .as_ref()
                .is_none_or(|s| s.boolean(settings::SHOW_BANNERS_KEY)),
            sound
                .as_ref()
                .is_none_or(|s| s.boolean(settings::EVENT_SOUNDS_KEY)),
            settings.as_ref().map_or_default(|s| s.active_mode()),
            settings::FocusEffects { store_path },
            focus::now_system,
        )));
        let scheduler = Scheduler::spawn("deskd-focus", {
            let eng = eng.clone();
            let bus = setup.bus.clone();
            move || {
                let (delay, changed, ended) = {
                    let mut e = focus::lock(&eng);
                    let (delay, changed) = e.tick();
                    (delay, changed, e.take_session_ended())
                };
                if (changed || ended)
                    && let Some(conn) = bus.get()
                {
                    zbus::block_on(focus::service::emit(conn, ended));
                }
                delay
            }
        });
        FocusFeature {
            eng,
            scheduler,
            gate: setup.gate.clone(),
            settings,
            notifications,
            sound,
        }
    }

    fn watch_settings(&self, s: &FocusSettings) {
        let eng = self.eng.clone();
        let sched = self.scheduler.clone();
        let typed = s.clone();
        s.0.connect_changed(None, move |_, key| {
            // active-mode is deskd's own mirror of the state
            if key != "active-mode" {
                focus::lock(&eng).set_config(settings::read_focus(&typed));
                sched.trigger();
            }
        });
    }

    /// GNOME's own do-not-disturb switch and the sound switch: the user wins over a mode.
    fn watch_gnome_switches(&self, conn: &zbus::Connection) {
        for (gs, key) in [
            (&self.notifications, settings::SHOW_BANNERS_KEY),
            (&self.sound, settings::EVENT_SOUNDS_KEY),
        ] {
            let Some(gs) = gs else { continue };
            let eng = self.eng.clone();
            let sched = self.scheduler.clone();
            let conn = conn.clone();
            gs.connect_changed(Some(key), move |s, key| {
                let on = s.boolean(key);
                let changed = {
                    let mut e = focus::lock(&eng);
                    if key == settings::SHOW_BANNERS_KEY {
                        e.on_external_show_banners(on)
                    } else {
                        e.on_external_event_sounds(on)
                    }
                };
                if changed {
                    sched.trigger();
                    let conn = conn.clone();
                    std::thread::spawn(move || zbus::block_on(focus::service::emit(&conn, false)));
                }
            });
        }
    }
}

impl Engine for FocusFeature {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            FOCUS_PATH,
            focus::service::FocusIface {
                eng: self.eng.clone(),
                scheduler: self.scheduler.clone(),
                gate: self.gate.clone(),
            },
        )
    }

    fn start(&mut self, ctx: &mut RuntimeCtx<'_>) {
        {
            let sched = self.scheduler.clone();
            ctx.wake.on_wake(move || sched.trigger());
        }
        if let Some(s) = &self.settings {
            self.watch_settings(s);
        }
        self.watch_gnome_switches(ctx.conn);
    }

    fn shutdown(&mut self) {
        self.scheduler.shutdown();
    }
}
