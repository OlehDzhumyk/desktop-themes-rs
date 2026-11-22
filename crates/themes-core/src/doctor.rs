// SPDX-License-Identifier: GPL-3.0-or-later
//! «Перевірити систему»: стан розширень, журнал, відсутні теми/значки/курсори/звуки,
//! зміна версії GNOME. `--fix` вимикає зламані розширення і повертає полагоджені.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::apply::{disable_broken, ext_lists, ext_stamp};
use crate::dconf;
use crate::error::Result;
use crate::lock::Lock;
use crate::modules::Module;
use crate::paths::Ctx;
use crate::shell::{ExtState, Shell};
use crate::state::State;
use crate::theme::Theme;
use crate::util::{log, now, run};
use crate::{EXT_KEY, PERMANENT_EXTENSIONS};

#[derive(Debug, Serialize, Clone)]
pub struct Issue {
    /// error | warning | info
    pub severity: &'static str,
    pub kind: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    pub fixable: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct DoctorReport {
    pub ok: bool,
    /// GNOME вимкнув користувацькі розширення (`disable-user-extensions=true`).
    pub extensions_disabled_by_gnome: bool,
    /// Останні падіння справжньої оболонки (найновіші спершу).
    pub shell_crashes: Vec<crate::crash::ShellCrash>,
    pub shell_running: bool,
    pub gnome_version: Option<String>,
    pub previous_gnome_version: Option<String>,
    pub version_changed: bool,
    pub extensions_checked: usize,
    pub issues: Vec<Issue>,
    pub fixed: Vec<String>,
    pub restored: Vec<String>,
    pub broken_recorded: Vec<String>,
}

impl DoctorReport {
    fn push(
        &mut self,
        severity: &'static str,
        kind: &'static str,
        message: String,
        uuid: Option<String>,
        fixable: bool,
    ) {
        self.issues.push(Issue {
            severity,
            kind,
            message,
            uuid,
            fixable,
        });
    }
    pub fn problems(&self) -> usize {
        self.issues.iter().filter(|i| i.severity != "info").count()
    }
}

fn shell_version_offline() -> Option<String> {
    let o = run("gnome-shell", &["--version"], None).ok()?;
    o.stdout.split_whitespace().last().map(String::from)
}

fn major(v: &str) -> &str {
    v.split('.').next().unwrap_or(v)
}

/// Чи заявляє metadata.json підтримку цієї версії оболонки.
fn metadata_supports(ctx: &Ctx, uuid: &str, ver: &str) -> Option<bool> {
    let dir = ctx.extension_path(uuid)?;
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("metadata.json")).ok()?).ok()?;
    let list = v.get("shell-version")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|x| x.as_str())
            .any(|s| s == ver || s == major(ver)),
    )
}

fn search_dirs(ctx: &Ctx, kind: &str) -> Vec<PathBuf> {
    let h = &ctx.home;
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut v = match kind {
        "themes" => vec![h.join(".local/share/themes"), h.join(".themes")],
        "icons" => vec![h.join(".local/share/icons"), h.join(".icons")],
        _ => vec![h.join(".local/share/sounds")],
    };
    for d in data_dirs.split(':').filter(|d| !d.is_empty()) {
        v.push(PathBuf::from(d).join(kind));
    }
    v.push(PathBuf::from("/usr/share").join(kind));
    v
}

fn find_asset(ctx: &Ctx, kind: &str, name: &str, sub: Option<&str>) -> bool {
    search_dirs(ctx, kind).iter().any(|d| {
        let p = d.join(name);
        match sub {
            Some(s) => p.join(s).exists(),
            None => p.is_dir(),
        }
    })
}

const BUILTIN_GTK: &[&str] = &[
    "Adwaita",
    "Adwaita-dark",
    "HighContrast",
    "HighContrastInverse",
    "Default",
    "Emacs",
];

/// Відсутні теми/значки/курсори/звуки/шпалери, на які посилаються активні теми та dconf.
/// Захист GNOME після збою (`disable-user-extensions`) і останні падіння оболонки. Нічого не
/// виправляє автоматично: розширення повертаються лише за згодою користувача
/// (`themes doctor --enable-extensions` або кнопка у вікні).
fn check_crashes(ctx: &Ctx, rep: &mut DoctorReport) {
    rep.shell_crashes = crate::crash::recent_shell_crashes(ctx, 48);
    if crate::crash::user_extensions_disabled() {
        rep.extensions_disabled_by_gnome = true;
        let mut msg = "GNOME вимкнув розширення після збою оболонки (disable-user-extensions=true): \
                       жодне розширення зараз не працює"
            .to_string();
        if let Some(c) = rep.shell_crashes.first() {
            msg.push_str(&format!(". {}", crate::crash::describe(c)));
            if let Some(t) = &c.theme {
                msg.push_str(&format!(
                    ". Радимо спершу перейти на безпечну тему (themes apply {}), якщо винна «{t}»",
                    crate::BUILTIN_THEME
                ));
            }
        }
        msg.push_str(". Увімкнути знову: themes doctor --enable-extensions");
        rep.push("error", "extensions-disabled", msg, None, false);
    } else if let Some(c) = rep.shell_crashes.first() {
        rep.push("warning", "shell-crash", crate::crash::describe(c), None, false);
    }
}

fn check_assets(ctx: &Ctx, st: &State, rep: &mut DoctorReport) {
    for t in crate::theme::all_themes(ctx) {
        if let Some(e) = &t.inherit_error {
            rep.push("warning", "inherits", format!("тема «{}»: {e}", t.id), None, false);
        }
        match crate::tokens::load(&t) {
            Some(Err(e)) => {
                for i in &e.errors {
                    rep.push("error", "tokens", format!("тема «{}»: {i}", t.id), None, false);
                }
            }
            Some(Ok(l)) => {
                for i in &l.warnings {
                    rep.push("info", "tokens", format!("тема «{}»: {i}", t.id), None, false);
                }
            }
            None => {}
        }
    }
    // (що, ключ dconf, схема, ключ схеми)
    let checks: &[(&str, &str, &str, &str)] = &[
        (
            "gtk",
            "/org/gnome/desktop/interface/gtk-theme",
            "org.gnome.desktop.interface",
            "gtk-theme",
        ),
        (
            "icons",
            "/org/gnome/desktop/interface/icon-theme",
            "org.gnome.desktop.interface",
            "icon-theme",
        ),
        (
            "cursor",
            "/org/gnome/desktop/interface/cursor-theme",
            "org.gnome.desktop.interface",
            "cursor-theme",
        ),
        (
            "sound",
            "/org/gnome/desktop/sound/theme-name",
            "org.gnome.desktop.sound",
            "theme-name",
        ),
        (
            "shell",
            crate::SHELL_THEME_KEY,
            "org.gnome.shell.extensions.user-theme",
            "name",
        ),
    ];
    let mut wanted: Vec<(&str, String, String)> = Vec::new(); // (тип, значення, звідки)
    for (kind, key, schema, skey) in checks {
        if let Some(v) = dconf::read(key)
            .and_then(|v| dconf::unquote_str(&v))
            .or_else(|| dconf::effective_string(schema, skey))
        {
            wanted.push((kind, v, "dconf".into()));
        }
        for m in Module::ALL {
            let Some(src) = st.source(m) else { continue };
            let t = Theme::new(ctx, src);
            if let Some(v) = t.settings(m).get(*key).and_then(|v| dconf::unquote_str(v)) {
                wanted.push((kind, v, format!("тема {src} ({m})")));
            }
        }
    }
    let mut seen = BTreeSet::new();
    for (kind, name, from) in wanted {
        if name.is_empty() || !seen.insert((kind, name.clone())) {
            continue;
        }
        let ok = match kind {
            "gtk" => BUILTIN_GTK.contains(&name.as_str()) || find_asset(ctx, "themes", &name, None),
            "icons" => find_asset(ctx, "icons", &name, Some("index.theme")),
            "cursor" => find_asset(ctx, "icons", &name, Some("cursors")),
            "sound" => {
                find_asset(ctx, "sounds", &name, Some("index.theme"))
                    || find_asset(ctx, "sounds", &name, None)
            }
            "shell" => find_asset(ctx, "themes", &name, Some("gnome-shell")),
            _ => true,
        };
        if !ok {
            let what = match kind {
                "gtk" => "тему GTK",
                "icons" => "тему значків",
                "cursor" => "тему курсорів",
                "sound" => "звукову тему",
                _ => "тему оболонки",
            };
            rep.push(
                "warning",
                "missing-asset",
                format!("не знайдено {what} «{name}» (з: {from})"),
                None,
                false,
            );
        }
    }
    for key in [
        "/org/gnome/desktop/background/picture-uri",
        "/org/gnome/desktop/background/picture-uri-dark",
    ] {
        if let Some(p) = dconf::read(key).and_then(|v| crate::util::file_from_uri(&v))
            && !std::path::Path::new(&p).exists()
        {
            rep.push(
                "warning",
                "missing-asset",
                format!("файл шпалер не існує: {p}"),
                None,
                false,
            );
        }
    }
}

/// Generated files of the active themes went missing (e.g. ~/.cache was cleaned): `--fix`
/// regenerates them (the links point at stable paths, so they work again).
fn check_token_cache(ctx: &Ctx, st: &State, fix: bool, rep: &mut DoctorReport) {
    let mut seen = BTreeSet::new();
    for m in [Module::Look, Module::Apps] {
        let Some(src) = st.source(m) else { continue };
        if !seen.insert(src.to_string()) {
            continue;
        }
        let t = Theme::new(ctx, src);
        let Ok(files) = crate::tokens::plan(&t) else { continue };
        if files.iter().all(|f| f.status == "cached") {
            continue;
        }
        if fix && crate::tokens::materialize(&t, false).is_ok() {
            rep.fixed
                .push(format!("згенеровано файли токенів теми «{src}»"));
        } else {
            rep.push(
                "warning",
                "tokens-cache",
                format!("файли, згенеровані з tokens.toml теми «{src}», відсутні або застарілі (themes doctor --fix)"),
                None,
                true,
            );
        }
    }
}

/// Помилки розширень у журналі за останні 30 хвилин (поточне завантаження).
fn check_journal(enabled: &[String], rep: &mut DoctorReport) {
    let Ok(o) = run(
        "journalctl",
        &[
            "--user",
            "-b",
            "--since",
            "-30min",
            "-o",
            "cat",
            "--no-pager",
            "-q",
            "_COMM=gnome-shell",
        ],
        None,
    ) else {
        return;
    };
    let mut seen = BTreeSet::new();
    for line in o.stdout.lines() {
        let l = line.trim();
        let is_err = l.contains("JS ERROR")
            || l.contains("Error")
            || l.contains("error")
            || l.contains("Exception");
        if !is_err {
            continue;
        }
        let Some(u) = enabled.iter().find(|u| l.contains(u.as_str())) else {
            continue;
        };
        let short: String = l.chars().take(220).collect();
        if seen.insert(short.clone()) && seen.len() <= 8 {
            rep.push(
                "warning",
                "journal",
                format!("журнал: {short}"),
                Some(u.clone()),
                false,
            );
        }
    }
}

pub struct DoctorOptions {
    pub fix: bool,
    /// Скільки чекати на GNOME Shell (для запуску під час входу).
    pub wait_shell: Duration,
    pub notify: bool,
}

pub fn doctor(ctx: &Ctx, opt: &DoctorOptions) -> Result<DoctorReport> {
    crate::util::ensure_bus();
    let mut rep = DoctorReport::default();
    let shell = if opt.wait_shell.is_zero() {
        Shell::connect()
    } else {
        Shell::wait(opt.wait_shell)
    };
    if shell.is_some() && !opt.wait_shell.is_zero() {
        // дати оболонці час завантажити розширення після входу
        if let Some(sh) = &shell {
            let cur = dconf::dump()?;
            let (en, ..) = ext_lists(&cur);
            if !sh.screen_locked() {
                let _ = sh.settle_targets(&en, &[], Duration::from_secs(20));
            }
        }
    }
    rep.shell_running = shell.is_some();
    let version = shell
        .as_ref()
        .and_then(Shell::version)
        .or_else(shell_version_offline);
    rep.gnome_version = version.clone();

    let mut st = State::load(ctx);
    rep.previous_gnome_version = st.gnome_version.clone();
    if let (Some(prev), Some(cur)) = (&st.gnome_version, &version)
        && prev != cur
    {
        rep.version_changed = true;
        rep.push(
            "info",
            "gnome-version",
            format!("версія GNOME Shell змінилася: {prev} → {cur} (після оновлення системи)"),
            None,
            false,
        );
    }

    let cur = dconf::dump()?;
    let (enabled, ..) = ext_lists(&cur);
    rep.extensions_checked = enabled.len();
    let ver = version.clone().unwrap_or_default();
    let mut to_disable: Vec<(String, String)> = Vec::new();
    // disable-user-extensions (захист GNOME після збою) перевіряє check_crashes()
    for u in &enabled {
        let installed = ctx.extension_path(u).is_some();
        let info = shell.as_ref().and_then(|s| s.ext_info(u));
        match (&shell, info) {
            (Some(sh), Some(i)) if i.state.broken() => {
                let mut msg = if i.error.is_empty() {
                    String::new()
                } else {
                    i.error.clone()
                };
                if msg.is_empty() {
                    msg = sh.ext_errors(u).join("; ");
                }
                let label = if i.state == ExtState::OutOfDate {
                    "застаріле (OUT_OF_DATE) для цієї версії GNOME"
                } else {
                    "помилка (ERROR)"
                };
                rep.push(
                    "error",
                    "extension",
                    format!(
                        "{u}: {label}{}",
                        if msg.is_empty() {
                            String::new()
                        } else {
                            format!(": {msg}")
                        }
                    ),
                    Some(u.clone()),
                    true,
                );
                to_disable.push((u.clone(), format!("{} {msg}", i.state.name())));
            }
            (Some(_), Some(_)) => {}
            (_, None) if !installed => rep.push(
                "warning",
                "extension",
                format!("{u}: увімкнене, але не встановлене"),
                Some(u.clone()),
                false,
            ),
            (Some(_), None) => rep.push(
                "info",
                "extension",
                format!(
                    "{u}: встановлене, але оболонка його ще не завантажила (потрібен перевхід)"
                ),
                Some(u.clone()),
                false,
            ),
            (None, _) => {
                if !ver.is_empty() && metadata_supports(ctx, u, &ver) == Some(false) {
                    rep.push(
                        "error",
                        "extension",
                        format!("{u}: metadata.json не підтримує GNOME {}", major(&ver)),
                        Some(u.clone()),
                        true,
                    );
                    to_disable.push((u.clone(), format!("OUT_OF_DATE (GNOME {})", major(&ver))));
                }
            }
        }
    }
    for p in PERMANENT_EXTENSIONS {
        if ctx.extension_path(p).is_none() {
            rep.push(
                "warning",
                "permanent",
                format!("розширення {p} (міст робочого столу: «Тема» у швидких налаштуваннях, буфер обміну) не встановлене"),
                None,
                false,
            );
        } else if !enabled.iter().any(|u| u == p) && !st.is_broken(p) {
            rep.push(
                "warning",
                "permanent",
                format!("розширення {p} вимкнене"),
                Some(p.to_string()),
                true,
            );
        }
    }
    check_journal(&enabled, &mut rep);
    check_crashes(ctx, &mut rep);
    check_assets(ctx, &st, &mut rep);
    check_token_cache(ctx, &st, opt.fix, &mut rep);
    for b in &st.broken_extensions {
        rep.broken_recorded.push(b.uuid.clone());
        rep.push(
            "info",
            "broken-recorded",
            format!("{} вимкнено {} ({})", b.uuid, b.since, b.reason),
            Some(b.uuid.clone()),
            false,
        );
    }

    if opt.fix {
        // 1. вимкнути зламані
        if !to_disable.is_empty() {
            if let Some(sh) = &shell {
                disable_broken(ctx, sh, &to_disable, "themes doctor --fix")?;
            } else {
                let _lock = Lock::acquire(ctx)?;
                let uu: BTreeSet<&String> = to_disable.iter().map(|x| &x.0).collect();
                let new_en: Vec<String> = enabled
                    .iter()
                    .filter(|u| !uu.contains(u))
                    .cloned()
                    .collect();
                dconf::write(EXT_KEY, &dconf::fmt_strv(&new_en))?;
                let mut s2 = State::load(ctx);
                for (u, r) in &to_disable {
                    if !s2.is_broken(u) {
                        s2.broken_extensions.push(crate::state::BrokenExtension {
                            uuid: u.clone(),
                            reason: format!("themes doctor --fix: {r}"),
                            since: now("%Y-%m-%d %H:%M:%S"),
                            shell_version: ver.clone(),
                            stamp: ext_stamp(ctx, u),
                        });
                    }
                }
                s2.save(ctx)?;
            }
            rep.fixed
                .extend(to_disable.iter().map(|(u, _)| format!("вимкнено {u}")));
        }
        // 2. повернути полагоджені (змінилася версія GNOME або оновилося саме розширення)
        st = State::load(ctx);
        let candidates: Vec<_> = st
            .broken_extensions
            .iter()
            .filter(|b| !to_disable.iter().any(|(u, _)| *u == b.uuid))
            .filter(|b| b.shell_version != ver || b.stamp != ext_stamp(ctx, &b.uuid))
            .filter(|b| ctx.extension_path(&b.uuid).is_some())
            .cloned()
            .collect();
        for b in candidates {
            let supported = metadata_supports(ctx, &b.uuid, &ver) != Some(false);
            if !supported {
                continue;
            }
            let _lock = Lock::acquire(ctx)?;
            let cur = dconf::dump()?;
            let (mut en, ..) = ext_lists(&cur);
            if !en.contains(&b.uuid) {
                en.push(b.uuid.clone());
            }
            dconf::write(EXT_KEY, &dconf::fmt_strv(&en))?;
            let ok = match &shell {
                Some(sh) => {
                    let infos = sh.settle(std::slice::from_ref(&b.uuid), Duration::from_secs(4));
                    std::thread::sleep(Duration::from_millis(500));
                    let st_now = sh.ext_info(&b.uuid).map(|i| i.state);
                    let _ = infos;
                    st_now == Some(ExtState::Active)
                }
                None => true, // перевіриться при вході
            };
            let mut s2 = State::load(ctx);
            if ok {
                s2.broken_extensions.retain(|x| x.uuid != b.uuid);
                rep.restored.push(b.uuid.clone());
            } else {
                en.retain(|u| *u != b.uuid);
                dconf::write(EXT_KEY, &dconf::fmt_strv(&en))?;
                if let Some(x) = s2.broken_extensions.iter_mut().find(|x| x.uuid == b.uuid) {
                    x.shell_version = ver.clone();
                    x.stamp = ext_stamp(ctx, &b.uuid);
                }
            }
            s2.save(ctx)?;
        }
        // 3. постійне розширення
        for p in PERMANENT_EXTENSIONS {
            let cur = dconf::dump()?;
            let (mut en, ..) = ext_lists(&cur);
            let s2 = State::load(ctx);
            if ctx.extension_path(p).is_some() && !en.iter().any(|u| u == p) && !s2.is_broken(p) {
                en.push(p.to_string());
                dconf::write(EXT_KEY, &dconf::fmt_strv(&en))?;
                rep.fixed.push(format!("увімкнено {p}"));
            }
        }
    }

    // запам'ятати версію
    {
        let _lock = Lock::acquire(ctx)?;
        let mut s2 = State::load(ctx);
        if version.is_some() {
            s2.gnome_version = version.clone();
        }
        s2.last_doctor = Some(now("%Y-%m-%d %H:%M:%S"));
        // запис про останнє падіння оболонки (тема й підозрювані розширення на той момент)
        if let Some(c) = rep.shell_crashes.first() {
            s2.extra.insert(
                "last_shell_crash".into(),
                serde_json::to_value(c).unwrap_or_default(),
            );
        }
        s2.save(ctx)?;
    }
    rep.ok = rep.issues.iter().all(|i| i.severity == "info")
        || (opt.fix && rep.issues.iter().all(|i| i.severity == "info" || i.fixable));
    if opt.fix
        && opt.notify
        && (!rep.fixed.is_empty()
            || !rep.restored.is_empty()
            || rep.version_changed
            || rep.extensions_disabled_by_gnome)
    {
        let mut body = Vec::new();
        if rep.extensions_disabled_by_gnome {
            body.push(
                "GNOME вимкнув розширення після збою оболонки. Відкрийте «Теми», щоб увімкнути їх знову."
                    .to_string(),
            );
        }
        if rep.version_changed {
            body.push(format!(
                "GNOME оновлено: {} → {}",
                rep.previous_gnome_version.clone().unwrap_or_default(),
                rep.gnome_version.clone().unwrap_or_default()
            ));
        }
        body.extend(rep.fixed.iter().cloned());
        body.extend(rep.restored.iter().map(|u| format!("знову увімкнено {u}")));
        let _ = crate::shell::notify(
            "Теми: перевірка системи",
            &body.join("\n"),
            "preferences-desktop-appearance-symbolic",
        );
    }
    log(
        ctx,
        &format!(
            "doctor fix={} shell={} version={} issues={} fixed={} restored={}",
            opt.fix,
            rep.shell_running,
            rep.gnome_version.clone().unwrap_or("-".into()),
            rep.problems(),
            rep.fixed.join(",").replace(' ', "_"),
            rep.restored.join(",")
        ),
    );
    Ok(rep)
}
