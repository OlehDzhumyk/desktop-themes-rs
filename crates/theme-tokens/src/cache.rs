// SPDX-License-Identifier: GPL-3.0-or-later
//! Content-addressed cache of generated files: `<cache>/<theme>/` holds the output and a
//! `.stamp` with the key (FNV-1a 64 of the generator version, the target and the merged,
//! canonical tokens). Same key and all files present → nothing is rendered or written.
//! A new generation is written to a sibling temp directory and swapped in with a rename, so
//! paths linked from ~/.config and ~/.local/share stay valid.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::generate::{self, Outputs, Target};
use crate::load::Loaded;

/// Bump when any generator's output changes for the same tokens.
pub const GENERATOR_VERSION: u32 = 1;
const STAMP: &str = ".stamp";

/// FNV-1a 64 of `data`: the one implementation lives in `desk_settings::stable_hash`.
pub use desk_settings::stable_hash::fnv1a64_bytes as fnv1a64;

pub fn cache_key(l: &Loaded, tg: &Target) -> String {
    let o = Outputs::for_target(&l.tokens, tg);
    let s = format!(
        "v{GENERATOR_VERSION}\n{}\n{}\n{}\n{}\n{:?}\n{}",
        tg.theme_id,
        tg.theme_name,
        tg.home,
        tg.cache_dir.display(),
        o,
        l.merged
    );
    format!("{:016x}", fnv1a64(s.as_bytes()))
}

/// Relative paths the tokens produce (without rendering them).
pub fn planned(l: &Loaded, tg: &Target) -> Vec<String> {
    let o = Outputs::for_target(&l.tokens, tg);
    let mut v = Vec::new();
    if o.gtk {
        v.push(generate::GTK_CSS.to_string());
    }
    if o.shell {
        v.push(generate::SHELL_CSS.to_string());
    }
    if o.ptyxis {
        v.push(generate::ptyxis_rel(&tg.theme_id));
    }
    if o.wallpaper {
        v.push(generate::WALLPAPER_LIGHT.to_string());
        v.push(generate::WALLPAPER_DARK.to_string());
    }
    v
}

#[derive(Clone, Debug)]
pub struct Materialized {
    pub dir: PathBuf,
    pub key: String,
    pub files: Vec<PathBuf>,
    /// Output was already up to date.
    pub cached: bool,
    pub elapsed: Duration,
}

pub fn is_current(l: &Loaded, tg: &Target) -> bool {
    let key = cache_key(l, tg);
    stamp_matches(&tg.cache_dir, &key, &planned(l, tg))
}

fn stamp_matches(dir: &Path, key: &str, files: &[String]) -> bool {
    let Ok(s) = std::fs::read_to_string(dir.join(STAMP)) else {
        return false;
    };
    s.lines().next() == Some(key) && files.iter().all(|f| dir.join(f).is_file())
}

/// Ensures `tg.cache_dir` holds the output for `l` (renders only when the key changed or
/// `force`). A theme whose tokens produce no files gets its cache directory removed.
pub fn materialize(l: &Loaded, tg: &Target, force: bool) -> io::Result<Materialized> {
    let t0 = Instant::now();
    let dir = tg.cache_dir.clone();
    let key = cache_key(l, tg);
    let rels = planned(l, tg);
    let files: Vec<PathBuf> = rels.iter().map(|r| dir.join(r)).collect();
    if rels.is_empty() {
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        return Ok(Materialized {
            dir,
            key,
            files,
            cached: true,
            elapsed: t0.elapsed(),
        });
    }
    if !force && stamp_matches(&dir, &key, &rels) {
        return Ok(Materialized {
            dir,
            key,
            files,
            cached: true,
            elapsed: t0.elapsed(),
        });
    }
    let parent = dir.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    for f in generate::render(&l.tokens, tg) {
        let p = tmp.join(&f.rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, f.contents)?;
    }
    std::fs::write(tmp.join(STAMP), format!("{key}\n{}\n", rels.join("\n")))?;
    let old = parent.join(format!(".{name}.old-{}", std::process::id()));
    if dir.exists() {
        std::fs::rename(&dir, &old)?;
    }
    std::fs::rename(&tmp, &dir)?;
    if old.exists() {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(Materialized {
        dir,
        key,
        files,
        cached: false,
        elapsed: t0.elapsed(),
    })
}
