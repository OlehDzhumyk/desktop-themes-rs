// SPDX-License-Identifier: GPL-3.0-or-later
//! The search providers that live in deskd: the clipboard history (`# text`) and emoji
//! (`: text`). Both also answer plain queries, weighted below applications. Files come from
//! `desk_search::FileIndex`.

use desk_search::rank;
use desk_search::{Provider, ResultKind, SearchResult, W_CLIPBOARD, W_EMOJI};

use crate::clipboard::{Engine as ClipboardHistory, Entry};
use crate::emoji::data::EmojiEntry;
use crate::emoji::search::ranked;

/// The longest title of a clipboard result, in characters (then «…»).
pub const PREVIEW_CHARS: usize = 80;
/// How much of an entry is matched, in characters (very long copies stay cheap to search).
const MATCH_CHARS: usize = 4096;

/// The clipboard history as a provider. It answers nothing while the history is off or in
/// private mode (spec «Privacy & security»: private mode hides clipboard results).
pub struct ClipboardProvider<'a>(pub &'a ClipboardHistory);

impl Provider for ClipboardProvider<'_> {
    fn kind(&self) -> ResultKind {
        ResultKind::Clipboard
    }

    fn search(&self, text: &str, scoped: bool, limit: usize) -> Vec<SearchResult> {
        let cfg = &self.0.cfg;
        if !cfg.enabled || cfg.private || limit == 0 {
            return Vec::new();
        }
        let query = rank::fold(text);
        if query.is_empty() && !scoped {
            return Vec::new();
        }
        let entries = self.0.entries();
        let mut out: Vec<SearchResult> = entries
            .iter()
            .enumerate()
            .filter_map(|(pos, e)| {
                let tier = if query.is_empty() {
                    rank::SUBSTRING
                } else {
                    let head: String = e.text.chars().take(MATCH_CHARS).collect();
                    let folded = rank::fold(&head);
                    if folded.starts_with(&query) {
                        rank::PREFIX
                    } else if folded.contains(&query) {
                        rank::SUBSTRING
                    } else {
                        return None;
                    }
                };
                let score = if scoped {
                    // newest first: the history order, kept by a falling score
                    u32::try_from(entries.len() - pos).unwrap_or(u32::MAX)
                } else {
                    tier * W_CLIPBOARD / 100
                };
                Some(clip_result(e, score))
            })
            .collect();
        if !scoped {
            // a stable sort keeps the newest first within a tier
            out.sort_by_key(|r| std::cmp::Reverse(r.score));
        }
        out.truncate(limit);
        out
    }
}

fn clip_result(e: &Entry, score: u32) -> SearchResult {
    SearchResult {
        id: format!("clipboard:{}", e.id),
        kind: ResultKind::Clipboard,
        title: preview(&e.text),
        subtitle: String::new(),
        icon: if e.image.is_empty() {
            "edit-paste-symbolic"
        } else {
            "image-x-generic-symbolic"
        }
        .to_owned(),
        // the bridge activates a numeric target through the clipboard history
        activate: e.id.to_string(),
        score,
    }
}

/// The first non-empty line of `text`, runs of white space as one space, at most
/// [`PREVIEW_CHARS`] characters; «…» marks a cut line or more lines.
pub fn preview(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let mut out = String::new();
    let mut count = 0;
    let mut cut = false;
    'words: for word in line.split_whitespace() {
        let sep = if out.is_empty() { "" } else { " " };
        for c in sep.chars().chain(word.chars()) {
            if count == PREVIEW_CHARS {
                cut = true;
                break 'words;
            }
            out.push(c);
            count += 1;
        }
    }
    if cut {
        out.pop();
        out.truncate(out.trim_end().len());
        out.push('…');
    } else if text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .nth(1)
        .is_some()
    {
        out.push('…');
    }
    out
}

/// The emoji dataset as a provider, named in `lang` (`uk` or `en`); English and Ukrainian names
/// and keywords both match.
pub struct EmojiProvider<'a> {
    pub entries: &'a [EmojiEntry],
    pub lang: &'a str,
}

/// Match ranks of the emoji search that a plain query shows (exact name, name prefix, name word
/// prefix or exact keyword); weaker ones only in the `:` scope.
const PLAIN_EMOJI_SCORES: [u32; 3] = [rank::EXACT, rank::PREFIX, rank::WORD_PREFIX];

impl Provider for EmojiProvider<'_> {
    fn kind(&self) -> ResultKind {
        ResultKind::Emoji
    }

    fn search(&self, text: &str, scoped: bool, limit: usize) -> Vec<SearchResult> {
        let hits: Vec<(u8, &EmojiEntry)> = if text.trim().is_empty() {
            if !scoped {
                return Vec::new();
            }
            self.entries.iter().take(limit).map(|e| (4, e)).collect()
        } else {
            ranked(self.entries, text, limit, &[])
        };
        let count = hits.len();
        hits.into_iter()
            .enumerate()
            .filter_map(|(pos, (tier, e))| {
                let score = if scoped {
                    u32::try_from(count - pos).unwrap_or(u32::MAX)
                } else {
                    PLAIN_EMOJI_SCORES.get(usize::from(tier))? * W_EMOJI / 100
                };
                let item = e.to_item(self.lang);
                Some(SearchResult {
                    id: format!("emoji:{}", item.id),
                    kind: ResultKind::Emoji,
                    title: item.glyph.clone(),
                    subtitle: item.name,
                    icon: String::new(),
                    activate: item.glyph,
                    score,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::Config;
    use crate::emoji::data::dataset;

    const T: &str = "text/plain;charset=utf-8";

    fn history(tag: &str, cfg: Config) -> (ClipboardHistory, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("deskd-clipsearch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut h = ClipboardHistory::new(cfg, d.join("data"), d.join("run"));
        for (i, text) in ["Token abc", "meeting notes\nsecond line", "my token xyz"]
            .into_iter()
            .enumerate()
        {
            h.push(T, text.as_bytes(), &[], "", i64::try_from(i).unwrap());
        }
        (h, d)
    }

    fn titles(r: &[SearchResult]) -> Vec<&str> {
        r.iter().map(|x| x.title.as_str()).collect()
    }

    #[test]
    fn clipboard_scope_lists_newest_first_with_numeric_targets() {
        let (h, d) = history("scope", Config::default());
        let p = ClipboardProvider(&h);
        let all = p.search("", true, 10);
        assert_eq!(
            titles(&all),
            ["my token xyz", "meeting notes…", "Token abc"]
        );
        assert!(all.iter().all(|x| x.kind == ResultKind::Clipboard));
        let newest = h.entries()[0].id;
        assert_eq!(all[0].activate, newest.to_string());
        assert_eq!(all[0].id, format!("clipboard:{newest}"));
        assert_eq!(
            titles(&p.search("token", true, 10)),
            ["my token xyz", "Token abc"]
        );
        assert_eq!(p.search("token", true, 1).len(), 1);
        assert!(p.search("absent", true, 10).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn clipboard_plain_query_ranks_prefix_first_and_needs_text() {
        let (h, d) = history("plain", Config::default());
        let p = ClipboardProvider(&h);
        let r = p.search("token", false, 10);
        // «Token abc» starts with the query, so it outranks the newer substring match
        assert_eq!(titles(&r), ["Token abc", "my token xyz"]);
        assert_eq!(r[0].score, rank::PREFIX * W_CLIPBOARD / 100);
        assert!(r[0].score > r[1].score);
        assert!(p.search("", false, 10).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn clipboard_private_or_disabled_answers_nothing() {
        let (mut h, d) = history("private", Config::default());
        assert!(!ClipboardProvider(&h).search("", true, 10).is_empty());
        h.cfg.private = true;
        assert!(ClipboardProvider(&h).search("", true, 10).is_empty());
        assert!(ClipboardProvider(&h).search("token", false, 10).is_empty());
        h.cfg.private = false;
        h.cfg.enabled = false;
        assert!(ClipboardProvider(&h).search("token", true, 10).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn preview_is_one_short_line() {
        assert_eq!(preview("  hello   world  "), "hello world");
        assert_eq!(preview("\n\n  first\nsecond"), "first…");
        let long = "word ".repeat(40);
        let p = preview(&long);
        assert_eq!(p.chars().count(), PREVIEW_CHARS);
        assert!(p.ends_with('…'));
        assert_eq!(preview(&"ї".repeat(100)).chars().count(), PREVIEW_CHARS);
        assert_eq!(
            preview(&"ї".repeat(PREVIEW_CHARS)),
            "ї".repeat(PREVIEW_CHARS)
        );
        assert_eq!(preview(""), "");
    }

    #[test]
    fn emoji_match_english_and_ukrainian_with_glyph_targets() {
        for (lang, query) in [("en", "heart"), ("uk", "серце")] {
            let p = EmojiProvider {
                entries: dataset(),
                lang,
            };
            let r = p.search(query, true, 5);
            assert!(!r.is_empty(), "{query}");
            assert!(r.len() <= 5);
            for x in &r {
                assert_eq!(x.kind, ResultKind::Emoji);
                assert_eq!(x.title, x.activate);
                assert!(x.id.starts_with("emoji:"));
            }
            // scoped results keep the emoji ranking by a falling score
            assert!(r.windows(2).all(|w| w[0].score > w[1].score));
        }
        let p = EmojiProvider {
            entries: dataset(),
            lang: "en",
        };
        // the empty scope lists the start of the dataset; the empty plain query nothing
        assert_eq!(p.search("", true, 3).len(), 3);
        assert!(p.search("", false, 3).is_empty());
    }

    #[test]
    fn emoji_plain_query_keeps_strong_matches_weighted() {
        let p = EmojiProvider {
            entries: dataset(),
            lang: "en",
        };
        let r = p.search("heart", false, 50);
        assert!(!r.is_empty());
        assert!(
            r.iter()
                .all(|x| x.score >= rank::WORD_PREFIX * W_EMOJI / 100)
        );
        assert!(r.iter().all(|x| x.score <= rank::EXACT * W_EMOJI / 100));
    }
}
