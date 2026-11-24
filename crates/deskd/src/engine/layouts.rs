// SPDX-License-Identifier: GPL-3.0-or-later
//! The window layouts engine: the D-Bus object and the GSettings watcher. Idle cost is zero: no
//! thread or timer; only a GSettings change callback.

use std::sync::{Arc, Mutex};

use desk_layouts::LayoutStore;
use desk_proto::LAYOUTS_PATH;
use desk_settings::accessors::LayoutsSettings;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::layouts::service::{self, LayoutsIface, Shared};
use crate::layouts::{self, Layouts};
use crate::settings;
use crate::trust::Gate;

pub struct LayoutsEngine {
    eng: Shared,
    gate: Gate,
    settings: Option<LayoutsSettings>,
}

impl LayoutsEngine {
    pub fn new(setup: &Setup) -> LayoutsEngine {
        let settings = settings::layouts_settings();
        let cfg = settings
            .as_ref()
            .map_or_else(Default::default, settings::read_layouts);
        let store = LayoutStore::load(layouts::default_store_path());
        LayoutsEngine {
            eng: Arc::new(Mutex::new(Layouts::new(cfg, store))),
            gate: setup.gate.clone(),
            settings,
        }
    }
}

impl Engine for LayoutsEngine {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            LAYOUTS_PATH,
            LayoutsIface {
                engine: self.eng.clone(),
                gate: self.gate.clone(),
            },
        )
    }

    fn start(&mut self, ctx: &mut RuntimeCtx<'_>) {
        let Some(s) = &self.settings else { return };
        let eng = self.eng.clone();
        let typed = s.clone();
        let conn = ctx.conn.clone();
        s.0.connect_changed(None, move |_, key| {
            if !matches!(key, "enabled" | "gap-outer" | "gap-inner") {
                return;
            }
            if service::lock(&eng).set_config(settings::read_layouts(&typed)) {
                let conn = conn.clone();
                std::thread::spawn(move || zbus::block_on(service::emit(&conn)));
            }
        });
    }
}
