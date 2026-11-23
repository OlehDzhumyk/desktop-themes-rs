// SPDX-License-Identifier: GPL-3.0-or-later
//! The colour picker engine: formatting, the history of picks and the GSettings watcher.

use std::sync::{Arc, Mutex};

use desk_proto::COLOR_PICKER_PATH;
use desk_settings::accessors::ColorPickerSettings;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::color_picker::ColorPicker;
use crate::color_picker::service::{self, ColorPickerIface, Shared};
use crate::settings;
use crate::trust::Gate;

pub struct ColorPickerFeature {
    eng: Shared,
    gate: Gate,
    settings: Option<ColorPickerSettings>,
}

impl ColorPickerFeature {
    /// Read the settings and the history (or delete it when the history size is 0).
    pub fn new(setup: &Setup) -> ColorPickerFeature {
        let settings = settings::color_picker_settings();
        let cfg = settings
            .as_ref()
            .map_or_else(Default::default, settings::read_color_picker);
        ColorPickerFeature {
            eng: Arc::new(Mutex::new(ColorPicker::new(
                ColorPicker::default_path(),
                cfg,
            ))),
            gate: setup.gate.clone(),
            settings,
        }
    }
}

impl Engine for ColorPickerFeature {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            COLOR_PICKER_PATH,
            ColorPickerIface {
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
            if !matches!(key, "format" | "history-size") {
                return;
            }
            let cfg = settings::read_color_picker(&typed);
            if service::lock(&eng).set_config(cfg) {
                let conn = conn.clone();
                std::thread::spawn(move || zbus::block_on(service::emit(&conn)));
            }
        });
    }
}
