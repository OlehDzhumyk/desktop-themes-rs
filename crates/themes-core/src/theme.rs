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
    /// theme.conf; для теми з `INHERITS` — злите з батьківським (крім NAME, DESCRIPTION, INHERITS).
    pub conf: BTreeMap<String, String>,
    /// Батьківська тема (`INHERITS=<id>` у theme.conf): файли, яких немає в дочірній, беруться з неї.
    pub parent: Option<Box<Theme>>,
    /// Чому `INHERITS` не вдалося розв'язати (немає теми, цикл, некоректний id).
    pub inherit_error: Option<String>,
    /// Cache of files generated from tokens.toml (`~/.cache/desktop-themes/<id>`).
    pub cache_dir: PathBuf,
}

/// Максимальна глибина ланцюжка успадкування.
pub const MAX_INHERIT_DEPTH: usize = 8;
/// Ключі theme.conf, які дочірня тема не успадковує.
const OWN_CONF_KEYS: [&str; 3] = ["NAME", "DESCRIPTION", "INHERITS"];

/// Застосовує extensions.txt дочірньої теми до списку батьківської.
/// Рядки `+uuid` додають, `-uuid` прибирають; якщо є хоч один рядок без префікса, список
/// батьківської теми не береться (дочірня задає свій повний список), але `+`/`-` усе одно діють.
pub fn merge_extension_lines(base: &[String], text: &str) -> Vec<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .collect();
    let replace = lines
        .iter()
        .any(|l| !l.starts_with('+') && !l.starts_with('-'));
    let mut out: Vec<String> = if replace { vec![] } else { base.to_vec() };
    for l in lines {
        if let Some(u) = l.strip_prefix('-') {
            let u = u.trim();
            out.retain(|x| x != u);
        } else {
            let u = l.strip_prefix('+').unwrap_or(l).trim();
            if !u.is_empty() && !out.iter().any(|x| x == u) {
                out.push(u.to_string());
            }
        }
    }
    out
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
        Theme::load(ctx, id, &mut Vec::new())
    }

    fn load(ctx: &Ctx, id: &str, chain: &mut Vec<String>) -> Theme {
        let dir = ctx.themes.join(id);
        let own = read_conf(&dir.join("theme.conf"));
        chain.push(id.to_string());
        let mut parent = None;
        let mut inherit_error = None;
        if let Some(pid) = own.get("INHERITS").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if !util::valid_id(pid) {
                inherit_error = Some(format!("INHERITS: некоректний id «{pid}»"));
            } else if chain.iter().any(|c| c == pid) {
                inherit_error = Some(format!(
                    "INHERITS: цикл успадкування ({} → {pid})",
                    chain.join(" → ")
                ));
            } else if chain.len() >= MAX_INHERIT_DEPTH {
                inherit_error = Some(format!(
                    "INHERITS: ланцюжок довший за {MAX_INHERIT_DEPTH}"
                ));
            } else if !ctx.themes.join(pid).is_dir() {
                inherit_error = Some(format!("INHERITS: тему «{pid}» не знайдено"));
            } else {
                let p = Theme::load(ctx, pid, chain);
                if let Some(e) = &p.inherit_error {
                    inherit_error = Some(format!("{pid}: {e}"));
                }
                parent = Some(Box::new(p));
            }
        }
        chain.pop();
        let mut conf = BTreeMap::new();
        if let Some(p) = &parent {
            for (k, v) in &p.conf {
                if !OWN_CONF_KEYS.contains(&k.as_str()) {
                    conf.insert(k.clone(), v.clone());
                }
            }
        }
        conf.extend(own);
        Theme {
            id: id.to_string(),
            home: ctx.home.to_string_lossy().into_owned(),
            dir,
            conf,
            parent,
            inherit_error,
            cache_dir: crate::tokens::cache_root(ctx).join(id),
        }
    }

    /// Id батьківської теми, якщо `INHERITS` задано й розв'язано.
    pub fn inherits(&self) -> Option<&str> {
        self.parent.as_deref().map(|p| p.id.as_str())
    }

    /// Теки теми від дочірньої до найстаршого предка.
    pub fn layers(&self) -> Vec<&Path> {
        let mut v = vec![self.dir.as_path()];
        let mut p = self.parent.as_deref();
        while let Some(t) = p {
            v.push(t.dir.as_path());
            p = t.parent.as_deref();
        }
        v
    }

    /// Перший шар, у якому є `rel` (файл або тека).
    pub fn find(&self, rel: &str) -> Option<PathBuf> {
        self.layers()
            .into_iter()
            .map(|d| d.join(rel))
            .find(|p| p.exists())
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

    /// Усі *.dconf-файли модуля (з урахуванням успадкування: файл дочірньої теми перекриває
    /// однойменний батьківський), упорядковані за назвою файлу.
    pub fn dconf_files(&self, module: Module) -> Vec<PathBuf> {
        let mut by_name: BTreeMap<String, PathBuf> = BTreeMap::new();
        for layer in self.layers() {
            let Ok(rd) = std::fs::read_dir(layer) else {
                continue;
            };
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.ends_with(".dconf")
                    && e.path().is_file()
                    && Module::for_dconf_file(&n) == module
                {
                    by_name.entry(n).or_insert_with(|| e.path());
                }
            }
        }
        by_name.into_values().collect()
    }

    /// Keys of the module's *.dconf files only (no tokens, no placeholder expansion).
    fn file_settings(&self, module: Module) -> Settings {
        let mut d = Settings::new();
        for f in self.dconf_files(module) {
            if let Ok(s) = dconf::parse_file(&f) {
                d.extend(s);
            }
        }
        d
    }

    /// The theme's dconf files choose the shell theme themselves (then no shell theme is
    /// generated from tokens).
    pub fn handmade_shell_theme(&self) -> bool {
        self.file_settings(Module::Look)
            .contains_key(crate::SHELL_THEME_KEY)
    }

    /// Ключі dconf модуля (список розширень керується тільки extensions.txt). Keys derived from
    /// tokens.toml come first, the theme's *.dconf files override them.
    pub fn settings(&self, module: Module) -> Settings {
        let files = self.file_settings(module);
        let handmade_shell = module == Module::Look && files.contains_key(crate::SHELL_THEME_KEY);
        let mut d: Settings = crate::tokens::keys_for(self, module, handmade_shell)
            .into_iter()
            .collect();
        d.extend(files);
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

    /// UUID розширень: батьківський список, змінений рядками `+uuid`/`-uuid` дочірньої теми
    /// (див. [`merge_extension_lines`]).
    /// A generated shell theme needs the user-theme extension, so it is added when the tokens
    /// produce one.
    pub fn extensions(&self) -> Vec<String> {
        let base = self
            .parent
            .as_deref()
            .map(|p| p.extensions())
            .unwrap_or_default();
        let mut v = match std::fs::read_to_string(self.dir.join("extensions.txt")) {
            Ok(text) => merge_extension_lines(&base, &text),
            Err(_) => base,
        };
        let ut = theme_tokens::USER_THEME_EXTENSION;
        if !v.iter().any(|u| u == ut) && crate::tokens::outputs(self).shell {
            v.push(ut.to_string());
        }
        v
    }

    pub fn has_extensions_file(&self) -> bool {
        self.layers()
            .iter()
            .any(|d| d.join("extensions.txt").is_file())
    }

    /// Тека `gtk4`/`gtk3` (перший шар, у якому вона є; теки не зливаються).
    pub fn handmade_gtk_dir(&self, kind: &str) -> Option<PathBuf> {
        self.find(kind).filter(|p| p.is_dir())
    }

    /// The hand-made directory, else (gtk4 only) the one generated from tokens.toml.
    pub fn gtk_dir(&self, kind: &str) -> Option<PathBuf> {
        self.handmade_gtk_dir(kind).or_else(|| {
            (kind == "gtk4" && crate::tokens::outputs(self).gtk)
                .then(|| self.cache_dir.join(theme_tokens::generate::GTK4_DIR))
        })
    }

    pub fn gtk_entries(&self, kind: &str) -> Vec<String> {
        let Some(dir) = self.handmade_gtk_dir(kind) else {
            // generated: known without reading the (possibly not yet written) cache
            if self.gtk_dir(kind).is_some() {
                return vec!["gtk.css".to_string()];
            }
            return vec![];
        };
        let Ok(rd) = std::fs::read_dir(dir) else {
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
        let Some(path) = self.find("flatpak.conf") else {
            return vec![];
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return vec![];
        };
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| shlex_split(l).ok())
            .filter(|v| !v.is_empty())
            .collect()
    }

    /// apps/hooks/<name> (apply | revert), якщо файл виконуваний (успадковується).
    pub fn hook(&self, name: &str) -> Option<PathBuf> {
        self.layers()
            .into_iter()
            .map(|d| d.join("apps/hooks").join(name))
            .find(|p| is_executable(p))
    }

    /// Чи містить тема хоч щось для модуля.
    pub fn has_module(&self, m: Module) -> bool {
        match m {
            Module::Look => {
                !self.dconf_files(Module::Look).is_empty()
                    || crate::tokens::has_tokens(self)
                    || self.has_extensions_file()
                    || self.find("flatpak.conf").is_some_and(|p| p.is_file())
                    || GTK_KINDS.iter().any(|k| !self.gtk_entries(k).is_empty())
            }
            Module::Apps => {
                !self.dconf_files(m).is_empty()
                    || self.hook("apply").is_some()
                    || self.hook("revert").is_some()
                    || !crate::tokens::keys_for(self, m, false).is_empty()
            }
            _ => !self.dconf_files(m).is_empty() || !crate::tokens::keys_for(self, m, false).is_empty(),
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

    #[test]
    fn extension_line_merge() {
        let base: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(merge_extension_lines(&base, "+d\n-b\n# x\n"), vec!["a", "c", "d"]);
        assert_eq!(merge_extension_lines(&base, "+a\n"), vec!["a", "b", "c"]);
        assert_eq!(merge_extension_lines(&base, "x\ny # c\n-x\n"), vec!["y"]);
        assert_eq!(merge_extension_lines(&base, ""), base);
        assert_eq!(merge_extension_lines(&[], "- a\n+ b\n"), vec!["b"]);
    }

    #[test]
    fn inheritance() {
        let root = tmp_root("inherit");
        let ctx = Ctx::new(root.join("home"), root.clone());
        let p = ctx.themes.join("parent");
        write(
            &p.join("theme.conf"),
            "NAME=Parent\nDESCRIPTION=d\nPREVIEW=/p.png\n",
        );
        write(
            &p.join("settings.dconf"),
            "[org/gnome/desktop/interface]\ngtk-theme='P'\nicon-theme='PI'\n",
        );
        write(&p.join("spot.dconf"), "[org/x]\na=1\n");
        write(&p.join("behavior.dconf"), "[org/gnome/mutter]\nedge-tiling=true\n");
        write(&p.join("sounds.dconf"), "[org/gnome/desktop/sound]\ntheme-name='p'\n");
        write(&p.join("extensions.txt"), "a@x\nb@x\nc@x\n");
        write(&p.join("flatpak.conf"), "--env=A=1\n");
        write(&p.join("gtk4/gtk.css"), "x");
        let hook = p.join("apps/hooks/apply");
        write(&hook, "#!/bin/sh\n");
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let c = ctx.themes.join("child");
        write(&c.join("theme.conf"), "NAME=Child\nINHERITS=parent\n");
        // перекриває файл батька повністю (icon-theme зникає), додає власний
        write(
            &c.join("settings.dconf"),
            "[org/gnome/desktop/interface]\ngtk-theme='C'\n",
        );
        write(&c.join("glass.dconf"), "[org/y]\nb=2\n");
        write(&c.join("extensions.txt"), "+d@x\n-b@x\n");

        let t = get_theme(&ctx, "child").unwrap();
        assert_eq!(t.inherits(), Some("parent"));
        assert!(t.inherit_error.is_none());
        assert_eq!(t.name(), "Child");
        assert_eq!(t.description(), "", "DESCRIPTION is not inherited");
        assert_eq!(t.preview(), "/p.png", "PREVIEW is inherited");
        let look = t.settings(Module::Look);
        assert_eq!(look["/org/gnome/desktop/interface/gtk-theme"], "'C'");
        assert!(!look.contains_key("/org/gnome/desktop/interface/icon-theme"));
        assert_eq!(look["/org/x/a"], "1");
        assert_eq!(look["/org/y/b"], "2");
        assert_eq!(t.settings(Module::Behavior).len(), 1);
        assert_eq!(t.settings(Module::Sounds).len(), 1);
        assert_eq!(t.extensions(), vec!["a@x", "c@x", "d@x"]);
        assert_eq!(t.flatpak_lines(), vec![vec!["--env=A=1".to_string()]]);
        assert_eq!(t.gtk_entries("gtk4"), vec!["gtk.css"]);
        assert_eq!(t.gtk_dir("gtk4").unwrap(), p.join("gtk4"));
        assert_eq!(t.hook("apply").unwrap(), hook);
        assert_eq!(t.modules(), Module::ALL.to_vec());

        // онук: без extensions.txt бере список батька (уже змінений)
        let g = ctx.themes.join("grandchild");
        write(&g.join("theme.conf"), "INHERITS=child\n");
        let gt = get_theme(&ctx, "grandchild").unwrap();
        assert_eq!(gt.extensions(), vec!["a@x", "c@x", "d@x"]);
        assert_eq!(gt.layers().len(), 3);
        assert_eq!(gt.name(), "grandchild");

        // помилки: відсутня тема, цикл
        write(&ctx.themes.join("orphan/theme.conf"), "INHERITS=nope\n");
        let o = get_theme(&ctx, "orphan").unwrap();
        assert!(o.parent.is_none());
        assert!(o.inherit_error.as_deref().unwrap().contains("nope"));
        write(&ctx.themes.join("cy1/theme.conf"), "INHERITS=cy2\n");
        write(&ctx.themes.join("cy2/theme.conf"), "INHERITS=cy1\n");
        let cy = get_theme(&ctx, "cy1").unwrap();
        assert!(cy.inherit_error.as_deref().unwrap().contains("цикл"));
        assert_eq!(cy.layers().len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }
}
