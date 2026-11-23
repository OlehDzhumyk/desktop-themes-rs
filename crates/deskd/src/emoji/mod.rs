// SPDX-License-Identifier: GPL-3.0-or-later
//! Emoji and symbol picker engine of deskd (`docs/spec/features/emoji.md`).
//!
//! Offline dataset with Ukrainian and English search, Fitzpatrick skin-tone modifiers, and local
//! 0600 recents persistence.

pub mod data;
pub mod recents;
pub mod search;
pub mod service;

use std::path::PathBuf;

use desk_proto::emoji::EmojiItem;

use self::data::{apply_skin_tone, dataset, find_by_id};
use self::recents::Recents;

pub struct EmojiEngine {
    recents: Recents,
    default_tone: String,
    default_lang: String,
}

impl EmojiEngine {
    pub fn new(recents_path: PathBuf, remember_recents: bool, default_tone: String) -> Self {
        let mut recents = Recents::new(recents_path, remember_recents);
        recents.load();
        Self {
            recents,
            default_tone,
            default_lang: "en".to_string(),
        }
    }

    pub fn set_config(&mut self, remember_recents: bool, default_tone: String) {
        self.recents.set_remember(remember_recents);
        self.default_tone = default_tone;
    }

    pub fn set_lang(&mut self, lang: String) {
        if !lang.is_empty() {
            self.default_lang = lang;
        }
    }

    pub fn search(&self, query: &str, lang: &str, limit: u32) -> Vec<EmojiItem> {
        let effective_lang = if lang.is_empty() {
            &self.default_lang
        } else {
            lang
        };
        search::search(dataset(), query, effective_lang, limit, self.recents.ids())
    }

    pub fn category(&self, category: &str, limit: u32) -> Vec<EmojiItem> {
        let cat = category.trim().to_ascii_lowercase();
        let it = dataset().iter().filter(|e| e.category == cat);
        if limit == 0 {
            it.map(|e| e.to_item(&self.default_lang)).collect()
        } else {
            it.take(limit as usize)
                .map(|e| e.to_item(&self.default_lang))
                .collect()
        }
    }

    pub fn recents(&self) -> Vec<EmojiItem> {
        self.recents
            .ids()
            .iter()
            .filter_map(|id| find_by_id(id))
            .map(|e| e.to_item(&self.default_lang))
            .collect()
    }

    /// Pick an entry by `id`, applying `tone` (or the configured `default_tone` when `tone` is
    /// empty), and record it in recents if enabled. Returns `None` if `id` is unknown.
    pub fn pick(&mut self, id: &str, tone: &str) -> Option<String> {
        let entry = find_by_id(id)?;
        let effective_tone = if tone.is_empty() {
            &self.default_tone
        } else {
            tone
        };
        let glyph = apply_skin_tone(entry.glyph, effective_tone, entry.supports_tone);
        self.recents.record(entry.id);
        Some(glyph)
    }

    pub fn clear_recents(&mut self) {
        self.recents.clear();
    }
}
