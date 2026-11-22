// SPDX-License-Identifier: GPL-3.0-or-later
//! Design tokens in the apply pipeline (the token model itself lives in the `theme-tokens`
//! crate, see docs/design-tokens.md).
//!
//! * `tokens.toml` is read along the `INHERITS` chain and deep-merged.
//! * Derived dconf keys are merged *under* the theme's *.dconf files ([`Theme::settings`]):
//!   a key written by hand wins.
//! * Generated files live in the cache (`~/.cache/desktop-themes/<theme>/`, content-hashed)
//!   and reach their consumers as follows: `gtk4/` is COPIED into ~/.config/gtk-4.0 by
//!   `gtklinks` (only when no theme in the chain ships its own `gtk4/`; Flatpak apps with
//!   `xdg-config/gtk-4.0:ro` cannot follow links into ~/.cache), the shell theme is linked as
//!   `~/.local/share/themes/desktop-themes-<id>` (look module) and the Ptyxis palette as
//!   `~/.local/share/org.gnome.Ptyxis/palettes/desktop-themes-<id>.palette` (apps module).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use theme_tokens::generate::{self, Outputs, Target};
use theme_tokens::{KeyModule, Keys, Loaded, TokenError};

use desk_settings::tr;

use crate::error::{Error, Result};
use crate::modules::Module;
use crate::paths::Ctx;
use crate::state::State;
use crate::theme::Theme;
use crate::util::lexists;

pub use theme_tokens::{self, TOKENS_FILE};

/// Root of the generated-files cache. `DESKTOP_THEMES_CACHE` overrides it; with a custom
/// `DESKTOP_THEMES_ROOT` (tests, sandboxes) the cache stays inside that root.
pub fn cache_root(ctx: &Ctx) -> PathBuf {
    if let Some(p) = std::env::var_os("DESKTOP_THEMES_CACHE") {
        return PathBuf::from(p);
    }
    if ctx.root != ctx.home.join(".local/share/desktop-themes") {
        return ctx.root.join(".cache");
    }
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| ctx.home.join(".cache"))
        .join("desktop-themes")
}

/// `$XDG_DATA_HOME` (only for the default root; tests use `<home>/.local/share`).
pub fn data_home(ctx: &Ctx) -> PathBuf {
    if ctx.root == ctx.home.join(".local/share/desktop-themes")
        && let Some(p) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from)
        && p.is_absolute()
    {
        return p;
    }
    ctx.home.join(".local/share")
}

/// Token files of the chain, child first.
pub fn layers(theme: &Theme) -> Vec<theme_tokens::Layer> {
    theme
        .layers()
        .into_iter()
        .filter_map(|d| {
            let p = d.join(TOKENS_FILE);
            let text = std::fs::read_to_string(&p).ok()?;
            let id = d
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Some((format!("{id}/{TOKENS_FILE}"), text))
        })
        .collect()
}

pub fn has_tokens(theme: &Theme) -> bool {
    theme.layers().iter().any(|d| d.join(TOKENS_FILE).is_file())
}

/// `None`: the theme has no tokens.
pub fn load(theme: &Theme) -> Option<std::result::Result<Loaded, TokenError>> {
    let l = layers(theme);
    if l.is_empty() {
        return None;
    }
    Some(theme_tokens::load_chain(&l))
}

/// Generation target for a theme; `handmade_shell` = its dconf files set the user-theme name.
pub fn target(theme: &Theme, handmade_shell: bool) -> Target {
    Target {
        theme_id: theme.id.clone(),
        theme_name: theme.name(),
        home: theme.home.clone(),
        cache_dir: theme.cache_dir.clone(),
        handmade_gtk4: theme.handmade_gtk_dir("gtk4").is_some(),
        handmade_shell_theme: handmade_shell,
    }
}

pub fn module_of(k: KeyModule) -> Module {
    match k {
        KeyModule::Look => Module::Look,
        KeyModule::Sounds => Module::Sounds,
        KeyModule::Apps => Module::Apps,
    }
}

/// Derived keys of one module (empty without tokens or with invalid tokens).
pub fn keys_for(theme: &Theme, module: Module, handmade_shell: bool) -> Keys {
    let Some(Ok(l)) = load(theme) else {
        return Keys::new();
    };
    theme_tokens::keys(&l.tokens, &target(theme, handmade_shell))
        .into_iter()
        .find(|(k, _)| module_of(*k) == module)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

/// Which artefacts the theme's tokens produce (all false without valid tokens).
pub fn outputs(theme: &Theme) -> Outputs {
    let Some(Ok(l)) = load(theme) else {
        return Outputs::default();
    };
    Outputs::for_target(&l.tokens, &target(theme, theme.handmade_shell_theme()))
}

/// One generated file, for `--dry-run` and the apply report.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GeneratedFile {
    pub theme: String,
    pub path: String,
    /// "cached" (up to date), "written" (generated now) or "planned" (dry run, would be written)
    pub status: &'static str,
}

/// Validates the theme's tokens (error message lists every problem).
pub fn check(theme: &Theme) -> Result<Option<Loaded>> {
    match load(theme) {
        None => Ok(None),
        Some(Ok(l)) => Ok(Some(l)),
        Some(Err(e)) => Err(Error(tr!("theme “{theme}”: {error}", theme = theme.id, error = e))),
    }
}

/// Generated files of a theme without writing anything.
pub fn plan(theme: &Theme) -> Result<Vec<GeneratedFile>> {
    let Some(l) = check(theme)? else {
        return Ok(vec![]);
    };
    let tg = target(theme, theme.handmade_shell_theme());
    let current = theme_tokens::is_current(&l, &tg);
    Ok(theme_tokens::planned(&l, &tg)
        .into_iter()
        .map(|rel| GeneratedFile {
            theme: theme.id.clone(),
            path: tg.cache_dir.join(rel).to_string_lossy().into_owned(),
            status: if current { "cached" } else { "planned" },
        })
        .collect())
}

/// Ensures the theme's generated files are in the cache. Returns them and the time spent.
pub fn materialize(
    theme: &Theme,
    force: bool,
) -> Result<(Vec<GeneratedFile>, std::time::Duration)> {
    let Some(l) = check(theme)? else {
        return Ok((vec![], std::time::Duration::ZERO));
    };
    let tg = target(theme, theme.handmade_shell_theme());
    let m = theme_tokens::materialize(&l, &tg, force)?;
    let status = if m.cached { "cached" } else { "written" };
    let files = m
        .files
        .iter()
        .map(|p| GeneratedFile {
            theme: theme.id.clone(),
            path: p.to_string_lossy().into_owned(),
            status,
        })
        .collect();
    Ok((files, m.elapsed))
}

/// (link path, target) pairs a module needs for the theme's generated files.
pub fn links_for(ctx: &Ctx, theme: &Theme, module: Module) -> Vec<(PathBuf, PathBuf)> {
    let o = outputs(theme);
    let dh = data_home(ctx);
    let mut v = Vec::new();
    match module {
        Module::Look if o.shell => v.push((
            dh.join("themes")
                .join(generate::shell_theme_name(&theme.id)),
            theme.cache_dir.join(generate::SHELL_DIR),
        )),
        Module::Apps if o.ptyxis => v.push((
            dh.join("org.gnome.Ptyxis/palettes").join(format!(
                "{}.palette",
                generate::ptyxis_palette_id(&theme.id)
            )),
            theme.cache_dir.join(generate::ptyxis_rel(&theme.id)),
        )),
        _ => {}
    }
    v
}

/// Removes the token links a module created last time (only links that still point where we
/// made them point; anything else is left alone with a warning).
pub fn unlink_module(st: &mut State, module: Module, warnings: &mut Vec<String>) {
    let Some(links) = st.token_links.remove(&module) else {
        return;
    };
    for (dest, src) in links {
        let d = Path::new(&dest);
        match std::fs::read_link(d) {
            Ok(t) if t == Path::new(&src) => {
                let _ = std::fs::remove_file(d);
            }
            _ if lexists(d) => warnings.push(tr!("{path} was changed by hand; left as it is", path = dest)),
            _ => {}
        }
    }
}

/// Links the module's generated files of `theme` (after [`unlink_module`]).
pub fn link_module(
    ctx: &Ctx,
    st: &mut State,
    module: Module,
    theme: &Theme,
    warnings: &mut Vec<String>,
) -> Result<Vec<String>> {
    let mut made = Vec::new();
    let mut rec: BTreeMap<String, String> = BTreeMap::new();
    for (dest, src) in links_for(ctx, theme, module) {
        if lexists(&dest) {
            let ours = std::fs::read_link(&dest).is_ok_and(|t| t == src);
            if !ours {
                warnings.push(tr!(
                    "{path} already exists and is not a link of the manager; the generated file is not linked",
                    path = dest.display()
                ));
                continue;
            }
            std::fs::remove_file(&dest)?;
        }
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::os::unix::fs::symlink(&src, &dest)?;
        rec.insert(dest.to_string_lossy().into(), src.to_string_lossy().into());
        made.push(dest.to_string_lossy().into_owned());
    }
    if !rec.is_empty() {
        st.token_links.insert(module, rec);
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::get_theme;
    use crate::theme::tests::{tmp_root, write};

    const TOKENS: &str = r##"
schema = 1
[colors.light]
accent = "#6d4fc2"
window_bg = "#f7f5fc"
window_fg = "#221c35"
[radii]
button = 8
[fonts]
ui = "Inter 11"
[sounds]
theme = "ocean"
[shell]
[terminal]
font = "Mono 12"
[terminal.dark]
fg = "#e6e1f2"
bg = "#17141f"
palette = ["#2b2638", "#f07a8f", "#7fd4a0", "#ecc27a", "#8aa8f5", "#c99af2", "#7cc9d6", "#cfc8de",
           "#5d5672", "#f79aaa", "#9fe3b8", "#f3d59c", "#a9c0f8", "#dbb7f7", "#9fdbe5", "#f6f3fb"]
"##;

    #[test]
    fn tokens_feed_settings_gtk_and_extensions() {
        let root = tmp_root("tokens");
        let ctx = Ctx::new(root.join("home"), root.clone());
        let t = ctx.themes.join("tk");
        write(&t.join("theme.conf"), "NAME=Tk\n");
        write(&t.join(TOKENS_FILE), TOKENS);
        // hand-made dconf wins over a derived key
        write(
            &t.join("settings.dconf"),
            "[org/gnome/desktop/interface]\nfont-name='Hand 10'\n",
        );
        let th = get_theme(&ctx, "tk").unwrap();
        assert_eq!(th.cache_dir, root.join(".cache/tk"));
        // tokens have no behavior part
        assert_eq!(
            th.modules(),
            vec![Module::Look, Module::Sounds, Module::Apps]
        );
        let look = th.settings(Module::Look);
        assert_eq!(look["/org/gnome/desktop/interface/font-name"], "'Hand 10'");
        assert_eq!(
            look["/org/gnome/desktop/interface/accent-color"],
            "'purple'"
        );
        assert_eq!(
            look["/org/gnome/shell/extensions/user-theme/name"],
            "'desktop-themes-tk'"
        );
        assert_eq!(
            th.settings(Module::Sounds)["/org/gnome/desktop/sound/theme-name"],
            "'ocean'"
        );
        // the Ptyxis profile placeholder is skipped (no Ptyxis in the test environment)
        assert_eq!(
            th.settings(Module::Apps)["/org/gnome/Ptyxis/font-name"],
            "'Mono 12'"
        );
        assert!(
            th.extensions()
                .contains(&theme_tokens::USER_THEME_EXTENSION.to_string())
        );
        assert_eq!(th.gtk_dir("gtk4").unwrap(), root.join(".cache/tk/gtk4"));
        assert_eq!(th.gtk_entries("gtk4"), vec!["gtk.css"]);

        // dry plan, then generation, then cache hit
        let p = plan(&th).unwrap();
        assert!(p.iter().all(|f| f.status == "planned"), "{p:?}");
        let (files, _) = materialize(&th, false).unwrap();
        assert!(
            files
                .iter()
                .all(|f| f.status == "written" && Path::new(&f.path).is_file())
        );
        let (files, _) = materialize(&th, false).unwrap();
        assert!(files.iter().all(|f| f.status == "cached"));
        assert!(plan(&th).unwrap().iter().all(|f| f.status == "cached"));

        // links: look → shell theme dir, apps → palette; unlink removes only ours
        let mut st = State::default();
        let mut w = Vec::new();
        let made = link_module(&ctx, &mut st, Module::Look, &th, &mut w).unwrap();
        let shell_link = ctx.home.join(".local/share/themes/desktop-themes-tk");
        assert_eq!(made, vec![shell_link.to_string_lossy().into_owned()]);
        assert!(shell_link.join("gnome-shell/gnome-shell.css").is_file());
        link_module(&ctx, &mut st, Module::Apps, &th, &mut w).unwrap();
        let pal = ctx
            .home
            .join(".local/share/org.gnome.Ptyxis/palettes/desktop-themes-tk.palette");
        assert!(pal.is_file());
        unlink_module(&mut st, Module::Look, &mut w);
        assert!(!lexists(&shell_link));
        assert!(pal.is_file(), "apps links survive a look switch");
        assert!(w.is_empty(), "{w:?}");

        // generated GTK files are COPIED into ~/.config/gtk-4.0 (Flatpak), user files backed up
        write(&ctx.gtk4.join("gtk.css"), "/* mine */");
        let made = crate::gtklinks::link_theme(&ctx, &mut st, &th).unwrap();
        let css = ctx.gtk4.join("gtk.css");
        assert_eq!(made, vec![css.to_string_lossy().into_owned()]);
        assert!(!css.is_symlink() && css.is_file());
        assert_eq!(
            std::fs::read(&css).unwrap(),
            std::fs::read(root.join(".cache/tk/gtk4/gtk.css")).unwrap()
        );
        assert!(st.links.is_empty() && st.gtk_copies.len() == 1);
        // a new cache generation refreshes the copy
        std::fs::write(root.join(".cache/tk/gtk4/gtk.css"), "/* v2 */").unwrap();
        assert_eq!(crate::gtklinks::refresh_copies(&mut st, &mut w).len(), 1);
        assert_eq!(std::fs::read_to_string(&css).unwrap(), "/* v2 */");
        crate::gtklinks::unlink_all(&mut st, &mut w).unwrap();
        assert_eq!(
            std::fs::read_to_string(&css).unwrap(),
            "/* mine */",
            "user file restored"
        );
        assert!(st.gtk_copies.is_empty());
        // a copy the user edited is left alone
        materialize(&th, true).unwrap();
        std::fs::remove_file(&css).unwrap();
        crate::gtklinks::link_theme(&ctx, &mut st, &th).unwrap();
        std::fs::write(&css, "/* edited */").unwrap();
        crate::gtklinks::unlink_all(&mut st, &mut w).unwrap();
        assert_eq!(std::fs::read_to_string(&css).unwrap(), "/* edited */");
        assert!(w.iter().any(|m| m.contains("by hand")), "{w:?}");

        // a hand-made gtk4/ and user-theme name switch generation off
        write(&t.join("gtk4/gtk.css"), "/* hand */");
        write(
            &t.join("settings.dconf"),
            "[org/gnome/shell/extensions/user-theme]\nname='Mine'\n",
        );
        let th = get_theme(&ctx, "tk").unwrap();
        assert_eq!(th.gtk_dir("gtk4").unwrap(), t.join("gtk4"));
        assert_eq!(
            th.settings(Module::Look)["/org/gnome/shell/extensions/user-theme/name"],
            "'Mine'"
        );
        let o = outputs(&th);
        assert!(!o.gtk && !o.shell && o.ptyxis);

        // invalid tokens: apply refuses with every problem listed, settings ignore them
        write(
            &t.join(TOKENS_FILE),
            "[radii]\nbutton = 999\n[fonts]\nui = \"X\"\n",
        );
        let th = get_theme(&ctx, "tk").unwrap();
        let e = check(&th).unwrap_err().0;
        assert!(e.contains("radii.button") && e.contains("fonts.ui"), "{e}");
        assert!(
            !th.settings(Module::Look)
                .contains_key("/org/gnome/desktop/interface/accent-color")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn child_overrides_parent_tokens() {
        let root = tmp_root("tokens-inherit");
        let ctx = Ctx::new(root.join("home"), root.clone());
        write(
            &ctx.themes.join("p/tokens.toml"),
            "[colors]\naccent = \"teal\"\n[fonts]\nui = \"A 11\"\n",
        );
        write(&ctx.themes.join("c/theme.conf"), "INHERITS=p\n");
        write(
            &ctx.themes.join("c/tokens.toml"),
            "[fonts]\nui = \"B 12\"\n",
        );
        let c = get_theme(&ctx, "c").unwrap();
        let look = c.settings(Module::Look);
        assert_eq!(look["/org/gnome/desktop/interface/accent-color"], "'teal'");
        assert_eq!(look["/org/gnome/desktop/interface/font-name"], "'B 12'");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_themes_load() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
        let ctx = Ctx::new(
            PathBuf::from("/nonexistent-home"),
            repo.parent().unwrap().to_path_buf(),
        );
        for t in crate::theme::all_themes(&ctx) {
            if let Some(r) = load(&t) {
                let l = r.unwrap_or_else(|e| panic!("{}: {e}", t.id));
                assert!(l.warnings.is_empty(), "{}: {:?}", t.id, l.warnings);
            }
        }
        // macos-glass inherits the macos tokens
        let g = Theme::new(&ctx, "macos-glass");
        assert_eq!(
            g.settings(Module::Sounds)["/org/gnome/desktop/sound/theme-name"],
            "'macos-like'"
        );
        // windows11 keeps the builtin Ptyxis palette and its hand-made cursor shape
        let w = Theme::new(&ctx, "windows11");
        let apps = w.settings(Module::Apps);
        assert_eq!(apps["/org/gnome/Ptyxis/cursor-shape"], "'ibeam'");
        assert_eq!(apps["/org/gnome/Ptyxis/font-name"], "'Cascadia Mono 12'");
        // lavender: only theme.conf + tokens.toml, yet a full look
        let l = Theme::new(&ctx, "lavender");
        assert!(l.modules().contains(&Module::Look));
        assert!(l.modules().contains(&Module::Apps));
        assert!(
            outputs(&l).gtk && outputs(&l).shell && outputs(&l).ptyxis && outputs(&l).wallpaper
        );
    }
}
