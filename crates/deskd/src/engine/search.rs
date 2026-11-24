// SPDX-License-Identifier: GPL-3.0-or-later
//! The search engine: the D-Bus object, the GSettings watcher and the watchers of the
//! `applications/` folders and the XDG user folders. Idle cost is zero: no thread or timer; a
//! settings change, a desktop entry change or a file change (inotify through GIO) only updates or
//! drops in-memory state.

use std::sync::{Arc, Mutex};

use desk_proto::SEARCH_PATH;
use desk_settings::accessors::SearchSettings;
use gio::prelude::*;
use zbus::connection::Builder;

use super::{Engine, RuntimeCtx, Setup};
use crate::search::Search;
use crate::search::service::{self, SearchIface, Shared};
use crate::settings;
use crate::trust::Gate;

pub struct SearchEngine {
    eng: Shared,
    gate: Gate,
    settings: Option<SearchSettings>,
    /// Kept alive while deskd runs; dropping one stops its watch.
    monitors: Vec<gio::FileMonitor>,
}

impl SearchEngine {
    /// `clipboard` is the clipboard engine's history, searched by `# text` and plain queries.
    pub fn new(setup: &Setup, clipboard: crate::service::Shared) -> SearchEngine {
        let settings = settings::search_settings();
        let cfg = settings
            .as_ref()
            .map_or_else(Default::default, settings::read_search);
        SearchEngine {
            eng: Arc::new(Mutex::new(Search::for_session(cfg, clipboard))),
            gate: setup.gate.clone(),
            settings,
            monitors: Vec::new(),
        }
    }
}

impl SearchEngine {
    /// Run `on_change` on the engine when `dir` changes (a missing folder is watched too).
    fn watch(&mut self, dir: &std::path::Path, on_change: fn(&mut Search)) {
        let file = gio::File::for_path(dir);
        let Ok(monitor) =
            file.monitor_directory(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
        else {
            return;
        };
        let eng = self.eng.clone();
        monitor.connect_changed(move |_, _, _, _| on_change(&mut service::lock(&eng)));
        self.monitors.push(monitor);
    }
}

impl Engine for SearchEngine {
    fn register<'a>(&mut self, builder: Builder<'a>) -> Builder<'a> {
        builder.serve_at(
            SEARCH_PATH,
            SearchIface {
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
                if matches!(
                    key,
                    "enabled" | "web-search" | "max-results" | "remember-usage"
                ) {
                    service::lock(&eng).set_config(settings::read_search(&typed));
                }
            });
        }

        let dirs = service::lock(&self.eng).app_dirs().to_vec();
        for dir in dirs {
            self.watch(&dir, Search::invalidate);
        }
        let file_dirs = service::lock(&self.eng).file_watch_dirs();
        for dir in file_dirs {
            self.watch(&dir, Search::invalidate_files);
        }

        // read the desktop entries once in the background, so the first query is fast
        if service::lock(&self.eng).enabled() {
            let eng = self.eng.clone();
            std::thread::spawn(move || service::lock(&eng).warm());
        }
    }

    fn shutdown(&mut self) {
        for m in self.monitors.drain(..) {
            m.cancel();
        }
    }
}
