// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure search indexing and matching across all settings pages and keys.

use desk_settings::{Item, Page, PageKind, Widget, n_};

/// Built-in hub pages that live outside `desk_settings::PAGES`.
pub const BUILTIN_PAGES: &[(&str, &str, &str)] = &[
    (
        "themes",
        n_("Themes"),
        n_("Switch the look of the GNOME desktop"),
    ),
    (
        "mix",
        n_("Mix"),
        n_("Combine modules from different themes"),
    ),
    (
        "system",
        n_("System"),
        n_("System check, reset and baseline information"),
    ),
];

/// A single searchable target: either a page itself or a specific setting row on a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub page_id: &'static str,
    pub page_title: String,
    /// Specific row target (`key:<schema>:<name>` or `shortcut:<id>`), or `None` for a page hit.
    pub target_id: Option<String>,
    /// Display title for the hit row.
    pub title: String,
    /// Display subtitle for the hit row.
    pub subtitle: String,
}

/// Stable target identifier for a key row on a settings page.
pub fn key_target_id(schema: &str, name: &str) -> String {
    format!("key:{schema}:{name}")
}

/// Stable target identifier for a shortcut row on a settings page.
pub fn shortcut_target_id(id: &str) -> String {
    format!("shortcut:{id}")
}

/// Returns `true` if `query_lower` (already trimmed and lowercased) appears in any candidate text.
pub fn matches_query(query_lower: &str, candidates: &[&str]) -> bool {
    if query_lower.is_empty() {
        return false;
    }
    candidates
        .iter()
        .any(|c| !c.is_empty() && c.to_lowercase().contains(query_lower))
}

fn translate_gettext(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        desk_settings::i18n::gettext(s)
    }
}

/// Search across all built-in and registry pages using the active gettext catalogue.
pub fn search(query: &str) -> Vec<SearchHit> {
    search_with(
        query,
        desk_settings::PAGES,
        BUILTIN_PAGES,
        translate_gettext,
    )
}

/// Pure search function: matches both English msgids and translated text produced by `translate`,
/// case-insensitively.
pub fn search_with(
    query: &str,
    pages: &[&'static Page],
    extra_pages: &[(&'static str, &'static str, &'static str)],
    translate: impl Fn(&str) -> String,
) -> Vec<SearchHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for &(id, title_en, sub_en) in extra_pages {
        let title_tr = translate(title_en);
        let sub_tr = translate(sub_en);
        if matches_query(&q, &[title_en, &title_tr, sub_en, &sub_tr]) {
            hits.push(SearchHit {
                page_id: id,
                page_title: title_tr.clone(),
                target_id: None,
                title: title_tr,
                subtitle: sub_tr,
            });
        }
    }
    for &page in pages {
        collect_page_hits(&q, page, &translate, &mut hits);
    }
    hits
}

fn collect_page_hits(
    q: &str,
    page: &'static Page,
    translate: &impl Fn(&str) -> String,
    hits: &mut Vec<SearchHit>,
) {
    let page_title_tr = translate(page.title);
    let page_sub_tr = translate(page.subtitle);
    let page_matches = matches_query(
        q,
        &[page.title, &page_title_tr, page.subtitle, &page_sub_tr],
    );

    if page_matches {
        hits.push(SearchHit {
            page_id: page.id,
            page_title: page_title_tr.clone(),
            target_id: None,
            title: page_title_tr.clone(),
            subtitle: page_sub_tr.clone(),
        });
    }

    if page.kind != PageKind::Settings {
        return;
    }

    for group in page.groups {
        for item in group.items {
            match item {
                Item::Key(k) if k.widget != Widget::Hidden => {
                    let label_tr = translate(k.label);
                    let desc_tr = translate(k.description);
                    if page_matches
                        || matches_query(q, &[k.label, &label_tr, k.description, &desc_tr])
                    {
                        hits.push(SearchHit {
                            page_id: page.id,
                            page_title: page_title_tr.clone(),
                            target_id: Some(key_target_id(k.schema, k.name)),
                            title: label_tr,
                            subtitle: page_title_tr.clone(),
                        });
                    }
                }
                Item::Shortcut(sc) => {
                    let label_tr = translate(sc.label);
                    if page_matches || matches_query(q, &[sc.label, &label_tr]) {
                        hits.push(SearchHit {
                            page_id: page.id,
                            page_title: page_title_tr.clone(),
                            target_id: Some(shortcut_target_id(sc.id)),
                            title: label_tr,
                            subtitle: page_title_tr.clone(),
                        });
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(s: &str) -> String {
        s.to_string()
    }

    fn mock_uk(s: &str) -> String {
        match s {
            "Clipboard" => "Буфер обміну".into(),
            "History size" => "Розмір історії".into(),
            "How many unpinned items to keep" => "Скільки незакріплених записів зберігати".into(),
            "Private mode" => "Приватний режим".into(),
            "System" => "Система".into(),
            other => other.to_string(),
        }
    }

    #[test]
    fn empty_and_whitespace_query_returns_no_hits() {
        assert!(search_with("", desk_settings::PAGES, BUILTIN_PAGES, identity).is_empty());
        assert!(search_with("   \t\n ", desk_settings::PAGES, BUILTIN_PAGES, identity).is_empty());
    }

    #[test]
    fn matches_english_case_insensitively() {
        let hits = search_with(
            "HISTORY SIZE",
            desk_settings::PAGES,
            BUILTIN_PAGES,
            identity,
        );
        assert!(hits.iter().any(|h| {
            h.page_id == "clipboard"
                && h.target_id.as_deref()
                    == Some(&key_target_id(
                        desk_settings::CLIPBOARD_SCHEMA,
                        "history-size",
                    ))
        }));
    }

    #[test]
    fn matches_translated_text_case_insensitively() {
        let hits = search_with("БУФЕР", desk_settings::PAGES, BUILTIN_PAGES, mock_uk);
        assert!(
            hits.iter()
                .any(|h| h.page_id == "clipboard" && h.target_id.is_none())
        );
        assert!(
            hits.iter()
                .any(|h| h.page_id == "clipboard" && h.target_id.is_some())
        );

        let size_hits = search_with("розмір", desk_settings::PAGES, BUILTIN_PAGES, mock_uk);
        assert!(size_hits.iter().any(|h| {
            h.page_id == "clipboard" && h.title == "Розмір історії" && h.target_id.is_some()
        }));
    }

    #[test]
    fn matches_english_msgid_even_when_translated() {
        let hits = search_with("private mode", desk_settings::PAGES, BUILTIN_PAGES, mock_uk);
        assert!(hits.iter().any(|h| {
            h.page_id == "clipboard"
                && h.title == "Приватний режим"
                && h.target_id.as_deref()
                    == Some(&key_target_id(
                        desk_settings::CLIPBOARD_SCHEMA,
                        "private-mode",
                    ))
        }));
    }

    #[test]
    fn matches_key_description_and_page_subtitle() {
        let desc_hits = search_with("unpinned", desk_settings::PAGES, BUILTIN_PAGES, identity);
        assert!(desc_hits.iter().any(|h| {
            h.page_id == "clipboard"
                && h.target_id.as_deref()
                    == Some(&key_target_id(
                        desk_settings::CLIPBOARD_SCHEMA,
                        "history-size",
                    ))
        }));

        let sub_hits = search_with("Win+V", desk_settings::PAGES, BUILTIN_PAGES, identity);
        assert!(
            sub_hits
                .iter()
                .any(|h| h.page_id == "clipboard" && h.target_id.is_none())
        );
    }

    #[test]
    fn skips_hidden_keys() {
        let hits = search_with(
            "toggle-shortcut",
            desk_settings::PAGES,
            BUILTIN_PAGES,
            identity,
        );
        assert!(hits.is_empty());
        let hidden_target = key_target_id(desk_settings::CLIPBOARD_SCHEMA, "toggle-shortcut");
        let clip_hits = search_with("clipboard", desk_settings::PAGES, BUILTIN_PAGES, identity);
        assert!(
            clip_hits
                .iter()
                .all(|h| h.target_id.as_deref() != Some(hidden_target.as_str()))
        );
    }

    #[test]
    fn matches_builtin_pages_in_english_and_translation() {
        let en_hits = search_with("system", desk_settings::PAGES, BUILTIN_PAGES, mock_uk);
        assert!(
            en_hits
                .iter()
                .any(|h| h.page_id == "system" && h.target_id.is_none())
        );

        let uk_hits = search_with("система", desk_settings::PAGES, BUILTIN_PAGES, mock_uk);
        assert!(
            uk_hits
                .iter()
                .any(|h| h.page_id == "system" && h.target_id.is_none())
        );
    }
}
