// SPDX-License-Identifier: GPL-3.0-or-later
//! Символьні посилання ~/.config/gtk-{4,3}.0/* → тема, з резервними копіями витіснених файлів.
//! Files generated from tokens.toml are copied instead (the cache in ~/.cache is invisible to
//! Flatpak apps that only get `xdg-config/gtk-4.0:ro`).

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

fn file_hash(p: &Path) -> Option<String> {
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
fn tmp_name(dest: &Path) -> PathBuf {
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
/// is saved at once, so an interruption leaves a state this function can finish later.
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
    // осиротілі резервні копії
    let backups: Vec<String> = st.backups.keys().cloned().collect();
    for dest in backups {
        let b = st.backups[&dest].clone();
        if !lexists(Path::new(&b)) || !lexists(Path::new(&dest)) {
            restore_backup(ctx, st, &dest, warnings)?;
        }
    }
    let before = st.created_dirs.len();
    st.created_dirs.retain(|d| {
        let p = Path::new(d);
        if p.is_dir() && std::fs::read_dir(p).is_ok_and(|mut r| r.next().is_none()) {
            let _ = std::fs::remove_dir(p);
        }
        p.exists()
    });
    if st.created_dirs.len() != before {
        st.save(ctx)?;
    }
    Ok(())
}

/// Links (hand-made GTK files) or copies (generated ones) the theme's GTK files. The user's
/// file in the way is recorded as a backup BEFORE it is moved, and every new link or copy is
/// recorded before it is made, so `unlink_all` can always undo what exists.
pub fn link_theme(ctx: &Ctx, st: &mut State, theme: &Theme) -> Result<Vec<String>> {
    let mut made = Vec::new();
    for kind in GTK_KINDS {
        let generated = theme.handmade_gtk_dir(kind).is_none();
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
            if generated {
                // written beside the destination and renamed: never half a file in place
                let tmp = tmp_name(&dest);
                std::fs::copy(&src, &tmp)?;
                let hash = file_hash(&tmp).unwrap_or_default();
                st.gtk_copies.insert(
                    dkey.clone(),
                    GtkCopy {
                        src: src.to_string_lossy().into(),
                        hash,
                    },
                );
                st.save(ctx)?;
                if let Err(e) = std::fs::rename(&tmp, &dest) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(e.into());
                }
            } else {
                st.links.insert(dkey.clone(), src.to_string_lossy().into());
                st.save(ctx)?;
                std::os::unix::fs::symlink(&src, &dest)?;
            }
            made.push(dkey);
        }
    }
    Ok(made)
}
