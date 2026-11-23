// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline emoji, Unicode symbol and text-face dataset plus Fitzpatrick skin-tone modifiers.
//!
//! Loaded lazily on first use from `crates/deskd/data/emoji.tsv` (`include_str!`), so startup
//! touches nothing and no runtime file is needed.

use std::sync::OnceLock;

use desk_proto::emoji::EmojiItem;

const RAW_TSV: &str = include_str!("../../data/emoji.tsv");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmojiEntry {
    pub id: &'static str,
    pub glyph: &'static str,
    pub category: &'static str,
    pub supports_tone: bool,
    pub en_name: &'static str,
    pub en_keywords: &'static str,
    pub uk_name: &'static str,
    pub uk_keywords: &'static str,
}

impl EmojiEntry {
    pub fn to_item(&self, lang: &str) -> EmojiItem {
        let uk = lang.to_ascii_lowercase().starts_with("uk");
        EmojiItem {
            id: self.id.to_string(),
            glyph: self.glyph.to_string(),
            name: if uk { self.uk_name } else { self.en_name }.to_string(),
            category: self.category.to_string(),
            keywords: if uk {
                self.uk_keywords
            } else {
                self.en_keywords
            }
            .to_string(),
            supports_tone: self.supports_tone,
        }
    }
}

fn parse_dataset(raw: &'static str) -> Vec<EmojiEntry> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut cols = trimmed.split('\t');
        let (
            Some(id),
            Some(glyph),
            Some(category),
            Some(tone),
            Some(en_name),
            Some(en_keywords),
            Some(uk_name),
            Some(uk_keywords),
        ) = (
            cols.next(),
            cols.next(),
            cols.next(),
            cols.next(),
            cols.next(),
            cols.next(),
            cols.next(),
            cols.next(),
        )
        else {
            continue;
        };
        out.push(EmojiEntry {
            id,
            glyph,
            category,
            supports_tone: tone == "1",
            en_name,
            en_keywords,
            uk_name,
            uk_keywords,
        });
    }
    out
}

/// The full offline table, parsed once on first access.
pub fn dataset() -> &'static [EmojiEntry] {
    static TABLE: OnceLock<Vec<EmojiEntry>> = OnceLock::new();
    TABLE.get_or_init(|| parse_dataset(RAW_TSV))
}

pub fn find_by_id(id: &str) -> Option<&'static EmojiEntry> {
    dataset().iter().find(|e| e.id == id)
}

/// Fitzpatrick skin-tone modifier code point for a `skin-tone` setting value.
pub fn tone_modifier(tone: &str) -> Option<char> {
    match tone.trim() {
        "light" => Some('\u{1F3FB}'),
        "medium-light" => Some('\u{1F3FC}'),
        "medium" => Some('\u{1F3FD}'),
        "medium-dark" => Some('\u{1F3FE}'),
        "dark" => Some('\u{1F3FF}'),
        _ => None,
    }
}

/// Apply a Fitzpatrick skin-tone modifier to `glyph` when `supports_tone` is true.
///
/// Follows Unicode UTS #51: inserts the modifier right after the base emoji character, replacing
/// an immediately following variation selector (`U+FE0F`) if present and keeping any ZWJ tail
/// (e.g. `🧑‍💻` + `medium` → `🧑🏽‍💻`, `✌️` + `medium` → `✌🏽`, `👋` + `medium` → `👋🏽`).
pub fn apply_skin_tone(glyph: &str, tone: &str, supports_tone: bool) -> String {
    if !supports_tone {
        return glyph.to_string();
    }
    let Some(modifier) = tone_modifier(tone) else {
        return glyph.to_string();
    };
    let mut chars = glyph.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::with_capacity(glyph.len() + 4);
    out.push(first);
    let mut rest = chars.peekable();
    // Strip variation selector-16 (U+FE0F) right after the base character before adding modifier
    if rest.peek() == Some(&'\u{FE0F}') {
        rest.next();
    }
    // Replace any existing Fitzpatrick modifier
    if rest
        .peek()
        .is_some_and(|c| ('\u{1F3FB}'..='\u{1F3FF}').contains(c))
    {
        rest.next();
    }
    out.push(modifier);
    out.extend(rest);
    out
}
