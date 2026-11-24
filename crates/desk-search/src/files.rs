// SPDX-License-Identifier: GPL-3.0-or-later
//! The local files provider (`docs/spec/features/search.md`, `. text` and `file:`): a small,
//! bounded index of the user's XDG folders (Desktop, Documents, Downloads, Pictures, Music,
//! Videos), kept in memory by the daemon.
//!
//! The scan is breadth first over all roots together, so a large folder cannot starve the
//! others. It is bounded on purpose:
//!
//! * at most [`FileLimits::max_depth`] folder levels below a root and
//!   [`FileLimits::max_entries`] entries in total, so building takes a few milliseconds and a
//!   query over the index stays far below 5 ms;
//! * hidden names (`.git`, `.cache`, ...) and heavy build folders ([`SKIPPED_DIRS`]) are skipped;
//! * symbolic links are **never** followed (no loops, no escape from the roots): a link is listed
//!   as an item but not entered, and a root that is a link is not scanned;
//! * names that are not UTF-8 are skipped (the result must carry the exact path).

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use crate::rank;
use crate::{Provider, ResultKind, SearchResult, W_FILE};

/// Folder names that are never entered nor listed (build output and dependency caches).
pub const SKIPPED_DIRS: &[&str] = &["node_modules", "target", "__pycache__"];

/// The XDG user folders the index scans, as `user-dirs.dirs` keys with their default names.
pub const USER_DIR_KEYS: &[(&str, &str)] = &[
    ("XDG_DESKTOP_DIR", "Desktop"),
    ("XDG_DOCUMENTS_DIR", "Documents"),
    ("XDG_DOWNLOAD_DIR", "Downloads"),
    ("XDG_PICTURES_DIR", "Pictures"),
    ("XDG_MUSIC_DIR", "Music"),
    ("XDG_VIDEOS_DIR", "Videos"),
];

/// How far a scan goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileLimits {
    /// Folder levels entered below a root (0: only the root's own entries).
    pub max_depth: usize,
    /// Entries kept in total, over all roots.
    pub max_entries: usize,
}

impl Default for FileLimits {
    fn default() -> FileLimits {
        FileLimits {
            max_depth: 3,
            max_entries: 2000,
        }
    }
}

/// One indexed file or folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// Absolute path, UTF-8.
    pub path: String,
    /// The file name as shown.
    pub name: String,
    /// The file name folded for matching ([`rank::fold`]).
    folded: String,
    pub is_dir: bool,
}

/// The in-memory index of the files provider.
#[derive(Clone, Debug, Default)]
pub struct FileIndex {
    entries: Vec<FileEntry>,
    /// The folders that were read, roots first (the daemon watches some of them).
    dirs: Vec<PathBuf>,
}

impl FileIndex {
    /// Scan `roots` breadth first within `limits`. Missing or unreadable folders are skipped.
    pub fn scan(roots: &[PathBuf], limits: FileLimits) -> FileIndex {
        let mut index = FileIndex::default();
        let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
        for root in roots {
            // a root that is a link (or missing) is not scanned: links are never followed
            let is_real_dir = fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_dir());
            if is_real_dir && !queue.iter().any(|(d, _)| d == root) {
                queue.push_back((root.clone(), 0));
            }
        }
        while let Some((dir, depth)) = queue.pop_front() {
            if index.entries.len() >= limits.max_entries {
                break;
            }
            let Ok(read) = fs::read_dir(&dir) else {
                continue;
            };
            index.dirs.push(dir);
            // sorted, so the same tree always gives the same index under the budget
            let mut children: Vec<(String, PathBuf, fs::FileType)> = read
                .filter_map(Result::ok)
                .filter_map(|e| {
                    // `DirEntry::file_type` does not follow links
                    let kind = e.file_type().ok()?;
                    let name = e.file_name().into_string().ok()?;
                    Some((name, e.path(), kind))
                })
                .filter(|(name, _, _)| !skipped(name))
                .collect();
            children.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, path, kind) in children {
                if index.entries.len() >= limits.max_entries {
                    break;
                }
                let Some(path_text) = path.to_str().map(str::to_owned) else {
                    continue;
                };
                let is_dir = kind.is_dir();
                if is_dir && depth < limits.max_depth {
                    queue.push_back((path, depth + 1));
                }
                index.entries.push(FileEntry {
                    path: path_text,
                    folded: rank::fold(&name),
                    name,
                    is_dir,
                });
            }
        }
        index
    }

    /// The indexed entries, in scan order.
    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// The folders that were read, in scan order (roots first).
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Up to `limit` entries matching `text`, best first. `weight` (percent) scales the scores,
    /// so files rank below applications in the plain query. An empty `text` finds nothing.
    pub fn search(&self, text: &str, limit: usize, weight: u32) -> Vec<SearchResult> {
        let folded = rank::fold(text);
        if folded.is_empty() || limit == 0 {
            return Vec::new();
        }
        let variants = rank::variants(&folded);
        let mut hits: Vec<(u32, &FileEntry)> = self
            .entries
            .iter()
            .filter_map(|e| {
                let score = variants
                    .iter()
                    .filter_map(|(q, penalty)| {
                        let s = rank::score_folded(q, &e.folded)?;
                        Some((s * weight / 100).saturating_sub(*penalty))
                    })
                    .max()?;
                Some((score, e))
            })
            .collect();
        // best score, then folders before files, then the shorter (shallower) path
        hits.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.is_dir.cmp(&a.1.is_dir))
                .then_with(|| a.1.path.len().cmp(&b.1.path.len()))
                .then_with(|| a.1.path.cmp(&b.1.path))
        });
        hits.into_iter()
            .take(limit)
            .map(|(score, e)| file_result(e, score))
            .collect()
    }
}

impl Provider for FileIndex {
    fn kind(&self) -> ResultKind {
        ResultKind::File
    }

    fn search(&self, text: &str, scoped: bool, limit: usize) -> Vec<SearchResult> {
        FileIndex::search(self, text, limit, if scoped { 100 } else { W_FILE })
    }
}

/// Hidden names and heavy build folders.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || SKIPPED_DIRS.contains(&name)
}

fn file_result(e: &FileEntry, score: u32) -> SearchResult {
    let parent = Path::new(&e.path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or_default()
        .to_owned();
    SearchResult {
        id: format!("file:{}", e.path),
        kind: ResultKind::File,
        title: e.name.clone(),
        subtitle: parent,
        icon: if e.is_dir {
            "folder-symbolic"
        } else {
            "text-x-generic-symbolic"
        }
        .to_owned(),
        activate: e.path.clone(),
        score,
    }
}

/// The XDG user folders to scan: the [`USER_DIR_KEYS`] read from `user_dirs_file` (the text of
/// `$XDG_CONFIG_HOME/user-dirs.dirs`, `None` when it is missing), else their default names below
/// `home`. A folder set to `$HOME` itself is dropped (the index never scans the whole home).
pub fn user_dirs(home: &Path, user_dirs_file: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (key, default) in USER_DIR_KEYS {
        let dir = user_dirs_file
            .and_then(|text| user_dir_value(text, key, home))
            .unwrap_or_else(|| home.join(default));
        if dir != home && dir.is_absolute() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// The value of `key` in a `user-dirs.dirs` text: `KEY="$HOME/path"` or `KEY="/abs/path"`.
fn user_dir_value(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    text.lines().rev().find_map(|line| {
        let rest = line
            .trim()
            .strip_prefix(key)?
            .trim_start()
            .strip_prefix('=')?;
        let value = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
        if value == "$HOME" || value == "$HOME/" {
            return Some(home.to_path_buf());
        }
        if let Some(rel) = value.strip_prefix("$HOME/") {
            return Some(home.join(rel));
        }
        value.starts_with('/').then(|| PathBuf::from(value))
    })
}

/// The roots of this session: `$HOME` and `$XDG_CONFIG_HOME/user-dirs.dirs`.
pub fn session_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let text = fs::read_to_string(config.join("user-dirs.dirs")).ok();
    user_dirs(&home, text.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::tests::tmp;

    fn touch(p: &Path) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, b"x").unwrap();
    }

    fn names(index: &FileIndex) -> Vec<&str> {
        index.entries().iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn skips_hidden_and_heavy_folders() {
        let home = tmp("files-skip");
        let docs = home.join("Documents");
        touch(&docs.join("report.odt"));
        touch(&docs.join(".secret.txt"));
        touch(&docs.join(".git/config"));
        touch(&docs.join("app/node_modules/pkg/index.js"));
        touch(&docs.join("app/target/debug/app"));
        touch(&docs.join("app/__pycache__/x.pyc"));
        touch(&docs.join("app/main.rs"));
        let index = FileIndex::scan(&[docs], FileLimits::default());
        let mut got = names(&index);
        got.sort_unstable();
        assert_eq!(got, ["app", "main.rs", "report.odt"]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn depth_and_entry_budget_are_capped() {
        let home = tmp("files-depth");
        let docs = home.join("Documents");
        touch(&docs.join("a/b/c/d/deep.txt"));
        let index = FileIndex::scan(
            std::slice::from_ref(&docs),
            FileLimits {
                max_depth: 2,
                max_entries: 100,
            },
        );
        // a (depth 0), b (1), c (2) are listed; c is not entered
        assert_eq!(names(&index), ["a", "b", "c"]);

        for i in 0..50 {
            touch(&docs.join(format!("f{i:02}.txt")));
        }
        let index = FileIndex::scan(
            &[docs],
            FileLimits {
                max_depth: 3,
                max_entries: 10,
            },
        );
        assert_eq!(index.len(), 10);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn roots_are_scanned_breadth_first_together() {
        let home = tmp("files-bfs");
        let docs = home.join("Documents");
        let pics = home.join("Pictures");
        for i in 0..20 {
            touch(&docs.join(format!("sub/n{i:02}.txt")));
        }
        touch(&pics.join("photo.png"));
        let index = FileIndex::scan(
            &[docs, pics],
            FileLimits {
                max_depth: 3,
                max_entries: 5,
            },
        );
        // the shallow entries of both roots come before the deep ones of the first
        assert!(names(&index).contains(&"photo.png"), "{:?}", names(&index));
        let _ = fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_never_followed() {
        use std::os::unix::fs::symlink;
        let home = tmp("files-link");
        let docs = home.join("Documents");
        touch(&docs.join("loop/inner.txt"));
        // a loop back to the root and a link out of the roots
        symlink(&docs, docs.join("loop/back")).unwrap();
        touch(&home.join("outside/private.txt"));
        symlink(home.join("outside"), docs.join("out")).unwrap();
        // a root that is a link is not scanned
        symlink(home.join("outside"), home.join("Music")).unwrap();

        let index = FileIndex::scan(&[docs, home.join("Music")], FileLimits::default());
        let mut got = names(&index);
        got.sort_unstable();
        assert_eq!(got, ["back", "inner.txt", "loop", "out"]);
        assert!(index.entries().iter().all(|e| !e.path.contains("private")));
        let links: Vec<_> = index.entries().iter().filter(|e| e.is_dir).collect();
        assert_eq!(links.len(), 1, "links are listed as plain items");
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn search_ranks_and_describes_results() {
        let home = tmp("files-rank");
        let docs = home.join("Documents");
        touch(&docs.join("budget.ods"));
        touch(&docs.join("old/budget 2024.ods"));
        touch(&docs.join("my budget notes.txt"));
        fs::create_dir_all(docs.join("Budget")).unwrap();
        touch(&docs.join("unrelated.txt"));
        let index = FileIndex::scan(std::slice::from_ref(&docs), FileLimits::default());

        let r = index.search("budget", 10, 100);
        let titles: Vec<&str> = r.iter().map(|x| x.title.as_str()).collect();
        // exact folder name, then prefixes (shorter first), then the word prefix
        assert_eq!(
            titles,
            [
                "Budget",
                "budget.ods",
                "budget 2024.ods",
                "my budget notes.txt"
            ]
        );
        let first = &r[0];
        assert_eq!(first.kind, ResultKind::File);
        assert_eq!(first.icon, "folder-symbolic");
        let budget = docs.join("Budget");
        assert_eq!(first.activate, budget.to_str().unwrap());
        assert_eq!(first.subtitle, docs.to_str().unwrap());
        assert_eq!(first.id, format!("file:{}", first.activate));

        // the weight scales; the limit cuts; an empty query finds nothing
        assert!(index.search("budget", 10, 50)[0].score < first.score);
        assert_eq!(index.search("budget", 2, 100).len(), 2);
        assert!(index.search("  ", 10, 100).is_empty());
        // the Ukrainian layout typo of «budget» still finds it
        assert_eq!(index.search("игвпуе", 1, 100)[0].title, "Budget");
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn user_dirs_reads_the_xdg_file_with_defaults() {
        let home = Path::new("/home/user");
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Стільниця\"\nXDG_DOCUMENTS_DIR=\"/data/docs\"\n\
                    XDG_DOWNLOAD_DIR=\"$HOME\"\nXDG_MUSIC_DIR=\"relative\"\n";
        assert_eq!(
            user_dirs(home, Some(text)),
            [
                home.join("Стільниця"),
                PathBuf::from("/data/docs"),
                home.join("Pictures"),
                home.join("Music"),
                home.join("Videos"),
            ]
        );
        assert_eq!(user_dirs(home, None).len(), USER_DIR_KEYS.len());
    }
}
