// SPDX-License-Identifier: GPL-3.0-or-later
//! The screenshot engine: the D-Bus object and the GSettings watcher. Idle cost is zero: no
//! thread, timer or image; only a GSettings change callback.

use std::sync::{Arc, Mutex};

use desk_proto::CAPTURE_PATH;
use desk_settings::accessors::CaptureSettings;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::capture::service::{self, CaptureIface, Shared};
use crate::capture::{Capture, Dirs};
use crate::settings;
use crate::trust::Gate;

pub struct CaptureFeature {
    eng: Shared,
    gate: Gate,
    settings: Option<CaptureSettings>,
}

impl CaptureFeature {
    pub fn new(setup: &Setup) -> CaptureFeature {
        let settings = settings::capture_settings();
        let cfg = settings
            .as_ref()
            .map_or_else(Default::default, settings::read_capture);
        CaptureFeature {
            eng: Arc::new(Mutex::new(Capture::new(cfg, Dirs::of_user()))),
            gate: setup.gate.clone(),
            settings,
        }
    }
}

impl Engine for CaptureFeature {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            CAPTURE_PATH,
            CaptureIface {
                engine: self.eng.clone(),
                gate: self.gate.clone(),
            },
        )
    }

    fn start(&mut self, _ctx: &mut RuntimeCtx<'_>) {
        let Some(s) = &self.settings else { return };
        let eng = self.eng.clone();
        let typed = s.clone();
        s.0.connect_changed(None, move |_, key| {
            if matches!(key, "enabled" | "save-folder") {
                service::lock(&eng).set_config(settings::read_capture(&typed));
            }
        });
    }
}
