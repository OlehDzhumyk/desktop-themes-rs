// SPDX-License-Identifier: GPL-3.0-or-later
//! GTK stylesheet and asset copies in `~/.config/gtk-{4,3}.0/*`, with backups of any user files
//! they displace. All GTK files (both generated from `tokens.toml` and hand-made themes such as
//! WhiteSur/Fluent) are copied rather than symlinked: Flatpak apps granted
//! `xdg-config/gtk-4.0:ro` cannot follow symlinks pointing outside that directory. Legacy
//! symlinks recorded in `State::links` are still removed cleanly by [`unlink_all`].

use desk_settings::tr;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::paths::Ctx;
use crate::state::{GtkCopy, State};
use crate::theme::{GTK_KINDS, Theme};
use crate::util::{lexists, move_path, now};

fn target_dir(ctx: &Ctx, kind: &str) -> PathBuf {
    if kind == "gtk4" {
        ctx.gtk4.clone()
    } else {
        ctx.gtk3.clone()
    }
}

pub(crate) fn file_hash(p: &Path) -> Option<String> {
    std::fs::read(p)
        .ok()
        .map(|b| format!("{:016x}", theme_tokens::cache::fnv1a64(&b)))
}

/// Moves the backup of `dest` back when the place is free; the record goes in any case (a
/// missing backup file means the move never happened). Saved after the change.
fn restore_backup(ctx: &Ctx, st: &mut State, dest: &str, warnings: &mut Vec<String>) -> Result<()> {
    let Some(b) = st.backups.get(dest).cloned() else {
        return Ok(());
    };
    let bp = Path::new(&b);
    if lexists(bp) {
        if lexists(Path::new(dest)) {
            warnings.push(tr!(
                "backup {path} not restored (the place is taken)",
                path = b
            ));
        } else {
            move_path(bp, Path::new(dest))?;
        }
    }
    st.backups.remove(dest);
    st.save(ctx)
}

/// Temporary name of a copy being written (renamed into place when complete).
pub(crate) fn tmp_name(dest: &Path) -> PathBuf {
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    dest.with_file_name(format!(".{name}.themes-tmp"))
}

/// Rewrites generated copies whose source changed (new cache generation); copies the user
/// edited are left alone. Returns the rewritten destinations.
pub fn refresh_copies(st: &mut State, warnings: &mut Vec<String>) -> Vec<String> {
    let mut done = Vec::new();
    for (dest, c) in st.gtk_copies.iter_mut() {
        let d = Path::new(dest);
        let Some(src_hash) = file_hash(Path::new(&c.src)) else {
            continue;
        };
        if src_hash == c.hash {
            continue;
        }
        if file_hash(d).as_deref() != Some(c.hash.as_str()) {
            warnings.push(tr!("{path} was changed by hand; not updated", path = dest));
            continue;
        }
        if std::fs::copy(&c.src, d).is_ok() {
            c.hash = src_hash;
            done.push(dest.clone());
        }
    }
    done
}

/// Removes the links and copies the manager made and puts the user's files back. Every change
/// is saved at once, so an interruption leaves a state this function can finish later. A live
/// wallpaper palette in a copy goes with the copy (its hash is recorded like any other).
pub fn unlink_all(ctx: &Ctx, st: &mut State, warnings: &mut Vec<String>) -> Result<()> {
    let copies: Vec<(String, GtkCopy)> = st
        .gtk_copies
        .iter()
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();
    for (dest, c) in copies {
        let d = Path::new(&dest);
        let _ = std::fs::remove_file(tmp_name(d));
        if lexists(d) {
            if !d.is_symlink() && file_hash(d).as_deref() == Some(c.hash.as_str()) {
                std::fs::remove_file(d)?;
            } else {
                warnings.push(tr!(
                    "{path} was changed by hand; left as it is",
                    path = dest
                ));
            }
        }
        st.gtk_copies.remove(&dest);
        st.save(ctx)?;
        restore_backup(ctx, st, &dest, warnings)?;
    }
    crate::overlay::forget(ctx, st)?;
    let links: Vec<(String, String)> = st
        .links
        .iter()
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();
    for (dest, src) in links {
        let d = Path::new(&dest);
        match std::fs::read_link(d) {
            Ok(t) if t == Path::new(&src) => std::fs::remove_file(d)?,
            _ if lexists(d) => warnings.push(tr!(
                "{path} was changed by hand; left as it is",
                path = dest
            )),
            _ => {}
        }
        st.links.remove(&dest);
        st.save(ctx)?;
        restore_backup(ctx, st, &dest, warnings)?;
    }
    let before = st.created_dirs.len();
    // remove deepest subdirectories first so parent directories become empty too,
    // before restoring backed-up user directories (such as a user's own `assets/`)
    let mut dirs = st.created_dirs.clone();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.len()));
    for d in &dirs {
        let p = Path::new(d);
        if p.is_dir() && std::fs::read_dir(p).is_ok_and(|mut r| r.next().is_none()) {
            let _ = std::fs::remove_dir(p);
        }
    }
    st.created_dirs.retain(|d| Path::new(d).exists());
    if st.created_dirs.len() != before {
        st.save(ctx)?;
    }
    // backed-up directories (whose replacement dirs were just cleaned up) and orphan backups
    let backups: Vec<String> = st.backups.keys().cloned().collect();
    for dest in backups {
        let b = st.backups[&dest].clone();
        if !lexists(Path::new(&b)) || !lexists(Path::new(&dest)) {
            restore_backup(ctx, st, &dest, warnings)?;
        }
    }
    Ok(())
}

/// Walks `src` (file or directory) into sorted subdirectories and `(src, dest)` file pairs.
fn collect_files(
    src: &Path,
    dest: &Path,
    dirs: &mut Vec<PathBuf>,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<()> {
    if src.is_dir() {
        dirs.push(dest.to_path_buf());
        let mut entries: Vec<_> = std::fs::read_dir(src)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name();
            collect_files(&entry.path(), &dest.join(name), dirs, files)?;
        }
    } else if src.is_file() {
        files.push((src.to_path_buf(), dest.to_path_buf()));
    }
    Ok(())
}

/// Copies the theme's GTK files (and asset subdirectories) into `~/.config/gtk-{4,3}.0`. The
/// user's file or directory in the way is recorded as a backup BEFORE it is moved, and every new
/// directory and copy is recorded in `state.json` BEFORE it is written, so `unlink_all` can
/// always undo what exists.
pub fn link_theme(ctx: &Ctx, st: &mut State, theme: &Theme) -> Result<Vec<String>> {
    let mut made = Vec::new();
    for kind in GTK_KINDS {
        let entries = theme.gtk_entries(kind);
        let Some(src_dir) = theme.gtk_dir(kind).filter(|_| !entries.is_empty()) else {
            continue;
        };
        let tdir = target_dir(ctx, kind);
        if !tdir.is_dir() {
            let s = tdir.to_string_lossy().into_owned();
            if !st.created_dirs.contains(&s) {
                st.created_dirs.push(s);
                st.save(ctx)?;
            }
            std::fs::create_dir_all(&tdir)?;
        }
        for e in entries {
            let src = src_dir.join(&e);
            let dest = tdir.join(&e);
            let dkey: String = dest.to_string_lossy().into();
            if lexists(&dest) {
                let bdir = ctx.backups.join(tdir.file_name().unwrap_or_default());
                std::fs::create_dir_all(&bdir)?;
                let mut b = bdir.join(&e);
                if lexists(&b) {
                    b = bdir.join(format!("{e}.{}", now("%Y%m%d-%H%M%S")));
                }
                st.backups.insert(dkey.clone(), b.to_string_lossy().into());
                st.save(ctx)?;
                move_path(&dest, &b)?;
            }
            let mut dirs = Vec::new();
            let mut files = Vec::new();
            collect_files(&src, &dest, &mut dirs, &mut files)?;
            for d in &dirs {
                if !d.is_dir() {
                    let s = d.to_string_lossy().into_owned();
                    if !st.created_dirs.contains(&s) {
                        st.created_dirs.push(s);
                    }
                }
            }
            for (fsrc, fdest) in &files {
                let fkey: String = fdest.to_string_lossy().into();
                let hash = file_hash(fsrc).unwrap_or_default();
                st.gtk_copies.insert(
                    fkey,
                    GtkCopy {
                        src: fsrc.to_string_lossy().into(),
                        hash,
                    },
                );
            }
            st.save(ctx)?;
            for d in &dirs {
                std::fs::create_dir_all(d)?;
            }
            for (fsrc, fdest) in files {
                // written beside the destination and renamed: never half a file in place
                let tmp = tmp_name(&fdest);
                std::fs::copy(&fsrc, &tmp)?;
                if let Err(err) = std::fs::rename(&tmp, &fdest) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(err.into());
                }
                made.push(fdest.to_string_lossy().into_owned());
            }
        }
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tests::{tmp_root, write};

    #[test]
    fn copies_handmade_files_and_asset_subdirs_and_restores_backups() {
        let root = tmp_root("gtklinks-tree");
        let ctx = Ctx::new(root.join("home"), root.clone());
        write(&ctx.themes.join("win/theme.conf"), "NAME=Win\n");
        write(&ctx.themes.join("win/gtk4/gtk.css"), "/* fluent */\n");
        write(&ctx.themes.join("win/gtk4/assets/check.png"), "PNG1");
        write(&ctx.themes.join("win/gtk4/assets/sub/dot.png"), "PNG2");
        write(&ctx.gtk4.join("gtk.css"), "/* mine */\n");
        write(&ctx.gtk4.join("assets/mine.png"), "MINE");

        let th = Theme::new(&ctx, "win");
        let mut st = State::load(&ctx);
        let made = link_theme(&ctx, &mut st, &th).unwrap();
        assert_eq!(made.len(), 3);
        assert!(st.links.is_empty());
        assert_eq!(st.gtk_copies.len(), 3);
        assert!(!ctx.gtk4.join("gtk.css").is_symlink());
        assert!(!ctx.gtk4.join("assets").is_symlink());
        assert_eq!(
            std::fs::read_to_string(ctx.gtk4.join("gtk.css")).unwrap(),
            "/* fluent */\n"
        );
        assert_eq!(
            std::fs::read(ctx.gtk4.join("assets/sub/dot.png")).unwrap(),
            b"PNG2"
        );

        // revert removes all copied files and nested directories and restores user backups
        let mut w = Vec::new();
        unlink_all(&ctx, &mut st, &mut w).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert!(st.gtk_copies.is_empty() && st.backups.is_empty());
        assert!(!ctx.gtk4.join("assets/sub").exists());
        assert_eq!(
            std::fs::read_to_string(ctx.gtk4.join("gtk.css")).unwrap(),
            "/* mine */\n"
        );
        assert_eq!(
            std::fs::read(ctx.gtk4.join("assets/mine.png")).unwrap(),
            b"MINE"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
