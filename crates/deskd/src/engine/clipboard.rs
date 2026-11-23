// SPDX-License-Identifier: GPL-3.0-or-later
//! The clipboard history engine: the history, its debounced saver, the expiry timer of probable
//! secrets and the GSettings watcher.

use std::sync::{Arc, Mutex};

use desk_proto::{CLIPBOARD_PATH, CLIPBOARD_SCHEMA_ID};
use desk_settings::accessors::ClipboardSettings;
use desk_settings::tr;
use gio::glib;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::clipboard::Config;
use crate::persist::{self, Saver};
use crate::service::{self, ClipboardIface, Shared};
use crate::settings;
use crate::trust::Gate;

pub struct ClipboardFeature {
    eng: Shared,
    saver: Saver,
    gate: Gate,
    settings: Option<ClipboardSettings>,
}

impl ClipboardFeature {
    /// Read the settings, load the saved history and start its saver.
    pub fn new(setup: &Setup) -> ClipboardFeature {
        let settings = settings::clipboard_settings();
        let cfg = match &settings {
            Some(s) => settings::read_config(s),
            None => {
                eprintln!(
                    "deskd: {}",
                    tr!(
                        "the schema {schema} is not installed (data/install.sh), using the default settings",
                        schema = CLIPBOARD_SCHEMA_ID
                    )
                );
                Config::default()
            }
        };
        let (data_dir, runtime_dir) = crate::clipboard::Engine::default_dirs();
        let mut eng = crate::clipboard::Engine::new(cfg, data_dir, runtime_dir);
        eng.load();
        let eng: Shared = Arc::new(Mutex::new(eng));
        let saver = {
            let eng = eng.clone();
            Saver::spawn(
                move || {
                    if let Err(e) = service::lock(&eng).save() {
                        eprintln!("deskd: cannot save the clipboard history: {e}");
                    }
                },
                persist::QUIET,
                persist::MAX_WAIT,
            )
        };
        ClipboardFeature {
            eng,
            saver,
            gate: setup.gate.clone(),
            settings,
        }
    }
}

impl Engine for ClipboardFeature {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            CLIPBOARD_PATH,
            ClipboardIface {
                eng: self.eng.clone(),
                saver: self.saver.clone(),
                set_private: settings::set_private_async,
                gate: self.gate.clone(),
            },
        )
    }

    fn start(&mut self, ctx: &mut RuntimeCtx<'_>) {
        // probable secrets live in memory for a short time only (clipboard::SECRET_TTL)
        {
            let eng = self.eng.clone();
            let conn = ctx.conn.clone();
            glib::timeout_add_seconds_local(15, move || {
                if service::lock(&eng).expire(service::now()) {
                    emit_changed(&conn);
                }
                glib::ControlFlow::Continue
            });
        }
        if let Some(s) = &self.settings {
            let eng = self.eng.clone();
            let conn = ctx.conn.clone();
            let typed = s.clone();
            s.0.connect_changed(None, move |_, key| {
                let cfg = settings::read_config(&typed);
                {
                    let mut e = eng.lock().unwrap_or_else(|p| p.into_inner());
                    e.set_config(cfg);
                    if key != "private-mode" {
                        let _ = e.save();
                    }
                }
                emit_changed(&conn);
            });
        }
    }

    fn shutdown(&mut self) {
        self.saver.flush();
    }
}

/// Сповістити клієнтів (міст, GUI), що список або властивості змінилися.
fn emit_changed(conn: &zbus::Connection) {
    let conn = conn.clone();
    std::thread::spawn(move || {
        zbus::block_on(async {
            if let Ok(iface) = conn
                .object_server()
                .interface::<_, ClipboardIface>(CLIPBOARD_PATH)
                .await
            {
                let em = iface.signal_emitter();
                let _ = ClipboardIface::changed(em).await;
                let g = iface.get().await;
                let _ = g.private_changed(em).await;
                let _ = g.enabled_changed(em).await;
                let _ = g.count_changed(em).await;
            }
        });
    });
}
