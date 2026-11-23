// SPDX-License-Identifier: GPL-3.0-or-later
//! The live wallpaper palette over a token theme (docs/spec/features/theme-from-wallpaper.md,
//! ADR 0009).
//!
//! While "Accent from wallpaper" is on, deskd runs `themes tokens overlay SEED`. When the look
//! theme's libadwaita stylesheet is generated from tokens (its `gtk.css` is a copy recorded in
//! `State::gtk_copies`), the theme's token chain is loaded with one more layer on top, the
//! tonal palette of the seed ([`palette::overlay_toml`]), and the result is rendered to
//! `<cache>/wallpaper-overlay/<theme>/gtk.css`. That file replaces the content of the same
//! copy in `~/.config/gtk-4.0` (copied, never linked: ADR 0004). The theme's own generated file
//! is remembered in [`Overlay::base_src`].
//!
//! Ways out, all exact:
//! * `remove` (live mode off, `themes doctor --fix`, deskd at start-up with the setting off):
//!   the theme's own generated file goes back into the copy, the record and the cache go;
//! * `gtklinks::unlink_all` (a new look theme, `themes reset`, a rollback): the copy is removed
//!   as usual (its hash is the overlay's) and [`forget`] drops the record and the cache.
//!
//! Hand-made stylesheets (WhiteSur, Fluent) are never touched: there is no copy to replace, and
//! the live mode only changes `accent-color` for those themes.

use std::path::{Path, PathBuf};

use desk_settings::tr;
use serde::{Deserialize, Serialize};
use theme_tokens::color::Rgba;
use theme_tokens::palette;

use crate::error::{Error, Result};
use crate::gtklinks::{file_hash, tmp_name};
use crate::lock::Lock;
use crate::modules::Module;
use crate::paths::Ctx;
use crate::state::{GtkCopy, State};
use crate::theme::get_theme;

/// Directory of the overlay inside the cache root.
pub const DIR: &str = "wallpaper-overlay";

/// The record in `state.json` while an overlay is installed.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Overlay {
    /// The look theme the overlay was rendered for.
    pub theme: String,
    /// The seed colour (`#rrggbb`).
    pub seed: String,
    /// The copy in `~/.config/gtk-4.0` that carries the overlay (a key of `gtk_copies`).
    pub dest: String,
    /// The theme's own generated stylesheet: what the copy holds again when the overlay goes.
    pub base_src: String,
}

/// What `install` did.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Outcome {
    /// The theme whose libadwaita palette now follows the wallpaper; `None`: accent only.
    pub theme: Option<String>,
    /// The overlay stylesheet in the cache.
    pub path: Option<String>,
    /// Why only the accent follows the wallpaper (empty when the overlay is installed).
    pub note: String,
}

impl Outcome {
    fn accent_only(note: String) -> Outcome {
        Outcome {
            theme: None,
            path: None,
            note,
        }
    }
}

pub fn dir(ctx: &Ctx) -> PathBuf {
    crate::tokens::cache_root(ctx).join(DIR)
}

fn gtk_css(ctx: &Ctx) -> String {
    ctx.gtk4.join("gtk.css").to_string_lossy().into_owned()
}

/// Copies `src` over `dest` through a temporary file (never half a file in place). Returns the
/// hash of what was written.
fn copy_into(src: &Path, dest: &Path) -> Result<String> {
    let tmp = tmp_name(dest);
    std::fs::copy(src, &tmp)?;
    let hash = file_hash(&tmp).unwrap_or_default();
    if let Err(e) = std::fs::rename(&tmp, dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(hash)
}

fn write_atomic(p: &Path, text: &str) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = tmp_name(p);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

/// Renders the overlay of `seed` for the look theme and puts it into its generated `gtk.css`
/// copy. Idempotent; takes the manager's lock.
pub fn install(ctx: &Ctx, seed: &str) -> Result<Outcome> {
    let seed = Rgba::parse(seed).map_err(Error)?.opaque();
    let _lock = Lock::acquire(ctx)?;
    let mut st = State::load(ctx);
    install_locked(ctx, &mut st, seed)
}

fn install_locked(ctx: &Ctx, st: &mut State, seed: Rgba) -> Result<Outcome> {
    if st.pending.is_some() {
        return Ok(Outcome::accent_only(tr!(
            "an interrupted apply is not finished (themes doctor)"
        )));
    }
    let Some(id) = st.source(Module::Look).map(str::to_string) else {
        return Ok(Outcome::accent_only(tr!("no theme is applied")));
    };
    let dkey = gtk_css(ctx);
    let Some(copy) = st.gtk_copies.get(&dkey).cloned() else {
        return Ok(Outcome::accent_only(tr!(
            "the theme “{theme}” has no stylesheet generated from tokens; only the accent follows the wallpaper",
            theme = id
        )));
    };
    let dest = PathBuf::from(&dkey);
    if file_hash(&dest).as_deref() != Some(copy.hash.as_str()) {
        return Ok(Outcome::accent_only(tr!(
            "{path} was changed by hand; not updated",
            path = dkey
        )));
    }
    let theme = get_theme(ctx, &id)?;
    let mut layers = vec![(
        palette::OVERLAY_LAYER.to_string(),
        palette::overlay_toml(seed),
    )];
    layers.extend(crate::tokens::layers(&theme));
    let l = theme_tokens::load_chain(&layers)
        .map_err(|e| Error(tr!("theme “{theme}”: {error}", theme = id, error = e)))?;
    let css = format!(
        "/* Live wallpaper palette (seed colour {seed}) over the theme “{id}”; removed when “Accent from wallpaper” is turned off. */\n{}",
        theme_tokens::generate::gtk::css(&l.tokens, &id),
        seed = seed.hex()
    );
    let src = dir(ctx).join(&id).join("gtk.css");
    write_atomic(&src, &css)?;
    let base_src = match &st.wallpaper_overlay {
        Some(o) if o.dest == dkey && o.theme == id => o.base_src.clone(),
        _ => copy.src.clone(),
    };
    // the record first: an interruption from here on is undone by `remove`
    st.wallpaper_overlay = Some(Overlay {
        theme: id.clone(),
        seed: seed.hex(),
        dest: dkey.clone(),
        base_src,
    });
    st.save(ctx)?;
    let hash = copy_into(&src, &dest)?;
    st.gtk_copies.insert(
        dkey,
        GtkCopy {
            src: src.to_string_lossy().into_owned(),
            hash,
        },
    );
    st.save(ctx)?;
    Ok(Outcome {
        theme: Some(id),
        path: Some(src.to_string_lossy().into_owned()),
        note: String::new(),
    })
}

/// Takes the overlay out (the theme's own stylesheet goes back) and deletes its cache. Returns
/// whether there was one. Takes the manager's lock.
pub fn remove(ctx: &Ctx, warnings: &mut Vec<String>) -> Result<bool> {
    let _lock = Lock::acquire(ctx)?;
    let mut st = State::load(ctx);
    remove_locked(ctx, &mut st, warnings)
}

pub(crate) fn remove_locked(ctx: &Ctx, st: &mut State, warnings: &mut Vec<String>) -> Result<bool> {
    let Some(o) = st.wallpaper_overlay.clone() else {
        // a cache left without a record (the record was dropped by hand): nothing to restore
        let had = dir(ctx).exists();
        let _ = std::fs::remove_dir_all(dir(ctx));
        return Ok(had);
    };
    if let Some(c) = st.gtk_copies.get(&o.dest).cloned() {
        let dest = Path::new(&o.dest);
        let base = Path::new(&o.base_src);
        if !base.is_file()
            && let Ok(t) = get_theme(ctx, &o.theme)
        {
            // the cache was cleaned: generate the theme's files again
            let _ = crate::tokens::materialize(&t, false);
        }
        let mut new = GtkCopy {
            src: o.base_src.clone(),
            hash: c.hash.clone(),
        };
        if file_hash(dest).as_deref() != Some(c.hash.as_str()) {
            warnings.push(tr!(
                "{path} was changed by hand; left as it is",
                path = o.dest
            ));
        } else if base.is_file() {
            new.hash = copy_into(base, dest)?;
        } else {
            // the record points at the theme's file again; `themes doctor --fix` regenerates
            // it and refreshes the copy
            warnings.push(tr!(
                "the files generated from tokens.toml of the theme “{theme}” are missing or out of date (themes doctor --fix)",
                theme = o.theme
            ));
        }
        st.gtk_copies.insert(o.dest.clone(), new);
    }
    st.wallpaper_overlay = None;
    st.save(ctx)?;
    let _ = std::fs::remove_dir_all(dir(ctx));
    Ok(true)
}

/// After `gtklinks::unlink_all` removed the copies: drop the record and the cache.
pub(crate) fn forget(ctx: &Ctx, st: &mut State) -> Result<()> {
    if st.wallpaper_overlay.take().is_some() {
        st.save(ctx)?;
    }
    let _ = std::fs::remove_dir_all(dir(ctx));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::{Options, apply};
    use crate::dconf::{Backend, mem::Db};
    use crate::theme::tests::{tmp_root, write};
    use std::sync::{Arc, Mutex};

    const SEED: &str = "#2a7f62";

    struct Fx {
        root: PathBuf,
        ctx: Ctx,
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// A token theme `tk`, a hand-made theme `hand` and the user's own gtk.css.
    fn fx(tag: &str) -> Fx {
        let root = tmp_root(tag);
        let mut ctx = Ctx::new(root.join("home"), root.clone());
        ctx.dconf = Backend::Memory(Arc::new(Mutex::new(Db::default())));
        write(&root.join("baseline-t/full.dconf"), "");
        write(&ctx.themes.join("tk/theme.conf"), "NAME=Tk\n");
        write(
            &ctx.themes.join("tk/tokens.toml"),
            "schema = 1\n[colors]\naccent = \"#6d4fc2\"\n[radii]\nbutton = 9\n",
        );
        write(&ctx.themes.join("hand/theme.conf"), "NAME=Hand\n");
        write(&ctx.themes.join("hand/gtk4/gtk.css"), "/* hand */\n");
        write(&ctx.gtk4.join("gtk.css"), "/* mine */\n");
        Fx { root, ctx }
    }

    fn css(f: &Fx) -> String {
        std::fs::read_to_string(f.ctx.gtk4.join("gtk.css")).unwrap_or_default()
    }

    fn applied(f: &Fx, id: &str) {
        apply(&f.ctx, id, &[], &Options::default()).unwrap();
    }

    #[test]
    fn install_then_remove_restores_the_theme_stylesheet() {
        let f = fx("overlay-cycle");
        applied(&f, "tk");
        let theirs = css(&f);
        assert!(theirs.contains("#6d4fc2"), "{theirs}");
        let o = install(&f.ctx, SEED).unwrap();
        assert_eq!(o.theme.as_deref(), Some("tk"), "{o:?}");
        let ours = css(&f);
        assert!(ours.starts_with("/* Live wallpaper palette (seed colour #2a7f62)"));
        assert!(!ours.contains("#6d4fc2"), "the accent is the wallpaper's");
        assert!(ours.contains("9px"), "radii stay the theme's");
        // idempotent, and a new seed keeps the theme's file as the one to return to
        assert_eq!(install(&f.ctx, SEED).unwrap(), o);
        install(&f.ctx, "#c2410c").unwrap();
        let st = State::load(&f.ctx);
        let rec = st.wallpaper_overlay.clone().unwrap();
        assert_eq!(rec.seed, "#c2410c");
        assert!(rec.base_src.ends_with(".cache/tk/gtk4/gtk.css"), "{rec:?}");
        assert!(dir(&f.ctx).join("tk/gtk.css").is_file());
        let mut w = vec![];
        assert!(remove(&f.ctx, &mut w).unwrap());
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(css(&f), theirs, "exactly the theme's stylesheet again");
        let st = State::load(&f.ctx);
        assert!(st.wallpaper_overlay.is_none());
        assert_eq!(st.gtk_copies[&gtk_css(&f.ctx)].src, rec.base_src);
        assert!(!dir(&f.ctx).exists(), "the cache is deleted (AC-WALL-08)");
        assert!(!remove(&f.ctx, &mut w).unwrap(), "nothing left to remove");
    }

    #[test]
    fn theme_switch_and_reset_drop_the_overlay_exactly() {
        let f = fx("overlay-switch");
        applied(&f, "tk");
        install(&f.ctx, SEED).unwrap();
        // switching the look theme: the user's own file comes back, the overlay is forgotten
        applied(&f, "hand");
        assert_eq!(
            std::fs::read_link(f.ctx.gtk4.join("gtk.css")).unwrap(),
            f.ctx.themes.join("hand/gtk4/gtk.css")
        );
        let st = State::load(&f.ctx);
        assert!(st.wallpaper_overlay.is_none() && st.gtk_copies.is_empty());
        assert!(!dir(&f.ctx).exists());
        // a hand-made stylesheet is never replaced (AC-WALL-07)
        let o = install(&f.ctx, SEED).unwrap();
        assert_eq!(o.theme, None);
        assert!(o.note.contains("hand"), "{o:?}");
        assert!(!dir(&f.ctx).exists());
        // the look-only reset path (unlink_all) from an overlay gives the user's file back
        applied(&f, "tk");
        install(&f.ctx, SEED).unwrap();
        let mut st = State::load(&f.ctx);
        let mut w = vec![];
        crate::gtklinks::unlink_all(&f.ctx, &mut st, &mut w).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(css(&f), "/* mine */\n");
        let st = State::load(&f.ctx);
        assert!(st.wallpaper_overlay.is_none() && st.backups.is_empty());
        assert!(!dir(&f.ctx).exists());
    }

    #[test]
    fn stale_and_edited_states() {
        let f = fx("overlay-stale");
        applied(&f, "tk");
        let theirs = css(&f);
        install(&f.ctx, SEED).unwrap();
        // the cache was cleaned while the overlay was on: remove regenerates the theme's file
        std::fs::remove_dir_all(f.root.join(".cache")).unwrap();
        let mut w = vec![];
        assert!(remove(&f.ctx, &mut w).unwrap());
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(css(&f), theirs);
        // interrupted install: record saved, copy not replaced yet
        install(&f.ctx, SEED).unwrap();
        let mut st = State::load(&f.ctx);
        let dkey = gtk_css(&f.ctx);
        let base = st.wallpaper_overlay.clone().unwrap().base_src;
        std::fs::copy(&base, &dkey).unwrap();
        st.gtk_copies.get_mut(&dkey).unwrap().hash = file_hash(Path::new(&dkey)).unwrap();
        st.save(&f.ctx).unwrap();
        assert!(remove(&f.ctx, &mut w).unwrap());
        assert_eq!(css(&f), theirs);
        // a copy the user edited is neither overwritten by install nor by remove
        install(&f.ctx, SEED).unwrap();
        std::fs::write(&dkey, "/* edited */\n").unwrap();
        let o = install(&f.ctx, "#c2410c").unwrap();
        assert!(o.theme.is_none() && o.note.contains("by hand"), "{o:?}");
        assert!(remove(&f.ctx, &mut w).unwrap());
        assert_eq!(css(&f), "/* edited */\n");
        assert!(w.iter().any(|m| m.contains("by hand")), "{w:?}");
        // a cache without a record is deleted
        std::fs::create_dir_all(dir(&f.ctx).join("x")).unwrap();
        assert!(remove(&f.ctx, &mut w).unwrap());
        assert!(!dir(&f.ctx).exists());
    }
}
