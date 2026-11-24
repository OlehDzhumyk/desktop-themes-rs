// SPDX-License-Identifier: GPL-3.0-or-later
//! Case-insensitive, diacritic/apostrophe-tolerant ranked search across English and Ukrainian
//! emoji and symbol names and keywords.

use desk_proto::emoji::EmojiItem;

use super::data::EmojiEntry;

const DEFAULT_LIMIT: usize = 120;

/// Fold case, strip Ukrainian/typographic apostrophes and fold common Latin diacritics so queries
/// like `румяна`, `рум'яна`, `cafe`, `café`, or `upside down` all match cleanly.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        for lc in c.to_lowercase() {
            match lc {
                '\'' | '\u{2019}' | '\u{02BC}' | '`' => {}
                '-' | '_' | ':' | ',' | '.' | ';' => out.push(' '),
                'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => out.push('a'),
                'é' | 'è' | 'ê' | 'ë' => out.push('e'),
                'í' | 'ì' | 'î' | 'ï' => out.push('i'),
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' => out.push('o'),
                'ú' | 'ù' | 'û' | 'ü' => out.push('u'),
                'ñ' => out.push('n'),
                'ç' => out.push('c'),
                'ё' => out.push('е'),
                other => out.push(other),
            }
        }
    }
    out
}

/// Match rank for a single entry against a normalized query (lower is better; `None` = no match).
///
/// 0: exact name match (en or uk)
/// 1: name prefix match
/// 2: name word prefix or exact keyword match
/// 3: keyword word prefix match
/// 4: substring match across name/keywords (all query tokens present)
fn score_entry(e: &EmojiEntry, q: &str, tokens: &[&str]) -> Option<u8> {
    let en_name = normalize(e.en_name);
    let uk_name = normalize(e.uk_name);
    if en_name == q || uk_name == q || e.glyph == q {
        return Some(0);
    }
    if en_name.starts_with(q) || uk_name.starts_with(q) {
        return Some(1);
    }
    let en_kw = normalize(e.en_keywords);
    let uk_kw = normalize(e.uk_keywords);

    let name_word_prefix = en_name
        .split_whitespace()
        .chain(uk_name.split_whitespace())
        .any(|w| w.starts_with(q));
    let exact_kw = en_kw
        .split_whitespace()
        .chain(uk_kw.split_whitespace())
        .any(|w| w == q);
    if name_word_prefix || exact_kw {
        return Some(2);
    }

    let kw_prefix = en_kw
        .split_whitespace()
        .chain(uk_kw.split_whitespace())
        .any(|w| w.starts_with(q));
    if kw_prefix {
        return Some(3);
    }

    let haystack = format!("{en_name} {uk_name} {en_kw} {uk_kw}");
    if tokens.iter().all(|tok| haystack.contains(tok)) {
        return Some(4);
    }
    None
}

/// Search `entries` for `query`, returning up to `limit` (0 = default cap) items localized to
/// `lang` (`"uk"` or `"en"`). Ties break by recency in `recents`, then dataset order.
pub fn search(
    entries: &[EmojiEntry],
    query: &str,
    lang: &str,
    limit: u32,
    recents: &[String],
) -> Vec<EmojiItem> {
    let cap = if limit == 0 {
        DEFAULT_LIMIT
    } else {
        limit as usize
    };
    if normalize(query).split_whitespace().next().is_none() {
        return entries.iter().take(cap).map(|e| e.to_item(lang)).collect();
    }
    ranked(entries, query, cap, recents)
        .into_iter()
        .map(|(_, e)| e.to_item(lang))
        .collect()
}

/// The entries matching a non-empty `query` with their match rank (0 is best, see
/// `score_entry`), best first, at most `cap`. An empty query matches nothing here.
pub fn ranked<'a>(
    entries: &'a [EmojiEntry],
    query: &str,
    cap: usize,
    recents: &[String],
) -> Vec<(u8, &'a EmojiEntry)> {
    let q = normalize(query.trim());
    let tokens: Vec<&str> = q.split_whitespace().collect();
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut ranked: Vec<(u8, usize, usize, &EmojiEntry)> = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, e)| {
            let rank = score_entry(e, &q, &tokens)?;
            let recent_pos = recents.iter().position(|r| r == e.id).unwrap_or(usize::MAX);
            Some((rank, recent_pos, idx, e))
        })
        .collect();

    ranked.sort_by_key(|(rank, recent_pos, idx, _)| (*rank, *recent_pos, *idx));
    ranked
        .into_iter()
        .take(cap)
        .map(|(rank, _, _, e)| (rank, e))
        .collect()
}
