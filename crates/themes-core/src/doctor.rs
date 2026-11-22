// SPDX-License-Identifier: GPL-3.0-or-later
//! «Перевірити систему»: стан розширень, журнал, відсутні теми/значки/курсори/звуки,
//! зміна версії GNOME. `--fix` вимикає зламані розширення і повертає полагоджені.

use desk_settings::tr;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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
        let mut msg = tr!(
            "GNOME turned extensions off after a shell crash (disable-user-extensions=true): no \
             extension works now"
        );
        if let Some(c) = rep.shell_crashes.first() {
            msg.push_str(&format!(". {}", crate::crash::describe(c)));
            if let Some(t) = &c.theme {
                msg.push_str(". ");
                msg.push_str(&tr!(
                    "Better switch to the safe theme first (themes apply {safe}) if “{theme}” is to blame",
                    safe = crate::BUILTIN_THEME,
                    theme = t
                ));
            }
        }
        msg.push_str(". ");
        msg.push_str(&tr!(
            "To turn them on again: themes doctor --enable-extensions"
        ));
        rep.push("error", "extensions-disabled", msg, None, false);
    } else if let Some(c) = rep.shell_crashes.first() {
        rep.push(
            "warning",
            "shell-crash",
            crate::crash::describe(c),
            None,
            false,
        );
    }
}

fn check_assets(ctx: &Ctx, st: &State, rep: &mut DoctorReport) {
    for t in crate::theme::all_themes(ctx) {
        if let Some(e) = &t.inherit_error {
            rep.push(
                "warning",
                "inherits",
                tr!("theme “{theme}”: {error}", theme = t.id, error = e),
                None,
                false,
            );
        }
        match crate::tokens::load(&t) {
            Some(Err(e)) => {
                for i in &e.errors {
                    rep.push(
                        "error",
                        "tokens",
                        tr!("theme “{theme}”: {error}", theme = t.id, error = i),
                        None,
                        false,
                    );
                }
            }
            Some(Ok(l)) => {
                for i in &l.warnings {
                    rep.push(
                        "info",
                        "tokens",
                        tr!("theme “{theme}”: {error}", theme = t.id, error = i),
                        None,
                        false,
                    );
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
                wanted.push((
                    kind,
                    v,
                    tr!("theme {theme} ({module})", theme = src, module = m),
                ));
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
            let msg = match kind {
                "gtk" => tr!(
                    "GTK theme “{name}” not found (from: {from})",
                    name = name,
                    from = from
                ),
                "icons" => tr!(
                    "icon theme “{name}” not found (from: {from})",
                    name = name,
                    from = from
                ),
                "cursor" => tr!(
                    "cursor theme “{name}” not found (from: {from})",
                    name = name,
                    from = from
                ),
                "sound" => tr!(
                    "sound theme “{name}” not found (from: {from})",
                    name = name,
                    from = from
                ),
                _ => tr!(
                    "shell theme “{name}” not found (from: {from})",
                    name = name,
                    from = from
                ),
            };
            rep.push("warning", "missing-asset", msg, None, false);
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
                tr!("the wallpaper file does not exist: {path}", path = p),
                None,
                false,
            );
        }
    }
}

/// Generated files of the active themes went missing (e.g. ~/.cache was cleaned): `--fix`
/// regenerates them (the links point at stable paths, so they work again).
/// What the doctor says about deskd, from the facts: `(severity, message)`; `None` = all well.
fn deskd_verdict(
    owned: bool,
    activatable: bool,
    running_version: Option<&str>,
    ours: &str,
) -> Option<(&'static str, String)> {
    match (owned, activatable, running_version) {
        (false, false, _) => Some((
            "warning",
            tr!("deskd is not installed: its D-Bus service file is missing (run data/install.sh)"),
        )),
        // D-Bus activation starts it on first use, so "not running" is normal after login
        (false, true, _) => Some((
            "info",
            tr!(
                "deskd is not running (it starts when a feature such as the clipboard is first used)"
            ),
        )),
        (true, _, None) => Some((
            "warning",
            tr!(
                "deskd owns its D-Bus name but does not answer; restart it: systemctl --user restart deskd"
            ),
        )),
        (true, _, Some(v)) if v != ours => Some((
            "warning",
            tr!(
                "deskd {running} is running but this program is {ours}; restart it: systemctl --user restart deskd",
                running = v,
                ours = ours
            ),
        )),
        (true, _, Some(_)) => None,
    }
}

/// deskd: installed (activatable), running (D-Bus name owned) and the same version as this program.
/// Without a session bus there is nothing to check.
fn check_deskd(rep: &mut DoctorReport) {
    use desk_proto::{BUS_NAME, DeskProxy};
    let Some(conn) = crate::shell::session() else {
        return;
    };
    let facts = zbus::block_on(async {
        let dbus = zbus::fdo::DBusProxy::new(&conn).await.ok()?;
        let owned = dbus.name_has_owner(BUS_NAME.try_into().ok()?).await.ok()?;
        let activatable = dbus
            .list_activatable_names()
            .await
            .ok()?
            .iter()
            .any(|n| n.as_str() == BUS_NAME);
        let version = if owned {
            match DeskProxy::new(&conn).await {
                Ok(p) => p.version().await.ok(),
                Err(_) => None,
            }
        } else {
            None
        };
        Some((owned, activatable, version))
    });
    let Some((owned, activatable, version)) = facts else {
        return;
    };
    if let Some((severity, message)) = deskd_verdict(
        owned,
        activatable,
        version.as_deref(),
        env!("CARGO_PKG_VERSION"),
    ) {
        rep.push(severity, "deskd", message, None, false);
    }
}

// ------------------------------------------------------------------ GSettings schemas

const SCHEMAS_COMPILED: &str = "gschemas.compiled";

/// What is wrong with a schema directory that holds `*.gschema.xml` files.
#[derive(Debug, PartialEq, Eq)]
enum SchemaProblem {
    /// no `gschemas.compiled` (the case of a real install: the extension cannot load its settings)
    Missing,
    /// a `.gschema.xml` is newer than the compiled file
    Older,
    /// the compiled file does not know this schema id
    Lacks(String),
}

fn schema_xmls(dir: &Path) -> impl Iterator<Item = std::fs::DirEntry> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".gschema.xml"))
}

/// `None` when the directory has no schema sources or its compiled file is complete and current.
fn schema_problem(dir: &Path, ids: &[&str]) -> Option<SchemaProblem> {
    let newest_xml = schema_xmls(dir)
        .map(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
        .max()?; // no .gschema.xml at all: nothing to compile
    let compiled = dir.join(SCHEMAS_COMPILED);
    let Ok(meta) = std::fs::metadata(&compiled) else {
        return Some(SchemaProblem::Missing);
    };
    if let (Some(x), Ok(c)) = (newest_xml, meta.modified())
        && x > c
    {
        return Some(SchemaProblem::Older);
    }
    // gvdb keeps schema ids as plain strings, so a byte search tells whether a schema is in there
    let bytes = std::fs::read(&compiled).unwrap_or_default();
    ids.iter()
        .find(|id| !bytes.windows(id.len()).any(|w| w == id.as_bytes()))
        .map(|id| SchemaProblem::Lacks((*id).to_string()))
}

fn compile_schemas(dir: &Path) -> bool {
    run("glib-compile-schemas", &[&dir.to_string_lossy()], None).is_ok_and(|o| o.ok)
}

/// Report a schema directory problem, or compile it with `--fix` (only the compiled file is
/// written, never a setting).
fn report_schema_problem(
    dir: &Path,
    problem: &SchemaProblem,
    (what, severity, uuid): (String, &'static str, Option<String>),
    fix: bool,
    compile: &dyn Fn(&Path) -> bool,
    rep: &mut DoctorReport,
) {
    if fix && compile(dir) {
        rep.fixed
            .push(tr!("compiled the GSettings schemas of {what}", what = what));
        return;
    }
    let why = match problem {
        SchemaProblem::Missing => tr!("gschemas.compiled is missing"),
        SchemaProblem::Older => tr!("gschemas.compiled is older than the schema files"),
        SchemaProblem::Lacks(id) => tr!("gschemas.compiled does not contain {schema}", schema = id),
    };
    rep.push(
        severity,
        "schemas",
        tr!(
            "{what}: {why} (themes doctor --fix runs glib-compile-schemas)",
            what = what,
            why = why
        ),
        uuid,
        true,
    );
}

/// Our GSettings schemas (`io.github.OlehDzhumyk.Desk*`) are installed and compiled, and every
/// installed extension that ships `schemas/*.gschema.xml` has `schemas/gschemas.compiled` next to
/// them (without it the extension fails to enable). `data_dirs` are the system data directories.
fn check_schemas_in(
    user_data: &Path,
    data_dirs: &[PathBuf],
    extensions: &Path,
    enabled: &[String],
    fix: bool,
    compile: &dyn Fn(&Path) -> bool,
    rep: &mut DoctorReport,
) {
    // the schemas of deskd, the bridge and the window
    let file = format!("{}.gschema.xml", desk_settings::DESK_SCHEMA);
    let dir = user_data.join("glib-2.0/schemas");
    if dir.join(&file).is_file() {
        let ids: Vec<&str> = desk_settings::SCHEMAS.iter().map(|s| s.id).collect();
        if let Some(p) = schema_problem(&dir, &ids) {
            let what = "desktop-themes-rs".to_string();
            report_schema_problem(&dir, &p, (what, "warning", None), fix, compile, rep);
        }
    } else if !data_dirs
        .iter()
        .any(|d| d.join("glib-2.0/schemas").join(&file).is_file())
    {
        rep.push(
            "warning",
            "schemas",
            tr!(
                "the GSettings schema {schema} is not installed (run data/install.sh)",
                schema = desk_settings::DESK_SCHEMA
            ),
            None,
            false,
        );
    }

    // extensions with a schema of their own
    let mut exts: Vec<PathBuf> = std::fs::read_dir(extensions)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    exts.sort();
    for ext in exts {
        let sdir = ext.join("schemas");
        let Some(problem) = schema_problem(&sdir, &[]) else {
            continue;
        };
        let uuid = ext
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let severity = if enabled.contains(&uuid) {
            "error"
        } else {
            "warning"
        };
        let what = tr!("the extension {uuid}", uuid = uuid);
        report_schema_problem(
            &sdir,
            &problem,
            (what, severity, Some(uuid.clone())),
            fix,
            compile,
            rep,
        );
    }
}

fn check_schemas(ctx: &Ctx, enabled: &[String], fix: bool, rep: &mut DoctorReport) {
    let data_dirs: Vec<PathBuf> = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
        .split(':')
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect();
    check_schemas_in(
        &ctx.home.join(".local/share"),
        &data_dirs,
        &ctx.user_extensions_dir(),
        enabled,
        fix,
        &compile_schemas,
        rep,
    );
}

fn check_token_cache(ctx: &Ctx, st: &State, fix: bool, rep: &mut DoctorReport) {
    let mut seen = BTreeSet::new();
    for m in [Module::Look, Module::Apps] {
        let Some(src) = st.source(m) else { continue };
        if !seen.insert(src.to_string()) {
            continue;
        }
        let t = Theme::new(ctx, src);
        let Ok(files) = crate::tokens::plan(&t) else {
            continue;
        };
        if files.iter().all(|f| f.status == "cached") {
            continue;
        }
        if fix && crate::tokens::materialize(&t, false).is_ok() {
            rep.fixed.push(tr!(
                "generated the token files of the theme “{theme}”",
                theme = src
            ));
        } else {
            rep.push(
                "warning",
                "tokens-cache",
                tr!(
                    "the files generated from tokens.toml of the theme “{theme}” are missing or out of date (themes doctor --fix)",
                    theme = src
                ),
                None,
                true,
            );
        }
    }
    // generated GTK files are copies: bring them up to date with the cache
    if fix
        && !st.gtk_copies.is_empty()
        && let Ok(_lock) = Lock::acquire(ctx)
    {
        let mut s2 = State::load(ctx);
        let mut w = Vec::new();
        let done = crate::gtklinks::refresh_copies(&mut s2, &mut w);
        if !done.is_empty() && s2.save(ctx).is_ok() {
            rep.fixed
                .extend(done.into_iter().map(|d| tr!("updated {path}", path = d)));
        }
        for m in w {
            rep.push("warning", "tokens-cache", m, None, false);
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
                tr!("journal: {line}", line = short),
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
            tr!(
                "the GNOME Shell version changed: {old} → {new} (after a system update)",
                old = prev,
                new = cur
            ),
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
                    tr!("out of date (OUT_OF_DATE) for this GNOME version")
                } else {
                    tr!("error (ERROR)")
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
                tr!("{uuid}: enabled but not installed", uuid = u),
                Some(u.clone()),
                false,
            ),
            (Some(_), None) => rep.push(
                "info",
                "extension",
                tr!(
                    "{uuid}: installed, but the shell has not loaded it yet (log in again)",
                    uuid = u
                ),
                Some(u.clone()),
                false,
            ),
            (None, _) => {
                if !ver.is_empty() && metadata_supports(ctx, u, &ver) == Some(false) {
                    rep.push(
                        "error",
                        "extension",
                        tr!(
                            "{uuid}: metadata.json does not support GNOME {version}",
                            uuid = u,
                            version = major(&ver)
                        ),
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
                tr!("extension {uuid} (the desktop bridge: “Theme” in quick settings, clipboard) is not installed", uuid = p),
                None,
                false,
            );
        } else if !enabled.iter().any(|u| u == p) && !st.is_broken(p) {
            rep.push(
                "warning",
                "permanent",
                tr!("extension {uuid} is disabled", uuid = p),
                Some(p.to_string()),
                true,
            );
        }
    }
    check_journal(&enabled, &mut rep);
    check_crashes(ctx, &mut rep);
    check_assets(ctx, &st, &mut rep);
    check_token_cache(ctx, &st, opt.fix, &mut rep);
    check_schemas(ctx, &enabled, opt.fix, &mut rep);
    check_deskd(&mut rep);
    for b in &st.broken_extensions {
        rep.broken_recorded.push(b.uuid.clone());
        rep.push(
            "info",
            "broken-recorded",
            tr!(
                "{uuid} turned off {since} ({reason})",
                uuid = b.uuid,
                since = b.since,
                reason = b.reason
            ),
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
            rep.fixed.extend(
                to_disable
                    .iter()
                    .map(|(u, _)| tr!("turned off {uuid}", uuid = u)),
            );
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
                rep.fixed.push(tr!("turned on {uuid}", uuid = p));
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
            body.push(tr!(
                "GNOME turned extensions off after a shell crash. Open Themes to turn them on again."
            ));
        }
        if rep.version_changed {
            body.push(tr!(
                "GNOME updated: {old} → {new}",
                old = rep.previous_gnome_version.clone().unwrap_or_default(),
                new = rep.gnome_version.clone().unwrap_or_default()
            ));
        }
        body.extend(rep.fixed.iter().cloned());
        body.extend(
            rep.restored
                .iter()
                .map(|u| tr!("turned on again: {uuid}", uuid = u)),
        );
        let _ = crate::shell::notify(
            &tr!("Themes: system check"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deskd_verdicts() {
        let sev = |o, a, v: Option<&str>| deskd_verdict(o, a, v, "1.2.3").map(|x| x.0);
        assert_eq!(sev(false, false, None), Some("warning")); // not installed
        assert_eq!(sev(false, true, None), Some("info")); // starts on demand: not a problem
        assert_eq!(sev(true, true, None), Some("warning")); // owns the name, silent
        assert_eq!(sev(true, true, Some("1.2.2")), Some("warning")); // older than this program
        assert_eq!(sev(true, false, Some("1.2.3")), None);
        assert_eq!(sev(true, true, Some("1.2.3")), None);
    }

    fn tmp(tag: &str) -> PathBuf {
        crate::theme::tests::tmp_root(&format!("doctor-schemas-{tag}"))
    }

    fn touch(p: &Path, s: &str) {
        crate::theme::tests::write(p, s);
    }

    /// Stand-in for glib-compile-schemas: writes a compiled file that names the schemas it saw.
    fn fake_compile(dir: &Path) -> bool {
        let mut names = String::new();
        for e in schema_xmls(dir) {
            names.push_str(&std::fs::read_to_string(e.path()).unwrap());
        }
        std::fs::write(dir.join(SCHEMAS_COMPILED), names).unwrap();
        true
    }

    fn run_check(root: &Path, enabled: &[&str], fix: bool) -> DoctorReport {
        let mut rep = DoctorReport::default();
        let enabled: Vec<String> = enabled.iter().map(|s| s.to_string()).collect();
        check_schemas_in(
            &root.join("data"),
            &[root.join("sys")],
            &root.join("data/gnome-shell/extensions"),
            &enabled,
            fix,
            &fake_compile,
            &mut rep,
        );
        rep
    }

    fn desk_xml() -> String {
        desk_settings::SCHEMAS
            .iter()
            .map(|s| format!("<schema id=\"{}\"/>", s.id))
            .collect()
    }

    #[test]
    fn schema_problem_cases() {
        let d = tmp("problem");
        assert_eq!(schema_problem(&d, &[]), None, "no xml, nothing to compile");
        touch(&d.join("a.gschema.xml"), "<schema id=\"x.A\"/>");
        assert_eq!(schema_problem(&d, &[]), Some(SchemaProblem::Missing));
        touch(&d.join(SCHEMAS_COMPILED), "x.A");
        std::fs::File::options()
            .write(true)
            .open(d.join(SCHEMAS_COMPILED))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        assert_eq!(schema_problem(&d, &["x.A"]), None);
        assert_eq!(
            schema_problem(&d, &["x.A", "x.B"]),
            Some(SchemaProblem::Lacks("x.B".into()))
        );
        std::fs::File::options()
            .write(true)
            .open(d.join(SCHEMAS_COMPILED))
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(schema_problem(&d, &["x.A"]), Some(SchemaProblem::Older));
    }

    #[test]
    fn schemas_ok_and_not_installed() {
        let r = tmp("ok");
        // nothing installed anywhere: one non-fixable warning
        let rep = run_check(&r, &[], false);
        assert_eq!(rep.issues.len(), 1);
        assert!(!rep.issues[0].fixable && rep.issues[0].kind == "schemas");
        // installed by the distribution (system dir): fine
        touch(
            &r.join("sys/glib-2.0/schemas/io.github.OlehDzhumyk.Desk.gschema.xml"),
            "",
        );
        assert!(run_check(&r, &[], false).issues.is_empty());
    }

    /// The failure of a real install: an extension with schemas/*.gschema.xml but no
    /// schemas/gschemas.compiled. An enabled extension is an error, `--fix` compiles it.
    #[test]
    fn extension_without_compiled_schemas() {
        let r = tmp("ext");
        let ext = r.join("data/gnome-shell/extensions/e@x/schemas");
        touch(&ext.join("e.gschema.xml"), "<schema id=\"e\"/>");
        touch(
            &r.join("data/gnome-shell/extensions/plain@x/metadata.json"),
            "{}",
        );
        let dskd = r.join("data/glib-2.0/schemas");
        touch(
            &dskd.join("io.github.OlehDzhumyk.Desk.gschema.xml"),
            &desk_xml(),
        );
        fake_compile(&dskd);

        let rep = run_check(&r, &["e@x"], false);
        assert_eq!(rep.issues.len(), 1, "{:?}", rep.issues);
        let i = &rep.issues[0];
        assert_eq!((i.severity, i.kind, i.fixable), ("error", "schemas", true));
        assert_eq!(i.uuid.as_deref(), Some("e@x"));
        assert_eq!(run_check(&r, &[], false).issues[0].severity, "warning");

        let rep = run_check(&r, &["e@x"], true);
        assert!(rep.issues.is_empty() && rep.fixed.len() == 1, "{rep:?}");
        assert!(ext.join(SCHEMAS_COMPILED).is_file());
        assert!(run_check(&r, &["e@x"], false).issues.is_empty());
    }

    #[test]
    fn own_schema_compiled_stale_is_fixed() {
        let r = tmp("own");
        let dir = r.join("data/glib-2.0/schemas");
        touch(
            &dir.join("io.github.OlehDzhumyk.Desk.gschema.xml"),
            &desk_xml(),
        );
        // compiled before a schema was added: lacks the newest id
        touch(&dir.join(SCHEMAS_COMPILED), "io.github.OlehDzhumyk.Desk");
        let rep = run_check(&r, &[], false);
        assert_eq!(rep.issues.len(), 1, "{:?}", rep.issues);
        assert!(rep.issues[0].fixable);
        let rep = run_check(&r, &[], true);
        assert!(rep.issues.is_empty() && !rep.fixed.is_empty());
        assert!(run_check(&r, &[], false).issues.is_empty());
    }
}
