// SPDX-License-Identifier: GPL-3.0-or-later
//! Search engine of deskd (`docs/spec/features/search.md`): the settings that matter here and
//! the glue around the pure-Rust core `desk-search`. The application list is read once and kept
//! in memory; a change in an `applications/` folder only drops it (the next query reads it
//! again), so nothing runs while nobody searches.
//!
//! The other providers: the clipboard history (the clipboard engine's in-memory history, shared
//! with it), emoji (the emoji engine's offline data) and files (a bounded index of the XDG user
//! folders, `desk_search::files`). The file index is built on the first query that needs it and
//! dropped when a watched folder changes or when it is older than [`FILES_MAX_AGE`] (checked on a
//! query, never by a timer).

pub mod providers;
pub mod service;

use std::fmt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use desk_search::query::{self, Scope};
use desk_search::{
    Catalog, FileIndex, FileLimits, ParseOptions, Provider, SearchOptions, SearchResult,
    UsageStore, desktop, files,
};
use desk_settings::i18n::gettext;

use self::providers::{ClipboardProvider, EmojiProvider};

/// The oldest a file index may be when a query uses it, in seconds. The folder watchers see only
/// the roots and their direct subfolders; deeper changes show up after this.
pub const FILES_MAX_AGE: u64 = 10 * 60;
/// The most folders watched for the file index.
pub const MAX_FILE_WATCHES: usize = 64;

/// What the search settings give the engine (the shortcut is the bridge's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub web_search: bool,
    pub max_results: u32,
    pub remember_usage: bool,
}

impl Default for Config {
    /// The schema defaults.
    fn default() -> Config {
        Config {
            enabled: true,
            web_search: false,
            max_results: 12,
            remember_usage: true,
        }
    }
}

/// Why a search call failed.
#[derive(Debug)]
pub enum SearchError {
    /// The `enabled` setting is off.
    Disabled,
    /// The core refused or failed (bad item id, I/O, JSON).
    Core(desk_search::Error),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::Disabled => f.write_str("search is turned off"),
            SearchError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl From<desk_search::Error> for SearchError {
    fn from(e: desk_search::Error) -> Self {
        SearchError::Core(e)
    }
}

/// Unix seconds now (the clock of the usage decay).
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The engine: settings, usage store and the cached catalog.
pub struct Search {
    config: Config,
    usage: UsageStore,
    app_dirs: Vec<PathBuf>,
    parse: ParseOptions,
    /// `None` until the first query and after an `applications/` folder changed.
    catalog: Option<Catalog>,
    /// The XDG user folders the file index scans.
    file_roots: Vec<PathBuf>,
    file_limits: FileLimits,
    /// The file index and when it was built (unix seconds); `None` until a query needs it and
    /// after a watched folder changed.
    files: Option<(FileIndex, u64)>,
    /// The clipboard engine's history; `None` in a build or test without it.
    clipboard: Option<crate::service::Shared>,
}

impl Search {
    pub fn new(
        config: Config,
        usage_path: PathBuf,
        app_dirs: Vec<PathBuf>,
        file_roots: Vec<PathBuf>,
        parse: ParseOptions,
    ) -> Search {
        let usage = UsageStore::load(usage_path, config.remember_usage);
        Search {
            config,
            usage,
            app_dirs,
            parse,
            catalog: None,
            file_roots,
            file_limits: FileLimits::default(),
            files: None,
            clipboard: None,
        }
    }

    /// The engine of this session: the XDG data and user dirs, the session's language and the
    /// clipboard history of the clipboard engine.
    pub fn for_session(config: Config, clipboard: crate::service::Shared) -> Search {
        let mut s = Search::new(
            config,
            UsageStore::default_path(),
            desktop::application_dirs(&desktop::data_dirs()),
            files::session_roots(),
            ParseOptions::from_env(),
        );
        s.clipboard = Some(clipboard);
        s
    }

    /// Search this clipboard history too.
    #[cfg(test)]
    pub fn set_clipboard(&mut self, clipboard: crate::service::Shared) {
        self.clipboard = Some(clipboard);
    }

    /// The folders to watch for the file index: the roots and their direct subfolders (not
    /// hidden, not skipped, not links), at most [`MAX_FILE_WATCHES`].
    pub fn file_watch_dirs(&self) -> Vec<PathBuf> {
        let top = FileIndex::scan(
            &self.file_roots,
            FileLimits {
                max_depth: 0,
                max_entries: MAX_FILE_WATCHES,
            },
        );
        let mut dirs = self.file_roots.clone();
        dirs.extend(
            top.entries()
                .iter()
                .filter(|e| e.is_dir)
                .map(|e| PathBuf::from(&e.path)),
        );
        dirs.truncate(MAX_FILE_WATCHES);
        dirs
    }

    /// The `applications/` folders to watch.
    pub fn app_dirs(&self) -> &[PathBuf] {
        &self.app_dirs
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Take new settings; turning `remember-usage` off deletes the usage file.
    pub fn set_config(&mut self, config: Config) {
        self.usage.set_remember(config.remember_usage);
        self.config = config;
    }

    /// An `applications/` folder changed: read the entries again on the next query.
    pub fn invalidate(&mut self) {
        self.catalog = None;
    }

    /// A watched user folder changed: scan the files again on the next query that needs them.
    pub fn invalidate_files(&mut self) {
        self.files = None;
    }

    /// Read the applications and the files now if they are not cached (so the first query is
    /// fast).
    pub fn warm(&mut self) {
        cached(&mut self.catalog, &self.app_dirs, &self.parse);
        self.refresh_files(now());
    }

    /// Build the file index when there is none or it is older than [`FILES_MAX_AGE`] (or from
    /// the future: the clock moved back).
    fn refresh_files(&mut self, now: u64) {
        let fresh = self
            .files
            .as_ref()
            .is_some_and(|(_, built)| *built <= now && now - built < FILES_MAX_AGE);
        if !fresh {
            self.files = Some((FileIndex::scan(&self.file_roots, self.file_limits), now));
        }
    }

    fn check_enabled(&self) -> Result<(), SearchError> {
        if self.config.enabled {
            Ok(())
        } else {
            Err(SearchError::Disabled)
        }
    }

    /// The effective limit: 0 means the setting; more than the setting is cut to it.
    fn limit(&self, asked: u32) -> usize {
        let setting = self.config.max_results.clamp(5, 50);
        let n = if asked == 0 {
            setting
        } else {
            asked.min(setting)
        };
        usize::try_from(n).unwrap_or(12)
    }

    /// Answer a query as JSON.
    pub fn query_json(
        &mut self,
        text: &str,
        max_results: u32,
        now: u64,
    ) -> Result<String, SearchError> {
        let results = self.query(text, max_results, now)?;
        Ok(serde_json::to_string(&results).map_err(desk_search::Error::from)?)
    }

    /// Answer a query.
    pub fn query(
        &mut self,
        text: &str,
        max_results: u32,
        now: u64,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.check_enabled()?;
        let limit = self.limit(max_results);
        let scope = query::parse(text).scope;
        if matches!(scope, Scope::All | Scope::Files) {
            self.refresh_files(now);
        }
        let opts = SearchOptions {
            max_results: limit,
            web_search: self.config.web_search,
            web_title: gettext("Search the web for “{query}”"),
        };
        let catalog = cached(&mut self.catalog, &self.app_dirs, &self.parse);
        // lock order: search, then clipboard (the clipboard engine never locks search)
        let clipboard = self.clipboard.as_ref().map(crate::service::lock);
        let clip = clipboard.as_deref().map(ClipboardProvider);
        let emoji = EmojiProvider {
            entries: crate::emoji::data::dataset(),
            lang: if desk_settings::i18n::ui_language() == "uk" {
                "uk"
            } else {
                "en"
            },
        };
        let mut providers: Vec<&dyn Provider> = vec![&emoji];
        if let Some((index, _)) = &self.files {
            providers.push(index);
        }
        if let Some(c) = &clip {
            providers.push(c);
        }
        Ok(catalog.search_with(text, &self.usage, &opts, now, &providers))
    }

    /// Count one activation (nothing is written with `remember-usage` off). Returns true when
    /// it was recorded.
    pub fn record_launch(&mut self, item_id: &str, now: u64) -> Result<bool, SearchError> {
        self.check_enabled()?;
        Ok(self.usage.record(item_id, now)?)
    }

    /// Forget all usage counts and delete the file (allowed while search is off).
    pub fn clear_usage(&mut self) -> Result<(), SearchError> {
        Ok(self.usage.clear()?)
    }
}

/// The cached catalog, read from `dirs` when there is none.
fn cached<'a>(
    slot: &'a mut Option<Catalog>,
    dirs: &[PathBuf],
    parse: &ParseOptions,
) -> &'a Catalog {
    slot.get_or_insert_with(|| Catalog::new(desktop::scan(dirs, parse), gettext))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use desk_search::ResultKind;

    use super::*;

    /// A fresh, empty folder for one test.
    pub(crate) fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deskd-search-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("applications")).unwrap();
        d
    }

    pub(crate) fn write_app(dir: &Path, file: &str, name: &str) {
        std::fs::write(
            dir.join("applications").join(file),
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec={file}\nIcon=x\n"),
        )
        .unwrap();
    }

    pub(crate) fn engine_in(dir: &Path, config: Config) -> Search {
        write_app(dir, "org.gnome.TextEditor.desktop", "Text Editor");
        write_app(dir, "org.example.Clipper.desktop", "Clipboard History");
        Search::new(
            config,
            dir.join("data/search-usage.json"),
            vec![dir.join("applications")],
            vec![dir.join("Documents")],
            ParseOptions::default(),
        )
    }

    fn ids(r: &[SearchResult]) -> Vec<&str> {
        r.iter().map(|x| x.id.as_str()).collect()
    }

    #[test]
    fn queries_route_and_rank() {
        let dir = tmp("route");
        let mut s = engine_in(&dir, Config::default());
        let r = s.query("text ed", 0, 0).unwrap();
        assert_eq!(r[0].id, "app:org.gnome.TextEditor.desktop");
        assert_eq!(s.query("= 12*(3+4)", 0, 0).unwrap()[0].activate, "84");
        let r = s.query("сдшзищфкв", 0, 0).unwrap();
        assert!(
            ids(&r).contains(&"app:org.example.Clipper.desktop"),
            "{:?}",
            ids(&r)
        );
        let r = s.query(": heart", 3, 0).unwrap();
        assert!(!r.is_empty() && r.len() <= 3);
        assert!(
            r.iter()
                .all(|x| x.kind == ResultKind::Emoji && !x.activate.is_empty())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn files_scope_and_plain_queries_find_documents() {
        let dir = tmp("files");
        touch(&dir.join("Documents/Quarterly report.odt"));
        touch(&dir.join("Documents/.hidden report.odt"));
        touch(&dir.join("Documents/node_modules/report.js"));
        let mut s = engine_in(&dir, Config::default());
        let want = dir.join("Documents/Quarterly report.odt");
        for q in [". report", "file:report", "FILE: quarterly"] {
            let r = s.query(q, 0, 0).unwrap();
            assert_eq!(ids(&r).len(), 1, "{q}: {:?}", ids(&r));
            assert_eq!(r[0].kind, ResultKind::File);
            assert_eq!(r[0].activate, want.to_str().unwrap());
            assert_eq!(r[0].subtitle, dir.join("Documents").to_str().unwrap());
        }
        // plain text: the file is found, below a matching application
        write_app(&dir, "org.example.Reporter.desktop", "Report Studio");
        s.invalidate();
        let r = s.query("report", 0, 0).unwrap();
        let kinds: Vec<ResultKind> = r.iter().map(|x| x.kind).collect();
        assert_eq!(kinds[..2], [ResultKind::App, ResultKind::File], "{kinds:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_index_is_cached_until_invalidated_or_old() {
        let dir = tmp("files-cache");
        touch(&dir.join("Documents/first.txt"));
        let mut s = engine_in(&dir, Config::default());
        assert_eq!(s.query(". first", 0, 100).unwrap().len(), 1);
        touch(&dir.join("Documents/second.txt"));
        assert!(s.query(". second", 0, 100).unwrap().is_empty(), "cached");
        s.invalidate_files();
        assert_eq!(s.query(". second", 0, 100).unwrap().len(), 1);
        touch(&dir.join("Documents/third.txt"));
        assert!(s.query(". third", 0, 100).unwrap().is_empty(), "cached");
        // older than the limit, or the clock moved back: scanned again
        assert_eq!(s.query(". third", 0, 100 + FILES_MAX_AGE).unwrap().len(), 1);
        touch(&dir.join("Documents/fourth.txt"));
        assert_eq!(s.query(". fourth", 0, 5).unwrap().len(), 1);
        // the watch list: the root and its direct subfolders
        std::fs::create_dir_all(dir.join("Documents/Work")).unwrap();
        assert_eq!(
            s.file_watch_dirs(),
            [dir.join("Documents"), dir.join("Documents/Work")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clipboard_results_respect_private_mode() {
        let dir = tmp("clip");
        let mut s = engine_in(&dir, Config::default());
        let mut history = crate::clipboard::Engine::new(
            crate::clipboard::Config::default(),
            dir.join("clip-data"),
            dir.join("clip-run"),
        );
        history.push("text/plain", b"deploy token 42", &[], "", 1);
        let shared = Arc::new(Mutex::new(history));
        s.set_clipboard(shared.clone());
        let id = crate::service::lock(&shared).entries()[0].id;

        let r = s.query("# token", 0, 0).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, ResultKind::Clipboard);
        assert_eq!(r[0].activate, id.to_string());
        assert_eq!(r[0].title, "deploy token 42");
        let r = s.query("deploy", 0, 0).unwrap();
        assert!(r.iter().any(|x| x.kind == ResultKind::Clipboard));

        crate::service::lock(&shared).cfg.private = true;
        assert!(s.query("# token", 0, 0).unwrap().is_empty());
        let r = s.query("deploy", 0, 0).unwrap();
        assert!(r.iter().all(|x| x.kind != ResultKind::Clipboard));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_queries_include_emoji() {
        let dir = tmp("emoji-plain");
        let mut s = engine_in(&dir, Config::default());
        let r = s.query("heart", 0, 0).unwrap();
        assert!(
            r.iter().any(|x| x.kind == ResultKind::Emoji),
            "{:?}",
            ids(&r)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn limits_follow_the_setting() {
        let dir = tmp("limit");
        let mut s = engine_in(
            &dir,
            Config {
                max_results: 5,
                ..Config::default()
            },
        );
        assert_eq!(s.query(">", 0, 0).unwrap().len(), 5);
        assert_eq!(s.query(">", 2, 0).unwrap().len(), 2);
        assert_eq!(s.query(">", 40, 0).unwrap().len(), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_entries_appear_after_invalidate() {
        let dir = tmp("invalidate");
        let mut s = engine_in(&dir, Config::default());
        s.warm();
        write_app(&dir, "org.example.Maps.desktop", "Maps");
        assert!(s.query("maps", 0, 0).unwrap().is_empty(), "cached");
        s.invalidate();
        assert_eq!(
            s.query("maps", 0, 0).unwrap()[0].id,
            "app:org.example.Maps.desktop"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ac_srch_06_web_result_only_when_enabled() {
        let dir = tmp("web");
        let mut s = engine_in(&dir, Config::default());
        let r = s.query("weather", 0, 0).unwrap();
        assert!(r.iter().all(|x| x.kind != ResultKind::Web));
        s.set_config(Config {
            web_search: true,
            ..Config::default()
        });
        let r = s.query("weather", 0, 0).unwrap();
        assert_eq!(r.last().unwrap().kind, ResultKind::Web);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ac_srch_07_usage_follows_remember_usage() {
        let dir = tmp("usage");
        let file = dir.join("data/search-usage.json");
        let mut s = engine_in(
            &dir,
            Config {
                remember_usage: false,
                ..Config::default()
            },
        );
        assert!(
            !s.record_launch("app:org.gnome.TextEditor.desktop", 0)
                .unwrap()
        );
        assert!(!file.exists());

        s.set_config(Config::default());
        assert!(
            s.record_launch("app:org.gnome.TextEditor.desktop", 0)
                .unwrap()
        );
        assert!(file.exists());
        assert_eq!(
            ids(&s.query("", 0, 0).unwrap()),
            ["app:org.gnome.TextEditor.desktop"]
        );
        s.clear_usage().unwrap();
        assert!(!file.exists());
        assert!(s.query("", 0, 0).unwrap().is_empty());

        s.record_launch("action:lock", 0).unwrap();
        s.set_config(Config {
            remember_usage: false,
            ..Config::default()
        });
        assert!(!file.exists(), "turning it off deletes the file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabled_refuses_but_clear_still_works() {
        let dir = tmp("off");
        let mut s = engine_in(
            &dir,
            Config {
                enabled: false,
                ..Config::default()
            },
        );
        assert!(matches!(s.query("x", 0, 0), Err(SearchError::Disabled)));
        assert!(matches!(
            s.record_launch("calc", 0),
            Err(SearchError::Disabled)
        ));
        s.clear_usage().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
