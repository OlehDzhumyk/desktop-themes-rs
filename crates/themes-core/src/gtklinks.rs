// SPDX-License-Identifier: GPL-3.0-or-later
//! Символьні посилання ~/.config/gtk-{4,3}.0/* → тема, з резервними копіями витіснених файлів.
//! Files generated from tokens.toml are copied instead (the cache in ~/.cache is invisible to
//! Flatpak apps that only get `xdg-config/gtk-4.0:ro`).

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

fn restore_backup(st: &mut State, dest: &str, warnings: &mut Vec<String>) -> Result<()> {
    if let Some(b) = st.backups.remove(dest) {
        let bp = Path::new(&b);
        if lexists(bp) {
            if !lexists(Path::new(dest)) {
                move_path(bp, Path::new(dest))?;
            } else {
                warnings.push(format!("резервну копію {b} не відновлено (місце зайняте)"));
            }
        }
    }
    Ok(())
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
            warnings.push(format!("{dest} змінено вручну — не оновлено"));
            continue;
        }
        if std::fs::copy(&c.src, d).is_ok() {
            c.hash = src_hash;
            done.push(dest.clone());
        }
    }
    done
}

pub fn unlink_all(st: &mut State, warnings: &mut Vec<String>) -> Result<()> {
    let copies: Vec<(String, GtkCopy)> = std::mem::take(&mut st.gtk_copies).into_iter().collect();
    for (dest, c) in copies {
        let d = Path::new(&dest);
        if lexists(d) {
            if !d.is_symlink() && file_hash(d).as_deref() == Some(c.hash.as_str()) {
                std::fs::remove_file(d)?;
            } else {
                warnings.push(format!("{dest} змінено вручну — залишено як є"));
            }
        }
        restore_backup(st, &dest, warnings)?;
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
            _ if lexists(d) => warnings.push(format!("{dest} змінено вручну — залишено як є")),
            _ => {}
        }
        st.links.remove(&dest);
        if let Some(b) = st.backups.remove(&dest) {
            let bp = Path::new(&b);
            if lexists(bp) {
                if !lexists(d) {
                    move_path(bp, d)?;
                } else {
                    warnings.push(format!("резервну копію {b} не відновлено (місце зайняте)"));
                }
            }
        }
    }
    // осиротілі резервні копії
    let backups: Vec<(String, String)> = st
        .backups
        .iter()
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();
    for (dest, b) in backups {
        if lexists(Path::new(&b)) && !lexists(Path::new(&dest)) {
            move_path(Path::new(&b), Path::new(&dest))?;
            st.backups.remove(&dest);
        }
    }
    st.created_dirs.retain(|d| {
        let p = Path::new(d);
        if p.is_dir()
            && std::fs::read_dir(p)
                .map(|mut r| r.next().is_none())
                .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(p);
        }
        p.exists()
    });
    Ok(())
}

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
            std::fs::create_dir_all(&tdir)?;
            let s = tdir.to_string_lossy().into_owned();
            if !st.created_dirs.contains(&s) {
                st.created_dirs.push(s);
            }
        }
        for e in entries {
            let src = src_dir.join(&e);
            let dest = tdir.join(&e);
            if lexists(&dest) {
                let bdir = ctx.backups.join(tdir.file_name().unwrap_or_default());
                std::fs::create_dir_all(&bdir)?;
                let mut b = bdir.join(&e);
                if lexists(&b) {
                    b = bdir.join(format!("{e}.{}", now("%Y%m%d-%H%M%S")));
                }
                move_path(&dest, &b)?;
                st.backups
                    .insert(dest.to_string_lossy().into(), b.to_string_lossy().into());
            }
            if generated {
                std::fs::copy(&src, &dest)?;
                let hash = file_hash(&dest).unwrap_or_default();
                st.gtk_copies.insert(
                    dest.to_string_lossy().into(),
                    GtkCopy {
                        src: src.to_string_lossy().into(),
                        hash,
                    },
                );
            } else {
                std::os::unix::fs::symlink(&src, &dest)?;
                st.links
                    .insert(dest.to_string_lossy().into(), src.to_string_lossy().into());
            }
            made.push(dest.to_string_lossy().into_owned());
        }
    }
    Ok(made)
}
