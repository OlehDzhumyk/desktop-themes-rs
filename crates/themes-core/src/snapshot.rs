// SPDX-License-Identifier: GPL-3.0-or-later
//! «Зберегти поточний вигляд як тему»: знімок, розкладений по модулях.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::dconf::{self, Settings};
use crate::error::Result;
use crate::lock::Lock;
use crate::modules::Module;
use crate::paths::Ctx;
use crate::state::State;
use crate::theme::{all_themes, protected_gtk, write_conf};
use crate::util::{copy_tree, file_from_uri, log, now, url_quote_path, valid_id};
use crate::{DIS_KEY, EXT_KEY, PERMANENT_EXTENSIONS};

const SNAPSHOT_PREFIXES: &[&str] = &[
    "/org/gnome/desktop/interface/",
    "/org/gnome/desktop/wm/preferences/",
    "/org/gnome/desktop/background/",
    "/org/gnome/desktop/screensaver/picture-",
    "/org/gnome/desktop/screensaver/primary-color",
    "/org/gnome/desktop/sound/",
    "/org/gnome/shell/extensions/",
    "/org/gnome/mutter/",
    "/org/gnome/desktop/wm/keybindings/",
    "/org/gnome/shell/keybindings/",
    "/org/gnome/shell/app-switcher/",
    "/org/gnome/shell/window-switcher/",
    "/org/gnome/desktop/peripherals/touchpad/",
];

/// Евристика модуля для ключа, якого не задає жодна тема.
pub fn guess_module(key: &str) -> Module {
    const BEHAVIOR: &[&str] = &[
        "/org/gnome/desktop/wm/keybindings/",
        "/org/gnome/shell/keybindings/",
        "/org/gnome/mutter/keybindings/",
        "/org/gnome/mutter/wayland/keybindings/",
        "/org/gnome/settings-daemon/plugins/media-keys/",
        "/org/gnome/shell/app-switcher/",
        "/org/gnome/shell/window-switcher/",
        "/org/gnome/desktop/peripherals/",
        "/org/gnome/desktop/interface/enable-animations",
        "/org/gnome/desktop/interface/enable-hot-corners",
        "/org/gnome/desktop/interface/font-antialiasing",
        "/org/gnome/desktop/interface/font-hinting",
        "/org/gnome/desktop/interface/font-rgba-order",
        "/org/gnome/desktop/wm/preferences/focus-mode",
        "/org/gnome/desktop/wm/preferences/action-",
        "/org/gnome/desktop/wm/preferences/mouse-button-modifier",
        "/org/gnome/mutter/edge-tiling",
        "/org/gnome/mutter/dynamic-workspaces",
        "/org/gnome/mutter/workspaces-only-on-primary",
        "/org/gnome/mutter/overlay-key",
    ];
    if key.starts_with("/org/gnome/desktop/sound/") {
        Module::Sounds
    } else if key.starts_with("/org/gnome/Ptyxis/") || key.starts_with("/org/gnome/TextEditor/") {
        Module::Apps
    } else if BEHAVIOR.iter().any(|p| key.starts_with(p)) {
        Module::Behavior
    } else {
        Module::Look
    }
}

/// Модуль ключа: як у наявних темах (не-«вигляд» має пріоритет), інакше евристика.
pub fn module_for_key(key: &str, known: &BTreeMap<Module, BTreeSet<String>>) -> Module {
    for m in [Module::Behavior, Module::Sounds, Module::Apps] {
        if known.get(&m).is_some_and(|s| s.contains(key)) {
            return m;
        }
    }
    if known.get(&Module::Look).is_some_and(|s| s.contains(key)) {
        return Module::Look;
    }
    guess_module(key)
}

#[derive(Debug, Serialize)]
pub struct SnapshotResult {
    pub ok: bool,
    pub theme: String,
    pub name: String,
    pub keys: usize,
    pub module_keys: BTreeMap<Module, usize>,
    pub extensions: Vec<String>,
    pub path: String,
    pub warnings: Vec<String>,
}

pub fn snapshot(ctx: &Ctx, id: &str, name: Option<&str>, force: bool) -> Result<SnapshotResult> {
    if !valid_id(id) {
        crate::bail!("некоректний id: дозволені a-z 0-9 . _ - (до 64 символів)");
    }
    if id == crate::BUILTIN_THEME {
        crate::bail!("id «fedora» зарезервовано");
    }
    crate::util::ensure_bus();
    let _lock = Lock::acquire(ctx)?;
    let tdir = ctx.themes.join(id);
    if tdir.exists() {
        if !force {
            crate::bail!("тема «{id}» вже існує (додайте --force для перезапису)");
        }
        std::fs::remove_dir_all(&tdir)?;
    }
    let baseline = dconf::parse_file(&ctx.baseline_dir()?.join("full.dconf"))?;
    let cur = dconf::dump()?;
    let st = State::load(ctx);
    let mut known: BTreeMap<Module, BTreeSet<String>> = BTreeMap::new();
    for t in all_themes(ctx) {
        for m in Module::ALL {
            known
                .entry(m)
                .or_default()
                .extend(t.settings(m).into_keys());
        }
    }
    let mut managed: BTreeSet<String> = st.applied_keys.iter().cloned().collect();
    managed.extend(known.values().flatten().cloned());
    let mut warnings = Vec::new();
    let mut settings = Settings::new();
    for (k, v) in &cur {
        if k == EXT_KEY || k == DIS_KEY {
            continue;
        }
        if (managed.contains(k) || SNAPSHOT_PREFIXES.iter().any(|p| k.starts_with(p)))
            && baseline.get(k) != Some(v)
        {
            settings.insert(k.clone(), v.clone());
        }
    }
    for k in baseline.keys() {
        if SNAPSHOT_PREFIXES.iter().any(|p| k.starts_with(p)) && !cur.contains_key(k) {
            warnings.push(format!(
                "ключ {k} скинуто відносно baseline — у знімку не відображається"
            ));
        }
    }
    std::fs::create_dir_all(&tdir)?;
    // шпалери з кореня тем копіюємо, щоб знімок пережив видалення теми-джерела
    let root_real = std::fs::canonicalize(&ctx.root).unwrap_or(ctx.root.clone());
    for key in [
        "/org/gnome/desktop/background/picture-uri",
        "/org/gnome/desktop/background/picture-uri-dark",
        "/org/gnome/desktop/screensaver/picture-uri",
    ] {
        let Some(p) = settings.get(key).and_then(|v| file_from_uri(v)) else {
            continue;
        };
        let pp = std::path::Path::new(&p);
        let real = std::fs::canonicalize(pp).unwrap_or(pp.to_path_buf());
        if real.starts_with(&root_real) && pp.is_file() {
            let ext = pp
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let leaf = key
                .rsplit('/')
                .next()
                .unwrap_or("")
                .replace("picture-uri", "img");
            let dst = tdir.join(format!("wallpaper-{leaf}{ext}"));
            std::fs::copy(pp, &dst)?;
            settings.insert(
                key.into(),
                format!("'file://{}'", url_quote_path(&dst.to_string_lossy())),
            );
        }
    }
    // розкласти по модулях
    let mut per: BTreeMap<Module, Settings> = BTreeMap::new();
    for (k, v) in &settings {
        per.entry(module_for_key(k, &known))
            .or_default()
            .insert(k.clone(), v.clone());
    }
    let mut module_keys = BTreeMap::new();
    for (m, s) in &per {
        let file = match m {
            Module::Look => "settings.dconf",
            Module::Behavior => "behavior.dconf",
            Module::Sounds => "sounds.dconf",
            Module::Apps => "apps.dconf",
        };
        std::fs::write(tdir.join(file), dconf::to_text(s))?;
        module_keys.insert(*m, s.len());
    }
    if !per.contains_key(&Module::Look) {
        std::fs::write(tdir.join("settings.dconf"), "")?;
    }
    // розширення
    let (cur_en, _, default_en, _) = crate::apply::ext_lists(&cur);
    let base_en = baseline
        .get(EXT_KEY)
        .map(|s| dconf::parse_strv(s))
        .unwrap_or(default_en);
    let exts: Vec<String> = cur_en
        .iter()
        .filter(|u| !base_en.contains(u) && !PERMANENT_EXTENSIONS.contains(&u.as_str()))
        .cloned()
        .collect();
    for u in &base_en {
        if !cur_en.contains(u) {
            warnings.push(format!(
                "розширення {u} з baseline зараз вимкнене — знімок цього не збереже"
            ));
        }
    }
    if !exts.is_empty() {
        std::fs::write(tdir.join("extensions.txt"), exts.join("\n") + "\n")?;
    }
    // GTK (розіменовуючи наші посилання)
    for (kind, d) in [("gtk4", &ctx.gtk4), ("gtk3", &ctx.gtk3)] {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        let entries: Vec<_> = rd
            .flatten()
            .filter(|e| !protected_gtk(kind, &e.file_name().to_string_lossy()))
            .collect();
        if entries.is_empty() {
            continue;
        }
        std::fs::create_dir_all(tdir.join(kind))?;
        for e in entries {
            let s = e.path();
            if s.exists() {
                copy_tree(&s, &tdir.join(kind).join(e.file_name()))?;
            }
        }
    }
    // flatpak — від теми «вигляду»
    if let Some(look) = st.source(Module::Look) {
        let f = ctx.themes.join(look).join("flatpak.conf");
        if f.exists() {
            std::fs::copy(&f, tdir.join("flatpak.conf"))?;
        }
    }
    // прев'ю
    let scheme = cur
        .get("/org/gnome/desktop/interface/color-scheme")
        .or(baseline.get("/org/gnome/desktop/interface/color-scheme"))
        .cloned()
        .unwrap_or_default();
    let dark = scheme.contains("dark");
    let uri = |key: &str, leaf: &str| {
        settings
            .get(key)
            .or(cur.get(key))
            .cloned()
            .or_else(|| dconf::schema_default("org.gnome.desktop.background", leaf))
    };
    let lp = uri("/org/gnome/desktop/background/picture-uri", "picture-uri")
        .and_then(|u| file_from_uri(&u));
    let dp = uri(
        "/org/gnome/desktop/background/picture-uri-dark",
        "picture-uri-dark",
    )
    .and_then(|u| file_from_uri(&u));
    let tname = name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or(id)
        .to_string();
    let mut conf = vec![
        ("NAME", tname.clone()),
        (
            "DESCRIPTION",
            format!("Знімок вигляду від {}", now("%d.%m.%Y %H:%M")),
        ),
    ];
    let preview = if dark {
        dp.clone().or(lp.clone())
    } else {
        lp.clone().or(dp.clone())
    };
    conf.push(("PREVIEW", preview.unwrap_or_default()));
    if let (Some(l), Some(d)) = (&lp, &dp)
        && l != d
    {
        conf[2].1 = l.clone();
        conf.push(("PREVIEW_DARK", d.clone()));
    }
    write_conf(&tdir.join("theme.conf"), &conf)?;
    log(
        ctx,
        &format!(
            "snapshot {id} ({tname}) keys={} ext={}",
            settings.len(),
            exts.len()
        ),
    );
    Ok(SnapshotResult {
        ok: true,
        theme: id.into(),
        name: tname,
        keys: settings.len(),
        module_keys,
        extensions: exts,
        path: tdir.to_string_lossy().into(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_to_module() {
        let mut known: BTreeMap<Module, BTreeSet<String>> = BTreeMap::new();
        known
            .entry(Module::Look)
            .or_default()
            .insert("/org/gnome/desktop/wm/keybindings/maximize".into());
        known
            .entry(Module::Behavior)
            .or_default()
            .insert("/org/gnome/mutter/center-new-windows".into());
        assert_eq!(
            module_for_key("/org/gnome/desktop/wm/keybindings/maximize", &known),
            Module::Look
        );
        assert_eq!(
            module_for_key("/org/gnome/mutter/center-new-windows", &known),
            Module::Behavior
        );
        assert_eq!(
            module_for_key("/org/gnome/desktop/wm/keybindings/close", &known),
            Module::Behavior
        );
        assert_eq!(
            module_for_key("/org/gnome/desktop/sound/theme-name", &known),
            Module::Sounds
        );
        assert_eq!(
            module_for_key("/org/gnome/Ptyxis/font-name", &known),
            Module::Apps
        );
        assert_eq!(
            module_for_key("/org/gnome/desktop/interface/gtk-theme", &known),
            Module::Look
        );
        assert_eq!(
            module_for_key("/org/gnome/desktop/interface/enable-animations", &known),
            Module::Behavior
        );
    }
}
