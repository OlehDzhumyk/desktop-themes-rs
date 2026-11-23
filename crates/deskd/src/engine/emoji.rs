// SPDX-License-Identifier: GPL-3.0-or-later
//! The emoji picker engine: search, recents and the GSettings watcher.

use std::sync::{Arc, Mutex};

use desk_proto::EMOJI_PATH;
use desk_settings::accessors::EmojiSettings;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::emoji::service::{self, EmojiIface, SharedEmoji};
use crate::emoji::{EmojiEngine, recents::Recents};
use crate::settings;
use crate::trust::Gate;

pub struct EmojiFeature {
    eng: SharedEmoji,
    gate: Gate,
    settings: Option<EmojiSettings>,
}

impl EmojiFeature {
    /// Read the settings and the recents.
    pub fn new(setup: &Setup) -> EmojiFeature {
        let settings = settings::emoji_settings();
        let (remember, tone) = settings
            .as_ref()
            .map_or((true, "none".to_string()), settings::read_emoji);
        let eng = Arc::new(Mutex::new(EmojiEngine::new(
            Recents::default_path(),
            remember,
            tone,
        )));
        EmojiFeature {
            eng,
            gate: setup.gate.clone(),
            settings,
        }
    }
}

impl Engine for EmojiFeature {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            EMOJI_PATH,
            EmojiIface {
                engine: self.eng.clone(),
                gate: self.gate.clone(),
            },
        )
    }

    fn start(&mut self, _ctx: &mut RuntimeCtx<'_>) {
        if let Some(s) = &self.settings {
            let eng = self.eng.clone();
            let typed = s.clone();
            s.0.connect_changed(None, move |_, key| {
                if matches!(key, "remember-recents" | "skin-tone") {
                    let (remember, tone) = settings::read_emoji(&typed);
                    service::lock(&eng).set_config(remember, tone);
                }
            });
        }
    }
}
