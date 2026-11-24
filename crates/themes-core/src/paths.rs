// SPDX-License-Identifier: GPL-3.0-or-later
use desk_settings::tr;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Усі шляхи, з якими працює менеджер. Корінь можна перевизначити через DESKTOP_THEMES_ROOT.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub home: PathBuf,
    pub root: PathBuf,
    /// `$XDG_CONFIG_HOME` in a live process, else `<home>/.config` (always isolated in tests).
    pub config_home: PathBuf,
    pub themes: PathBuf,
    pub state: PathBuf,
    pub current: PathBuf,
    pub log: PathBuf,
    pub lock: PathBuf,
    pub backups: PathBuf,
    pub trash: PathBuf,
    pub gtk4: PathBuf,
    pub gtk3: PathBuf,
    pub flatpak_overrides: PathBuf,
    /// dconf of `apply` (the `dconf` utility; an in-memory database in unit tests).
    pub dconf: crate::dconf::Backend,
}

impl Ctx {
    pub fn from_env() -> Ctx {
        let home = std::env::var_os("HOME").map_or_else(|| "/".into(), PathBuf::from);
        let root = std::env::var_os("DESKTOP_THEMES_ROOT")
            .map_or_else(|| home.join(".local/share/desktop-themes"), PathBuf::from);
        let mut ctx = Ctx::new(home, root);
        if let Some(p) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
            && p.is_absolute()
        {
            ctx.config_home = p;
        }
        ctx
    }

    pub fn new(home: PathBuf, root: PathBuf) -> Ctx {
        let config_home = home.join(".config");
        Ctx {
            themes: root.join("themes"),
            state: root.join("state.json"),
            current: root.join("current"),
            log: root.join("apply.log"),
            lock: root.join(".lock"),
            backups: root.join("backups"),
            trash: root.join("trash"),
            gtk4: home.join(".config/gtk-4.0"),
            gtk3: home.join(".config/gtk-3.0"),
            flatpak_overrides: home.join(".local/share/flatpak/overrides"),
            dconf: Default::default(),
            config_home,
            home,
            root,
        }
    }

    /// Generated desk-shell stylesheet (`$XDG_CONFIG_HOME/desk-shell/desk-shell.css`; in tests
    /// `<home>/.config/desk-shell/desk-shell.css`).
    pub fn desk_shell_css(&self) -> PathBuf {
        self.config_home
            .join(theme_tokens::generate::DESK_SHELL_CSS)
    }

    pub fn baseline_dir(&self) -> Result<PathBuf> {
        let link = self.root.join("baseline");
        if link.is_dir() {
            return Ok(std::fs::canonicalize(&link)?);
        }
        let mut c: Vec<PathBuf> = std::fs::read_dir(&self.root)?
            .flatten()
            .filter(|e| {
                e.file_name().to_string_lossy().starts_with("baseline-") && e.path().is_dir()
            })
            .map(|e| e.path())
            .collect();
        c.sort();
        match c.into_iter().next() {
            Some(p) => Ok(p),
            None => Err(crate::Error(tr!(
                "no baseline-* found in {path}",
                path = self.root.display()
            ))),
        }
    }

    pub fn user_extensions_dir(&self) -> PathBuf {
        self.home.join(".local/share/gnome-shell/extensions")
    }

    pub fn extension_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.user_extensions_dir(),
            PathBuf::from("/usr/share/gnome-shell/extensions"),
        ]
    }

    /// Каталог встановленого розширення (користувацьке має пріоритет).
    pub fn extension_path(&self, uuid: &str) -> Option<PathBuf> {
        self.extension_dirs()
            .into_iter()
            .map(|d| d.join(uuid))
            .find(|p| p.join("metadata.json").is_file())
    }

    /// GNOME Shell of the session, when it runs (never for the in-memory test dconf).
    pub fn shell(&self) -> Option<crate::shell::Shell> {
        if self.dconf.live() {
            crate::shell::Shell::connect()
        } else {
            None
        }
    }

    pub fn exe_path() -> PathBuf {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("themes"))
    }

    pub fn rel<'a>(&self, p: &'a Path) -> &'a Path {
        p.strip_prefix(&self.home).unwrap_or(p)
    }
}
