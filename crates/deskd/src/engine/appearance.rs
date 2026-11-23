// SPDX-License-Identifier: GPL-3.0-or-later
//! The appearance engine: automatic light/dark with its one-shot wake scheduler, the live accent
//! from the wallpaper, and the GSettings watchers.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use desk_proto::APPEARANCE_PATH;
use desk_settings::accessors::AppearanceSettings;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::appearance::{self, AppearanceEngine, solar::Scheme, wallpaper};
use crate::schedule::Scheduler;
use crate::settings;
use crate::trust::Gate;

pub struct AppearanceFeature {
    eng: appearance::Shared,
    scheduler: Scheduler,
    wallpaper: wallpaper::Shared,
    gate: Gate,
    settings: Option<AppearanceSettings>,
    /// `org.gnome.desktop.interface`: the live `color-scheme`.
    interface: Option<gio::Settings>,
    /// Drops last (after the GSettings objects above).
    watch: Option<Rc<wallpaper::Watch>>,
}

impl AppearanceFeature {
    /// Read the settings and start the scheduler; it sends `Changed` once the bus exists.
    pub fn new(setup: &Setup) -> AppearanceFeature {
        let settings = settings::appearance_settings();
        let interface = settings::interface_settings();
        let cfg = settings
            .as_ref()
            .map_or_else(Default::default, settings::read_appearance);
        let initial_scheme = interface
            .as_ref()
            .map_or(Scheme::Light, settings::read_color_scheme);
        let eng: appearance::Shared = Arc::new(Mutex::new(AppearanceEngine::new(
            cfg,
            initial_scheme,
            settings::set_color_scheme_async,
            appearance::local_now_system,
            appearance::cities::detect_system_timezone,
        )));
        let scheduler = Scheduler::spawn("deskd-appearance", {
            let eng = eng.clone();
            let bus = setup.bus.clone();
            move || {
                let (delay, changed) = appearance::lock(&eng).tick();
                if changed && let Some(conn) = bus.get() {
                    zbus::block_on(appearance::service::emit_changed(conn));
                }
                delay
            }
        });
        let wallpaper: wallpaper::Shared = Arc::new(Mutex::new(
            wallpaper::WallpaperEngine::new(
                settings
                    .as_ref()
                    .map_or_else(Default::default, settings::read_wallpaper),
                wallpaper::state_path(),
            )
            .with_manager_state(wallpaper::manager_state_path()),
        ));
        AppearanceFeature {
            eng,
            scheduler,
            wallpaper,
            gate: setup.gate.clone(),
            settings,
            interface,
            watch: None,
        }
    }

    fn watch_settings(&self, conn: &zbus::Connection, s: &AppearanceSettings) {
        let eng = self.eng.clone();
        let sched = self.scheduler.clone();
        let conn = conn.clone();
        let typed = s.clone();
        let watch = self.watch.clone();
        s.0.connect_changed(None, move |_, key| {
            if matches!(key, "accent-from-wallpaper" | "accent-source") {
                if let Some(w) = &watch {
                    w.settings_changed(settings::read_wallpaper(&typed));
                }
                return;
            }
            let cfg = settings::read_appearance(&typed);
            appearance::lock(&eng).set_config(cfg);
            sched.trigger();
            emit_changed_async(&conn);
        });
    }

    fn watch_color_scheme(&self, conn: &zbus::Connection, s: &gio::Settings) {
        let eng = self.eng.clone();
        let conn = conn.clone();
        s.connect_changed(Some("color-scheme"), move |s, _| {
            let scheme = settings::read_color_scheme(s);
            if appearance::lock(&eng).on_external_scheme_change(scheme) {
                emit_changed_async(&conn);
            }
        });
    }
}

impl Engine for AppearanceFeature {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            APPEARANCE_PATH,
            appearance::service::AppearanceIface {
                eng: self.eng.clone(),
                scheduler: self.scheduler.clone(),
                gate: self.gate.clone(),
                wallpaper: self.wallpaper.clone(),
            },
        )
    }

    fn start(&mut self, ctx: &mut RuntimeCtx<'_>) {
        {
            let sched = self.scheduler.clone();
            ctx.wake.on_wake(move || sched.trigger());
        }
        // live accent from the wallpaper: GSettings signals, a 500 ms debounce, a decoding child
        self.watch = {
            let conn = ctx.conn.clone();
            wallpaper::Watch::start(self.wallpaper.clone(), move || emit_changed_async(&conn))
                .map(Rc::new)
        };
        if let Some(s) = &self.settings {
            self.watch_settings(ctx.conn, s);
        }
        if let Some(s) = &self.interface {
            self.watch_color_scheme(ctx.conn, s);
        }
    }

    fn shutdown(&mut self) {
        self.scheduler.shutdown();
    }
}

/// Send `Changed` from a new thread (GSettings handlers run in the GLib main thread).
fn emit_changed_async(conn: &zbus::Connection) {
    let conn = conn.clone();
    std::thread::spawn(move || {
        zbus::block_on(appearance::service::emit_changed(&conn));
    });
}
