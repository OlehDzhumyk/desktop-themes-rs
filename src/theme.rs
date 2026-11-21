// SPDX-License-Identifier: GPL-3.0-or-later
//! Тека теми та її поділ на модулі.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::dconf::{self, Settings};
use crate::error::Result;
use crate::modules::Module;
use crate::paths::Ctx;
use crate::util::{self, is_executable, shlex_split};
use crate::{DIS_KEY, EXT_KEY};

pub const GTK_KINDS: [&str; 2] = ["gtk4", "gtk3"];
/// Файли GTK, яких теми ніколи не чіпають.
pub fn protected_gtk(kind: &str, name: &str) -> bool {
    kind == "gtk3" && name == "bookmarks"
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub id: String,
    pub home: String,
    pub dir: PathBuf,
    pub conf: BTreeMap<String, String>,
}

/// Місце профілю Ptyxis у *.dconf: `[org/gnome/Ptyxis/Profiles/@DEFAULT_PTYXIS_PROFILE@]`.
/// Підставляється `default-profile-uuid` із dconf; якщо його немає, ці ключі пропускаються.
pub const PTYXIS_DEFAULT_PROFILE: &str = "@DEFAULT_PTYXIS_PROFILE@";

pub fn resolve_ptyxis_profile(d: Settings, uuid: Option<&str>) -> Settings {
    d.into_iter()
        .filter_map(|(k, v)| {
            if !k.contains(PTYXIS_DEFAULT_PROFILE) {
                return Some((k, v));
            }
            uuid.map(|u| (k.replace(PTYXIS_DEFAULT_PROFILE, u), v))
        })
        .collect()
}

pub fn read_conf(path: &Path) -> BTreeMap<String, String> {
    let mut conf = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return conf;
    };
    for line in text.lines() {
        let l = line.trim_start();
        let l = l.strip_prefix("export ").map(str::trim_start).unwrap_or(l);
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        let valid_key = !k.is_empty()
            && k.chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
            && k.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !valid_key {
            continue;
        }
        let val = match shlex_split(v) {
            Ok(parts) => parts.into_iter().next().unwrap_or_default(),
            Err(_) => v.trim().trim_matches('"').to_string(),
        };
        conf.insert(k.to_string(), val);
    }
    conf
}

pub fn write_conf(path: &Path, conf: &[(&str, String)]) -> Result<()> {
    let mut s = String::new();
    for (k, v) in conf {
        s.push_str(&format!("{k}={}\n", util::conf_quote(v)));
    }
    std::fs::write(path, s)?;
    Ok(())
}

impl Theme {
    pub fn new(ctx: &Ctx, id: &str) -> Theme {
        let dir = ctx.themes.join(id);
        let conf = read_conf(&dir.join("theme.conf"));
        Theme {
            id: id.to_string(),
            home: ctx.home.to_string_lossy().into_owned(),
            dir,
            conf,
        }
    }

    pub fn name(&self) -> String {
        self.conf
            .get("NAME")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| self.id.clone())
    }
    pub fn description(&self) -> String {
        self.conf.get("DESCRIPTION").cloned().unwrap_or_default()
    }
    pub fn preview(&self) -> String {
        self.preview_key("PREVIEW")
    }
    pub fn preview_dark(&self) -> String {
        self.preview_key("PREVIEW_DARK")
    }
    /// Шлях зображення з theme.conf; `~`, `$HOME` і `${HOME}` розкриваються.
    fn preview_key(&self, key: &str) -> String {
        self.conf
            .get(key)
            .map(|v| util::expand_home(v, &self.home))
            .unwrap_or_default()
    }
    pub fn builtin(&self) -> bool {
        self.id == crate::BUILTIN_THEME
    }

    /// Усі *.dconf-файли теми, згруповані за модулями.
    pub fn dconf_files(&self, module: Module) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return vec![];
        };
        let mut v: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.ends_with(".dconf") && e.path().is_file() && Module::for_dconf_file(&n) == module
            })
            .map(|e| e.path())
            .collect();
        v.sort();
        v
    }

    /// Ключі dconf модуля (список розширень керується тільки extensions.txt).
    pub fn settings(&self, module: Module) -> Settings {
        let mut d = Settings::new();
        for f in self.dconf_files(module) {
            if let Ok(s) = dconf::parse_file(&f) {
                d.extend(s);
            }
        }
        for v in d.values_mut() {
            *v = util::expand_home_value(v, &self.home);
        }
        if d.keys().any(|k| k.contains(PTYXIS_DEFAULT_PROFILE)) {
            let uuid = dconf::read("/org/gnome/Ptyxis/default-profile-uuid")
                .and_then(|v| dconf::unquote_str(&v));
            d = resolve_ptyxis_profile(d, uuid.as_deref());
        }
        d.remove(EXT_KEY);
        d.remove(DIS_KEY);
        d
    }

    pub fn extensions(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Ok(text) = std::fs::read_to_string(self.dir.join("extensions.txt")) {
            for line in text.lines() {
                let l = line.split('#').next().unwrap_or("").trim();
                if !l.is_empty() && !out.iter().any(|x| x == l) {
                    out.push(l.to_string());
                }
            }
        }
        out
    }

    pub fn gtk_entries(&self, kind: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(self.dir.join(kind)) else {
            return vec![];
        };
        let mut v: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !protected_gtk(kind, n))
            .collect();
        v.sort();
        v
    }

    pub fn flatpak_lines(&self) -> Vec<Vec<String>> {
        let Ok(text) = std::fs::read_to_string(self.dir.join("flatpak.conf")) else {
            return vec![];
        };
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| shlex_split(l).ok())
            .filter(|v| !v.is_empty())
            .collect()
    }

    /// apps/hooks/<name> (apply | revert), якщо файл виконуваний.
    pub fn hook(&self, name: &str) -> Option<PathBuf> {
        let p = self.dir.join("apps/hooks").join(name);
        is_executable(&p).then_some(p)
    }

    /// Чи містить тема хоч щось для модуля.
    pub fn has_module(&self, m: Module) -> bool {
        match m {
            Module::Look => {
                !self.dconf_files(Module::Look).is_empty()
                    || self.dir.join("extensions.txt").is_file()
                    || self.dir.join("flatpak.conf").is_file()
                    || GTK_KINDS.iter().any(|k| !self.gtk_entries(k).is_empty())
            }
            Module::Apps => {
                !self.dconf_files(m).is_empty()
                    || self.hook("apply").is_some()
                    || self.hook("revert").is_some()
            }
            _ => !self.dconf_files(m).is_empty(),
        }
    }

    pub fn modules(&self) -> Vec<Module> {
        Module::ALL
            .into_iter()
            .filter(|m| self.has_module(*m))
            .collect()
    }
}

pub fn all_themes(ctx: &Ctx) -> Vec<Theme> {
    let Ok(rd) = std::fs::read_dir(&ctx.themes) else {
        return vec![];
    };
    let mut ids: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    ids.sort_by_key(|x| (x != crate::BUILTIN_THEME, x.clone()));
    ids.iter().map(|i| Theme::new(ctx, i)).collect()
}

pub fn get_theme(ctx: &Ctx, id: &str) -> Result<Theme> {
    if !util::valid_id(id) {
        crate::bail!("некоректний id теми: {id:?}");
    }
    let t = Theme::new(ctx, id);
    if !t.dir.is_dir() {
        crate::bail!("тему «{id}» не знайдено у {}", ctx.themes.display());
    }
    Ok(t)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn tmp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("themes-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn write(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn ptyxis_placeholder() {
        let mut d = Settings::new();
        d.insert(
            format!("/org/gnome/Ptyxis/Profiles/{PTYXIS_DEFAULT_PROFILE}/palette"),
            "'Pro'".into(),
        );
        d.insert("/org/gnome/Ptyxis/font-name".into(), "'Mono'".into());
        let r = resolve_ptyxis_profile(d.clone(), Some("abc"));
        assert_eq!(r["/org/gnome/Ptyxis/Profiles/abc/palette"], "'Pro'");
        assert_eq!(r.len(), 2);
        let r = resolve_ptyxis_profile(d, None);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn module_split() {
        let root = tmp_root("split");
        let ctx = Ctx::new(root.join("home"), root.clone());
        let t = ctx.themes.join("t1");
        write(
            &t.join("theme.conf"),
            "NAME=\"Тест \\\"1\\\"\"\nDESCRIPTION='опис' # коментар\nexport PREVIEW=/x/y.png\nbad line\n",
        );
        write(
            &t.join("settings.dconf"),
            "[org/gnome/desktop/interface]\ngtk-theme='A'\n[org/gnome/shell]\nenabled-extensions=['x']\n",
        );
        write(
            &t.join("extra.dconf"),
            "[org/gnome/desktop/wm/preferences]\nbutton-layout='close:'\n",
        );
        write(
            &t.join("behavior.dconf"),
            "[org/gnome/mutter]\nedge-tiling=true\n",
        );
        write(
            &t.join("sounds.dconf"),
            "# звуки\n[org/gnome/desktop/sound]\ntheme-name='ocean'\n",
        );
        write(
            &t.join("apps.dconf"),
            "[org/gnome/Ptyxis]\nfont-name='Mono 12'\n",
        );
        write(&t.join("extensions.txt"), "a@b # comment\n\nc@d\na@b\n");
        write(
            &t.join("flatpak.conf"),
            "# c\n--filesystem=xdg-config/gtk-4.0:ro\n",
        );
        write(&t.join("gtk3/bookmarks"), "x");
        write(&t.join("gtk3/gtk.css"), "x");
        write(&t.join("README.md"), "ignored");
        let th = get_theme(&ctx, "t1").unwrap();
        assert_eq!(th.name(), "Тест \"1\"");
        assert_eq!(th.description(), "опис");
        assert_eq!(th.preview(), "/x/y.png");
        write(
            &ctx.themes.join("t3/theme.conf"),
            "PREVIEW=\"${HOME}/bg/a.png\"\nPREVIEW_DARK=~/bg/b.png\n",
        );
        write(
            &ctx.themes.join("t3/settings.dconf"),
            "[org/gnome/desktop/background]\npicture-uri='file://${HOME}/bg/a.png'\n",
        );
        let t3 = get_theme(&ctx, "t3").unwrap();
        let h = ctx.home.to_string_lossy().into_owned();
        assert_eq!(t3.preview(), format!("{h}/bg/a.png"));
        assert_eq!(t3.preview_dark(), format!("{h}/bg/b.png"));
        assert_eq!(
            t3.settings(Module::Look)["/org/gnome/desktop/background/picture-uri"],
            format!("'file://{h}/bg/a.png'")
        );
        let look = th.settings(Module::Look);
        assert_eq!(look.len(), 2, "{look:?}");
        assert!(look.contains_key("/org/gnome/desktop/interface/gtk-theme"));
        assert!(!look.contains_key(EXT_KEY));
        assert_eq!(
            th.settings(Module::Behavior).keys().collect::<Vec<_>>(),
            vec!["/org/gnome/mutter/edge-tiling"]
        );
        assert_eq!(th.settings(Module::Sounds).len(), 1);
        assert_eq!(th.settings(Module::Apps).len(), 1);
        assert_eq!(th.extensions(), vec!["a@b", "c@d"]);
        assert_eq!(th.gtk_entries("gtk3"), vec!["gtk.css"]);
        assert_eq!(
            th.flatpak_lines(),
            vec![vec!["--filesystem=xdg-config/gtk-4.0:ro".to_string()]]
        );
        assert_eq!(th.modules(), Module::ALL.to_vec());

        let t2 = ctx.themes.join("t2");
        write(
            &t2.join("behavior.dconf"),
            "[org/gnome/mutter]\nedge-tiling=false\n",
        );
        let th2 = get_theme(&ctx, "t2").unwrap();
        assert_eq!(th2.modules(), vec![Module::Behavior]);
        assert!(get_theme(&ctx, "../etc").is_err());
        assert!(get_theme(&ctx, "nope").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
