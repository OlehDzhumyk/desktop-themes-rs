// SPDX-License-Identifier: GPL-3.0-or-later
//! Update a git-based install: the checkout that `data/install.sh` recorded in `<root>/repo-path`,
//! its state (clean, on a branch, upstream, ahead/behind) and the commits waiting upstream.
//! `themes update` runs `scripts/update.sh` of that checkout; `themes doctor` shows a problem.

use desk_settings::tr;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::paths::Ctx;
use crate::util::run;

/// The file in the theme root that holds the absolute path of the checkout.
pub const REPO_PATH_FILE: &str = "repo-path";
/// The script that pulls, builds and installs (relative to the checkout).
pub const UPDATE_SCRIPT: &str = "scripts/update.sh";
/// A file only this project has: tells our checkout from any other git repository.
const MARKER: &str = "data/io.github.OlehDzhumyk.Themes.desktop.in";
/// How many commit subjects `themes update --check` shows.
pub const MAX_SUBJECTS: usize = 10;

/// Why the recorded checkout cannot be updated. [`RepoProblem::message`] says how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoProblem {
    NotRecorded(PathBuf),
    Unreadable(PathBuf, String),
    NotAbsolute(PathBuf, String),
    Missing(PathBuf),
    NotGit(PathBuf),
    NotOurs(PathBuf),
    Dirty(PathBuf, Vec<String>),
    Detached(PathBuf),
    NoUpstream(PathBuf),
    Diverged {
        repo: PathBuf,
        upstream: String,
        ahead: u32,
        behind: u32,
    },
}

impl RepoProblem {
    pub fn message(&self) -> String {
        match self {
            Self::NotRecorded(f) => tr!(
                "No git checkout is recorded in {file}. Install from a checkout with scripts/bootstrap.sh, or run scripts/update.sh in it once. A package install updates with the package manager.",
                file = f.display()
            ),
            Self::Unreadable(f, e) => {
                tr!("Cannot read {file}: {error}", file = f.display(), error = e)
            }
            Self::NotAbsolute(f, v) => tr!(
                "{file} does not hold an absolute path (“{value}”). Run scripts/update.sh in your checkout once to record it again.",
                file = f.display(),
                value = v
            ),
            Self::Missing(p) => tr!(
                "The recorded checkout {path} does not exist any more. Run scripts/update.sh in your checkout once to record its new place.",
                path = p.display()
            ),
            Self::NotGit(p) => tr!(
                "{path} is not a git repository. Clone the project again and run scripts/bootstrap.sh in it.",
                path = p.display()
            ),
            Self::NotOurs(p) => tr!(
                "{path} is not a desktop-themes-rs checkout. Run scripts/update.sh in the right checkout once.",
                path = p.display()
            ),
            Self::Dirty(p, files) => tr!(
                "The checkout {path} has local changes ({count}, for example {file}). Commit or stash them, then update again.",
                path = p.display(),
                count = files.len(),
                file = files.first().map_or("", String::as_str)
            ),
            Self::Detached(p) => tr!(
                "The checkout {path} is not on a branch. Run: git -C {path} switch main",
                path = p.display()
            ),
            Self::NoUpstream(p) => tr!(
                "The branch in {path} has no upstream. Run: git -C {path} branch --set-upstream-to=origin/main",
                path = p.display()
            ),
            Self::Diverged {
                repo,
                upstream,
                ahead,
                behind,
            } => tr!(
                "The branch in {path} and {upstream} have diverged ({ahead} local, {behind} new commits). Rebase or reset it, then update again.",
                path = repo.display(),
                upstream = upstream,
                ahead = ahead,
                behind = behind
            ),
        }
    }
}

impl From<RepoProblem> for Error {
    fn from(p: RepoProblem) -> Self {
        Error(p.message())
    }
}

pub fn repo_path_file(ctx: &Ctx) -> PathBuf {
    ctx.root.join(REPO_PATH_FILE)
}

/// The content of `repo-path`: one absolute path (surrounding white space is ignored).
pub fn parse_repo_path(file: &Path, content: &str) -> std::result::Result<PathBuf, RepoProblem> {
    let v = content.trim();
    let p = PathBuf::from(v);
    if v.is_empty() || !p.is_absolute() {
        return Err(RepoProblem::NotAbsolute(file.to_path_buf(), v.to_string()));
    }
    Ok(p)
}

/// The directory exists, is a git repository (`.git` is a directory, or a file in a worktree)
/// and is this project.
pub fn check_layout(repo: &Path) -> std::result::Result<(), RepoProblem> {
    if !repo.is_dir() {
        return Err(RepoProblem::Missing(repo.to_path_buf()));
    }
    if !repo.join(".git").exists() {
        return Err(RepoProblem::NotGit(repo.to_path_buf()));
    }
    if !repo.join(MARKER).is_file() || !repo.join(UPDATE_SCRIPT).is_file() {
        return Err(RepoProblem::NotOurs(repo.to_path_buf()));
    }
    Ok(())
}

/// The checkout recorded in `repo-path`, checked with [`check_layout`].
pub fn recorded_repo(ctx: &Ctx) -> std::result::Result<PathBuf, RepoProblem> {
    let file = repo_path_file(ctx);
    let content = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RepoProblem::NotRecorded(file));
        }
        Err(e) => return Err(RepoProblem::Unreadable(file, e.to_string())),
    };
    let repo = parse_repo_path(&file, &content)?;
    check_layout(&repo)?;
    Ok(repo)
}

/// `git status --porcelain=v2 --branch`, parsed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BranchStatus {
    /// `None`: detached HEAD.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Changed or untracked paths.
    pub changes: Vec<String>,
}

pub fn parse_status(out: &str) -> BranchStatus {
    let mut s = BranchStatus::default();
    for line in out.lines() {
        if let Some(h) = line.strip_prefix("# branch.head ") {
            s.head = (h != "(detached)").then(|| h.to_string());
        } else if let Some(u) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(u.to_string());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for n in ab.split_whitespace() {
                if let Some(a) = n.strip_prefix('+') {
                    s.ahead = a.parse().unwrap_or(0);
                } else if let Some(b) = n.strip_prefix('-') {
                    s.behind = b.parse().unwrap_or(0);
                }
            }
        } else {
            // ordinary (1, 8 fields before the path), renamed (2, 9; "path\torig"),
            // unmerged (u, 10) and untracked (?) entries
            let path = match line.split_once(' ') {
                Some(("1", _)) => line.splitn(9, ' ').nth(8),
                Some(("2", _)) => line
                    .splitn(10, ' ')
                    .nth(9)
                    .map(|p| p.split('\t').next().unwrap_or(p)),
                Some(("u", _)) => line.splitn(11, ' ').nth(10),
                Some(("?", p)) => Some(p),
                _ => None,
            };
            if let Some(p) = path {
                s.changes.push(p.to_string());
            }
        }
    }
    s
}

impl BranchStatus {
    /// What stops a fast-forward update of `repo`, if anything.
    pub fn problem(&self, repo: &Path) -> Option<RepoProblem> {
        let repo = repo.to_path_buf();
        if !self.changes.is_empty() {
            return Some(RepoProblem::Dirty(repo, self.changes.clone()));
        }
        if self.head.is_none() {
            return Some(RepoProblem::Detached(repo));
        }
        let Some(upstream) = &self.upstream else {
            return Some(RepoProblem::NoUpstream(repo));
        };
        (self.ahead > 0 && self.behind > 0).then(|| RepoProblem::Diverged {
            repo,
            upstream: upstream.clone(),
            ahead: self.ahead,
            behind: self.behind,
        })
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let r = repo.to_string_lossy();
    let mut all = vec!["-C", r.as_ref()];
    all.extend_from_slice(args);
    let o = run("git", &all, None)?;
    if !o.ok {
        return Err(Error(tr!(
            "git {command} failed in {path}: {error}",
            command = args.join(" "),
            path = repo.display(),
            error = o.stderr
        )));
    }
    Ok(o.stdout)
}

pub fn status(repo: &Path) -> Result<BranchStatus> {
    git(repo, &["status", "--porcelain=v2", "--branch"]).map(|o| parse_status(&o))
}

/// The arguments for `scripts/update.sh`.
pub fn update_args(dry_run: bool) -> Vec<&'static str> {
    if dry_run {
        vec!["--dry-run"]
    } else {
        Vec::new()
    }
}

/// The commits upstream has and the checkout does not (newest first).
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct Behind {
    pub upstream: String,
    pub count: usize,
    /// At most [`MAX_SUBJECTS`].
    pub subjects: Vec<String>,
}

/// `git log --format=%s HEAD..@{u}` output → [`Behind`].
pub fn parse_behind(upstream: &str, log: &str) -> Behind {
    let all: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).collect();
    Behind {
        upstream: upstream.to_string(),
        count: all.len(),
        subjects: all
            .iter()
            .take(MAX_SUBJECTS)
            .map(|s| (*s).to_string())
            .collect(),
    }
}

/// `git fetch`, then the commits waiting upstream.
pub fn check(repo: &Path) -> Result<Behind> {
    git(repo, &["fetch", "--quiet"])?;
    let st = status(repo)?;
    if st.head.is_none() {
        return Err(RepoProblem::Detached(repo.to_path_buf()).into());
    }
    let Some(upstream) = st.upstream else {
        return Err(RepoProblem::NoUpstream(repo.to_path_buf()).into());
    };
    let log = git(repo, &["log", "--format=%s", "HEAD..@{u}"])?;
    Ok(parse_behind(&upstream, &log))
}

/// For `themes doctor`: a recorded checkout that `themes update` cannot use. `None` when
/// nothing is recorded (a package or tarball install) or all is well. No network.
pub fn doctor_note(ctx: &Ctx) -> Option<String> {
    let repo = match recorded_repo(ctx) {
        Ok(r) => r,
        Err(RepoProblem::NotRecorded(_)) => return None,
        Err(p) => return Some(p.message()),
    };
    let st = status(&repo).ok()?;
    st.problem(&repo).map(|p| p.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tests::{tmp_root, write};

    #[test]
    fn repo_path_must_be_absolute() {
        let f = Path::new("/r/repo-path");
        assert_eq!(
            parse_repo_path(f, "  /home/user/src/dtr\n"),
            Ok(PathBuf::from("/home/user/src/dtr"))
        );
        for bad in ["", "\n", "src/dtr"] {
            assert!(matches!(
                parse_repo_path(f, bad),
                Err(RepoProblem::NotAbsolute(..))
            ));
        }
    }

    #[test]
    fn layout_tells_missing_foreign_and_ours() {
        let r = tmp_root("update-layout");
        assert_eq!(
            check_layout(&r.join("gone")),
            Err(RepoProblem::Missing(r.join("gone")))
        );
        assert_eq!(check_layout(&r), Err(RepoProblem::NotGit(r.clone())));
        // a worktree has a .git file, not a directory
        write(&r.join(".git"), "gitdir: /elsewhere\n");
        assert_eq!(check_layout(&r), Err(RepoProblem::NotOurs(r.clone())));
        write(&r.join(MARKER), "");
        write(&r.join(UPDATE_SCRIPT), "");
        assert_eq!(check_layout(&r), Ok(()));
    }

    #[test]
    fn recorded_repo_reads_the_root() {
        let r = tmp_root("update-recorded");
        let ctx = Ctx::new(r.join("home"), r.join("root"));
        assert!(matches!(
            recorded_repo(&ctx),
            Err(RepoProblem::NotRecorded(_))
        ));
        assert_eq!(doctor_note(&ctx), None);
        let repo = r.join("checkout");
        write(&repo_path_file(&ctx), &format!("{}\n", repo.display()));
        assert_eq!(recorded_repo(&ctx), Err(RepoProblem::Missing(repo.clone())));
        assert!(doctor_note(&ctx).is_some());
    }

    const CLEAN: &str =
        "# branch.oid 0123\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -3\n";

    #[test]
    fn status_of_a_clean_branch() {
        let s = parse_status(CLEAN);
        assert_eq!(s.head.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (0, 3));
        assert!(s.changes.is_empty());
        assert_eq!(s.problem(Path::new("/r")), None);
    }

    #[test]
    fn status_lists_changed_paths() {
        let out = format!(
            "{CLEAN}1 .M N... 100644 100644 100644 aaa bbb docs/a file.md\n\
             2 R. N... 100644 100644 100644 aaa bbb R100 new.rs\told.rs\n\
             u UU N... 100644 100644 100644 100644 aaa bbb ccc Cargo.lock\n\
             ? notes.txt\n"
        );
        let s = parse_status(&out);
        assert_eq!(
            s.changes,
            ["docs/a file.md", "new.rs", "Cargo.lock", "notes.txt"]
        );
        assert!(matches!(
            s.problem(Path::new("/r")),
            Some(RepoProblem::Dirty(_, f)) if f.len() == 4
        ));
    }

    #[test]
    fn status_problems() {
        let r = Path::new("/r");
        let detached = parse_status("# branch.oid 0123\n# branch.head (detached)\n");
        assert_eq!(detached.problem(r), Some(RepoProblem::Detached(r.into())));
        let no_up = parse_status("# branch.oid 0123\n# branch.head topic\n");
        assert_eq!(no_up.problem(r), Some(RepoProblem::NoUpstream(r.into())));
        let ahead = parse_status(&CLEAN.replace("+0 -3", "+2 -0"));
        assert_eq!(ahead.problem(r), None);
        let diverged = parse_status(&CLEAN.replace("+0 -3", "+2 -3"));
        assert_eq!(
            diverged.problem(r),
            Some(RepoProblem::Diverged {
                repo: r.into(),
                upstream: "origin/main".into(),
                ahead: 2,
                behind: 3
            })
        );
    }

    #[test]
    fn behind_keeps_the_count_and_ten_subjects() {
        let log: String = (1..=12).map(|i| format!("commit {i}\n")).collect();
        let b = parse_behind("origin/main", &log);
        assert_eq!(b.count, 12);
        assert_eq!(b.subjects.len(), MAX_SUBJECTS);
        assert_eq!(b.subjects[0], "commit 1");
        assert_eq!(parse_behind("origin/main", "").count, 0);
    }

    #[test]
    fn update_args_pass_dry_run() {
        assert!(update_args(false).is_empty());
        assert_eq!(update_args(true), ["--dry-run"]);
    }
}
