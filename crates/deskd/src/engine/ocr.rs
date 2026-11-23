// SPDX-License-Identifier: GPL-3.0-or-later
//! The OCR text extractor engine: the `Ocr` D-Bus object and the GSettings watcher. Idle cost is
//! zero: Tesseract is a child process started per request.

use std::path::PathBuf;
use std::sync::Arc;

use desk_proto::OCR_PATH;
use desk_settings::accessors::OcrSettings;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::clipboard::runtime_base;
use crate::ocr::DEFAULT_TIMEOUT;
use crate::ocr::service::{Ocr, OcrIface, Shared};
use crate::settings;
use crate::trust::Gate;

pub struct OcrEngine {
    ocr: Shared,
    gate: Gate,
    settings: Option<OcrSettings>,
}

impl OcrEngine {
    /// Read the settings; `tesseract` is looked up in `PATH` per request.
    pub fn new(setup: &Setup) -> OcrEngine {
        let settings = settings::ocr_settings();
        let cfg = settings
            .as_ref()
            .map_or_else(Default::default, settings::read_ocr);
        OcrEngine {
            ocr: Arc::new(Ocr::new(
                PathBuf::from("tesseract"),
                runtime_base(),
                DEFAULT_TIMEOUT,
                cfg,
            )),
            gate: setup.gate.clone(),
            settings,
        }
    }
}

impl Engine for OcrEngine {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            OCR_PATH,
            OcrIface {
                ocr: self.ocr.clone(),
                gate: self.gate.clone(),
            },
        )
    }

    fn start(&mut self, _ctx: &mut RuntimeCtx<'_>) {
        let Some(s) = &self.settings else { return };
        let ocr = self.ocr.clone();
        let typed = s.clone();
        s.0.connect_changed(None, move |_, key| {
            if matches!(
                key,
                "enabled" | "languages" | "keep-line-breaks" | "max-megapixels"
            ) {
                ocr.set_config(settings::read_ocr(&typed));
            }
        });
    }
}
