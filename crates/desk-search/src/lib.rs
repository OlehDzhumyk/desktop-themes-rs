// SPDX-License-Identifier: GPL-3.0-or-later
//! The search engine core of desktop-themes-rs (`docs/spec/features/search.md`, stage 4).
//!
//! Pure Rust with no GTK, GLib, GNOME or D-Bus dependency (ADR 0011), so the GNOME bridge and a
//! later KDE or wlroots adapter share it through `deskd`:
//!
//! * [`query`]: the query language (`=`, `>`, `.`/`file:`, `#`, `:`, plain text), cut at 256
//!   characters;
//! * [`calc`]: a safe arithmetic evaluator (AC-SRCH-03);
//! * [`rank`]: tiered, layout-tolerant fuzzy matching (AC-SRCH-02, AC-SRCH-04);
//! * [`desktop`]: XDG desktop entries;
//! * [`actions`]: the built-in system actions;
//! * [`usage`]: the decayed usage counts in `search-usage.json` (AC-SRCH-07).
//!
//! [`Catalog::search`] ties them together. Files, clipboard history and emoji come from other
//! providers of the daemon; this crate never reads the network (AC-SRCH-06).

pub mod actions;
pub mod calc;
pub mod desktop;
pub mod query;
pub mod rank;
pub mod usage;

use std::cmp::Ordering;
use std::fmt;

use serde::Serialize;

pub use actions::{SYSTEM_ACTIONS, SystemAction};
pub use desktop::{DesktopEntry, ParseOptions};
pub use query::{Query, Scope};
pub use usage::UsageStore;

use rank::Field;

/// Marks an English string for gettext extraction (`xgettext -kn_`); the adapter translates it.
pub const fn n_(s: &'static str) -> &'static str {
    s
}

/// The lowest and highest number of results a query returns.
pub const MIN_RESULTS: usize = 1;
pub const MAX_RESULTS: usize = 50;

/// Field weights in percent (the name is 100).
const W_GENERIC: u32 = 80;
const W_KEYWORD: u32 = 70;
const W_PROGRAM: u32 = 60;
const W_COMMENT: u32 = 50;

/// Errors of the core.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// An empty, too long or control-character item id.
    InvalidId(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Json(e) => write!(f, "{e}"),
            Error::InvalidId(id) => write!(
                f,
                "invalid item id {id:?}: it must have 1 to {} bytes and no control characters",
                usage::MAX_ID_BYTES
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

/// What a result is; also the order of equal scores (applications before actions, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResultKind {
    Calculator,
    App,
    Action,
    File,
    Clipboard,
    Emoji,
    Web,
}

/// One result, serialized into the `Query` answer.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchResult {
    /// Stable id, also the key of the usage store: `app:<desktop id>`, `action:<id>`, `calc`,
    /// `emoji:<id>`, `web`.
    pub id: String,
    pub kind: ResultKind,
    pub title: String,
    pub subtitle: String,
    /// Theme icon name or absolute path; empty for none.
    pub icon: String,
    /// What activating it uses: the desktop id, the action id, the text to copy (calculator,
    /// emoji) or the query to open in the browser (web).
    pub activate: String,
    pub score: u32,
}

/// The per-query choices of the caller.
#[derive(Clone, Debug)]
pub struct SearchOptions {
    /// Clamped to [`MIN_RESULTS`]..=[`MAX_RESULTS`].
    pub max_results: usize,
    /// Offer a «search the web» result (AC-SRCH-06: off by default; activating it only opens the
    /// browser, the core makes no request).
    pub web_search: bool,
    /// The title of the web result; `{query}` is replaced by the query.
    pub web_title: String,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            max_results: 12,
            web_search: false,
            web_title: "Search the web for “{query}”".to_owned(),
        }
    }
}

/// A system action with its title in the interface language.
#[derive(Clone, Debug)]
pub struct LocalAction {
    pub action: &'static SystemAction,
    pub title: String,
}

/// What the providers of this crate search: the applications and the actions.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub apps: Vec<DesktopEntry>,
    pub actions: Vec<LocalAction>,
}

impl Catalog {
    /// A catalog with the built-in actions titled by `translate` (gettext in the daemon).
    pub fn new(apps: Vec<DesktopEntry>, translate: impl Fn(&'static str) -> String) -> Catalog {
        let actions = SYSTEM_ACTIONS
            .iter()
            .map(|a| LocalAction {
                action: a,
                title: translate(a.title),
            })
            .collect();
        Catalog { apps, actions }
    }

    /// Answer a raw query (the text of the field).
    pub fn search(
        &self,
        raw: &str,
        usage: &UsageStore,
        opts: &SearchOptions,
        now: u64,
    ) -> Vec<SearchResult> {
        let max = opts.max_results.clamp(MIN_RESULTS, MAX_RESULTS);
        let q = query::parse(raw);
        let mut out = match q.scope {
            Scope::Calculator => calc::evaluate(&q.text)
                .ok()
                .map(|e| calc_result(&q.text, calc::format_number(e.value)))
                .into_iter()
                .collect(),
            Scope::SystemActions => self.actions_for(&q.text, usage, now),
            Scope::All if q.text.is_empty() => self.recent(usage, now),
            Scope::All => {
                let mut ranked = self.apps_for(&q.text);
                ranked.extend(self.actions_for(&q.text, usage, now));
                sort(&mut ranked, usage, now);
                if let Some(value) = calc::implicit(&q.text) {
                    ranked.insert(0, calc_result(&q.text, value));
                }
                ranked
            }
            // other providers of the daemon answer these scopes
            Scope::Files | Scope::Clipboard | Scope::Emoji => Vec::new(),
        };
        if opts.web_search && q.scope == Scope::All && !q.text.is_empty() {
            out.truncate(max - 1);
            out.push(SearchResult {
                id: "web".to_owned(),
                kind: ResultKind::Web,
                title: opts.web_title.replace("{query}", &q.text),
                subtitle: String::new(),
                icon: "web-browser-symbolic".to_owned(),
                activate: q.text.clone(),
                score: 0,
            });
        }
        out.truncate(max);
        out
    }

    fn apps_for(&self, text: &str) -> Vec<SearchResult> {
        let variants = rank::variants(&rank::fold(text));
        self.apps
            .iter()
            .filter_map(|app| {
                let mut fields = vec![Field {
                    text: &app.name,
                    weight: 100,
                }];
                if let Some(local) = &app.localized_name {
                    fields.push(Field {
                        text: local,
                        weight: 100,
                    });
                }
                if let Some(generic) = &app.generic_name {
                    fields.push(Field {
                        text: generic,
                        weight: W_GENERIC,
                    });
                }
                fields.extend(app.keywords.iter().map(|k| Field {
                    text: k,
                    weight: W_KEYWORD,
                }));
                fields.push(Field {
                    text: app.program(),
                    weight: W_PROGRAM,
                });
                if let Some(comment) = &app.comment {
                    fields.push(Field {
                        text: comment,
                        weight: W_COMMENT,
                    });
                }
                let score = rank::score_item(&variants, &fields)?;
                Some(app_result(app, score))
            })
            .collect()
    }

    fn actions_for(&self, text: &str, usage: &UsageStore, now: u64) -> Vec<SearchResult> {
        if text.is_empty() {
            return self.actions.iter().map(|a| action_result(a, 0)).collect();
        }
        let variants = rank::variants(&rank::fold(text));
        let mut out: Vec<SearchResult> = self
            .actions
            .iter()
            .filter_map(|a| {
                let mut fields = vec![
                    Field {
                        text: &a.title,
                        weight: 100,
                    },
                    Field {
                        text: a.action.title,
                        weight: 100,
                    },
                ];
                fields.extend(
                    a.action
                        .keywords
                        .iter()
                        .chain(a.action.keywords_uk)
                        .map(|k| Field {
                            text: k,
                            weight: W_KEYWORD,
                        }),
                );
                let score = rank::score_item(&variants, &fields)?;
                Some(action_result(a, score))
            })
            .collect();
        sort(&mut out, usage, now);
        out
    }

    /// The empty plain query: the most used applications and actions, if any are remembered.
    fn recent(&self, usage: &UsageStore, now: u64) -> Vec<SearchResult> {
        usage
            .top(now)
            .into_iter()
            .filter_map(|(id, _)| self.by_id(id))
            .collect()
    }

    fn by_id(&self, id: &str) -> Option<SearchResult> {
        if let Some(desktop_id) = id.strip_prefix("app:") {
            return self
                .apps
                .iter()
                .find(|a| a.id == desktop_id)
                .map(|a| app_result(a, 0));
        }
        let action_id = id.strip_prefix("action:")?;
        self.actions
            .iter()
            .find(|a| a.action.id == action_id)
            .map(|a| action_result(a, 0))
    }
}

/// Best score first; equal scores by usage, then kind, then title.
fn sort(results: &mut [SearchResult], usage: &UsageStore, now: u64) {
    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| {
                usage
                    .weight(&b.id, now)
                    .partial_cmp(&usage.weight(&a.id, now))
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.title.cmp(&b.title))
    });
}

fn calc_result(expr: &str, value: String) -> SearchResult {
    SearchResult {
        id: "calc".to_owned(),
        kind: ResultKind::Calculator,
        title: value.clone(),
        subtitle: format!("= {expr}"),
        icon: "accessories-calculator-symbolic".to_owned(),
        activate: value,
        score: rank::EXACT + 1,
    }
}

fn app_result(app: &DesktopEntry, score: u32) -> SearchResult {
    SearchResult {
        id: format!("app:{}", app.id),
        kind: ResultKind::App,
        title: app.display_name().to_owned(),
        subtitle: app
            .generic_name
            .clone()
            .or_else(|| app.comment.clone())
            .unwrap_or_default(),
        icon: app.icon.clone(),
        activate: app.id.clone(),
        score,
    }
}

fn action_result(a: &LocalAction, score: u32) -> SearchResult {
    SearchResult {
        id: format!("action:{}", a.action.id),
        kind: ResultKind::Action,
        title: a.title.clone(),
        subtitle: String::new(),
        icon: a.action.icon.to_owned(),
        activate: a.action.id.to_owned(),
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::usage::tests::tmp;
    use super::*;

    fn app(
        id: &str,
        name: &str,
        generic: Option<&str>,
        keywords: &[&str],
        exec: &str,
    ) -> DesktopEntry {
        DesktopEntry {
            id: id.into(),
            name: name.into(),
            generic_name: generic.map(Into::into),
            keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
            exec: exec.into(),
            icon: "icon".into(),
            ..DesktopEntry::default()
        }
    }

    fn catalog() -> Catalog {
        let mut terminal = app(
            "org.gnome.Ptyxis.desktop",
            "Terminal",
            None,
            &["shell"],
            "ptyxis",
        );
        terminal.localized_name = Some("Термінал".into());
        Catalog::new(
            vec![
                app(
                    "org.gnome.TextEditor.desktop",
                    "Text Editor",
                    Some("Text Editor"),
                    &["text", "editor", "plaintext"],
                    "gnome-text-editor %U",
                ),
                app(
                    "org.example.Texture.desktop",
                    "Texture Studio",
                    None,
                    &[],
                    "texture",
                ),
                app(
                    "org.example.Edit.desktop",
                    "Edit Tool",
                    None,
                    &["text"],
                    "edit-tool",
                ),
                app(
                    "org.example.Clipper.desktop",
                    "Clipper",
                    Some("Clipboard manager"),
                    &["clipboard", "history"],
                    "clipper",
                ),
                terminal,
            ],
            str::to_owned,
        )
    }

    fn ids(results: &[SearchResult]) -> Vec<&str> {
        results.iter().map(|r| r.id.as_str()).collect()
    }

    fn off() -> UsageStore {
        UsageStore::disabled(tmp("off").join("u.json"))
    }

    #[test]
    fn ac_srch_02_text_ed_finds_text_editor_first() {
        let r = catalog().search("text ed", &off(), &SearchOptions::default(), 0);
        assert_eq!(r[0].id, "app:org.gnome.TextEditor.desktop");
        assert_eq!(r[0].activate, "org.gnome.TextEditor.desktop");
        assert_eq!(r[0].title, "Text Editor");
    }

    #[test]
    fn ac_srch_03_calculator_first_with_the_copy_text() {
        let c = catalog();
        let r = c.search("= 12*(3+4)", &off(), &SearchOptions::default(), 0);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, ResultKind::Calculator);
        assert_eq!(r[0].title, "84");
        assert_eq!(r[0].activate, "84");
        // a plain expression is a calculation too, ranked first
        let r = c.search("12*(3+4)", &off(), &SearchOptions::default(), 0);
        assert_eq!(r[0].activate, "84");
        // a bad expression in the calculator scope answers nothing
        assert!(
            c.search("= rm -rf", &off(), &SearchOptions::default(), 0)
                .is_empty()
        );
    }

    #[test]
    fn ac_srch_04_ukrainian_layout_finds_clipboard_results() {
        let r = catalog().search("сдшзищфкв", &off(), &SearchOptions::default(), 0);
        let found = ids(&r);
        assert!(
            found.contains(&"app:org.example.Clipper.desktop"),
            "{found:?}"
        );
        assert!(found.contains(&"action:clipboard-private"), "{found:?}");
        // and the other way round: Latin letters for a Ukrainian name
        let r = catalog().search("nthvsyfk", &off(), &SearchOptions::default(), 0);
        assert_eq!(r[0].id, "app:org.gnome.Ptyxis.desktop");
        assert_eq!(r[0].title, "Термінал");
    }

    #[test]
    fn ac_srch_06_no_web_result_unless_enabled() {
        let c = catalog();
        let r = c.search("weather", &off(), &SearchOptions::default(), 0);
        assert!(r.iter().all(|x| x.kind != ResultKind::Web));
        let opts = SearchOptions {
            web_search: true,
            max_results: 5,
            ..SearchOptions::default()
        };
        let r = c.search("text", &off(), &opts, 0);
        assert!(r.len() <= 5);
        let last = r.last().unwrap();
        assert_eq!(last.kind, ResultKind::Web);
        assert_eq!(last.title, "Search the web for “text”");
        assert_eq!(last.activate, "text");
        // never in a prefixed scope or for an empty query
        assert!(
            c.search("> lock", &off(), &opts, 0)
                .iter()
                .all(|x| x.kind != ResultKind::Web)
        );
        assert!(c.search("", &off(), &opts, 0).is_empty());
    }

    #[test]
    fn usage_breaks_ties_and_fills_the_empty_query() {
        let dir = tmp("ties");
        let mut usage = UsageStore::load(dir.join("u.json"), true);
        let c = Catalog::new(
            vec![
                app("a.desktop", "Notes Alpha", None, &[], "a"),
                app("b.desktop", "Notes Bravo", None, &[], "b"),
            ],
            str::to_owned,
        );
        let opts = SearchOptions::default();
        let r = c.search("notes", &usage, &opts, 0);
        assert_eq!(r[0].score, r[1].score);
        assert_eq!(ids(&r), ["app:a.desktop", "app:b.desktop"]);
        usage.record("app:b.desktop", 0).unwrap();
        assert_eq!(
            ids(&c.search("notes", &usage, &opts, 0)),
            ["app:b.desktop", "app:a.desktop"]
        );
        // a better match still wins over usage
        assert_eq!(
            c.search("notes alpha", &usage, &opts, 0)[0].id,
            "app:a.desktop"
        );

        assert_eq!(ids(&c.search("", &usage, &opts, 0)), ["app:b.desktop"]);
        assert!(c.search("", &off(), &opts, 0).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scopes_and_limits() {
        let c = catalog();
        let opts = SearchOptions::default();
        let all = c.search(">", &off(), &opts, 0);
        assert_eq!(all.len(), 7);
        assert!(all.iter().all(|r| r.kind == ResultKind::Action));
        assert_eq!(c.search("> lock", &off(), &opts, 0)[0].id, "action:lock");
        assert_eq!(c.search("> заблок", &off(), &opts, 0)[0].id, "action:lock");
        assert_eq!(
            c.search("> reboot", &off(), &opts, 0)[0].id,
            "action:restart"
        );
        // plain text finds actions too
        assert_eq!(
            c.search("suspend", &off(), &opts, 0)[0].id,
            "action:suspend"
        );
        for prefixed in [". notes", "file:notes", "# token", ": smile"] {
            assert!(
                c.search(prefixed, &off(), &opts, 0).is_empty(),
                "{prefixed}"
            );
        }
        let one = SearchOptions {
            max_results: 0,
            ..SearchOptions::default()
        };
        assert_eq!(c.search(">", &off(), &one, 0).len(), 1);
        // a 1000-character query is cut and does not panic
        assert!(c.search(&"text ".repeat(200), &off(), &opts, 0).len() <= 12);
    }

    #[test]
    fn translated_action_titles_are_searched_and_shown() {
        let c = Catalog::new(Vec::new(), |s| {
            if s == "Lock screen" {
                "Заблокувати екран".to_owned()
            } else {
                s.to_owned()
            }
        });
        let r = c.search("заблокувати е", &off(), &SearchOptions::default(), 0);
        assert_eq!(r[0].title, "Заблокувати екран");
        // the English title still matches
        let r = c.search("lock scr", &off(), &SearchOptions::default(), 0);
        assert_eq!(r[0].id, "action:lock");
    }

    #[test]
    fn results_serialize_for_the_bus() {
        let r = catalog().search("= 2+2", &off(), &SearchOptions::default(), 0);
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json[0]["kind"], "calculator");
        assert_eq!(json[0]["id"], "calc");
        assert_eq!(json[0]["title"], "4");
    }
}
