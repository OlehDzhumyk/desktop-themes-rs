// SPDX-License-Identifier: GPL-3.0-or-later
//! Застосування теми (повністю або окремих модулів), живе оновлення оболонки, автовідкат.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::Serialize;

use crate::dconf::{self, Settings};
use crate::error::Result;
use crate::lock::Lock;
use crate::modules::Module;
use crate::paths::Ctx;
use crate::plan::{self, DconfInput, ExtInput};
use crate::shell::{ExtState, Shell};
use crate::state::{BrokenExtension, State};
use crate::theme::{self, Theme, all_themes, get_theme};
use crate::util::{log, now};
use crate::{DIS_KEY, EXT_KEY, PERMANENT_EXTENSIONS, SHELL_THEME_KEY, gtklinks, hooks};

#[derive(Debug, Default, Serialize, Clone)]
pub struct ApplyResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    pub theme: String,
    pub name: String,
    /// Модулі, які застосовано цього разу.
    pub applied_modules: Vec<Module>,
    /// Повний вибір після застосування.
    pub modules: BTreeMap<Module, String>,
    pub previous: Option<String>,
    pub previous_modules: BTreeMap<Module, String>,
    pub writes: Settings,
    pub resets: Vec<String>,
    pub changed_keys: Vec<String>,
    pub resets_effective: usize,
    pub enabled_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
    pub extensions_changed: bool,
    pub extensions_added: Vec<String>,
    pub extensions_removed: Vec<String>,
    pub shell_theme_changed: bool,
    pub gtk_links: Vec<String>,
    pub flatpak_lines: usize,
    pub hooks: Vec<String>,
    pub shell_running: bool,
    /// Зміни оболонки застосовано наживо (без перевходу).
    pub live: bool,
    /// GNOME вимкнув користувацькі розширення (захист після збою): розширення теми не працюють.
    pub extensions_disabled_by_gnome: bool,
    pub relogin: bool,
    pub relogin_reasons: Vec<String>,
    pub warnings: Vec<String>,
    /// Розширення, які автовідкат вимкнув (заповнює verify_live).
    pub rolled_back: Vec<String>,
    /// Ключі, виправлені після того, як розширення відпрацювали.
    pub cleanup: Vec<String>,
    /// Files generated from tokens.toml (dry run: what would be generated).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<crate::tokens::GeneratedFile>,
    /// Symlinks to generated files (shell theme, Ptyxis palette).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub token_links: Vec<String>,
    /// Time spent on token generation (cache check included), milliseconds.
    pub tokens_ms: f64,
}

/// Чинний список розширень: значення з dconf або типове схеми.
pub fn ext_lists(cur: &Settings) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let default_en = dconf::schema_default("org.gnome.shell", "enabled-extensions")
        .map(|s| dconf::parse_strv(&s))
        .unwrap_or_default();
    let default_dis = dconf::schema_default("org.gnome.shell", "disabled-extensions")
        .map(|s| dconf::parse_strv(&s))
        .unwrap_or_default();
    let cur_en = cur
        .get(EXT_KEY)
        .map(|s| dconf::parse_strv(s))
        .unwrap_or_else(|| default_en.clone());
    let cur_dis = cur
        .get(DIS_KEY)
        .map(|s| dconf::parse_strv(s))
        .unwrap_or_else(|| default_dis.clone());
    (cur_en, cur_dis, default_en, default_dis)
}

enum ExtAction {
    Write(String),
    Reset,
}

fn ext_action(key: &str, new: &[String], baseline: &Settings, default: &[String]) -> ExtAction {
    match baseline.get(key) {
        Some(b) if dconf::parse_strv(b) == new => ExtAction::Write(b.clone()),
        None if new == default => ExtAction::Reset,
        _ => ExtAction::Write(dconf::fmt_strv(new)),
    }
}

fn installed(ctx: &Ctx, uuid: &str) -> bool {
    ctx.extension_path(uuid).is_some()
}

/// Застосувати тему `id` до модулів `modules` (порожньо = усі).
pub fn apply(ctx: &Ctx, id: &str, modules: &[Module], dry_run: bool) -> Result<ApplyResult> {
    apply_opts(ctx, id, modules, dry_run, true)
}

/// `purge_ext_state`: при повному скиданні (fedora для всіх модулів) прибрати власні
/// налаштування розширень тем, яких немає в baseline.
pub fn apply_opts(
    ctx: &Ctx,
    id: &str,
    modules: &[Module],
    dry_run: bool,
    purge_ext_state: bool,
) -> Result<ApplyResult> {
    let target = get_theme(ctx, id)?;
    let affected: Vec<Module> = if modules.is_empty() {
        Module::ALL.to_vec()
    } else {
        modules.to_vec()
    };
    crate::util::ensure_bus();
    let _lock = Lock::acquire(ctx)?;
    let baseline = dconf::parse_file(&ctx.baseline_dir()?.join("full.dconf"))?;
    let mut st = State::load(ctx);
    let themes = all_themes(ctx);
    let cur = dconf::dump()?;
    let mut warnings = Vec::new();

    // новий вибір джерел
    let prev_sel = st.modules.clone();
    let mut sel = prev_sel.clone();
    for m in &affected {
        sel.insert(*m, id.to_string());
    }
    // модулі без джерела (новий state) — як fedora: нічого не задають
    let theme_of = |tid: &str| -> Option<Theme> { themes.iter().find(|t| t.id == tid).cloned() };
    let mut targets: BTreeMap<Module, Settings> = BTreeMap::new();
    for m in Module::ALL {
        let src = sel.get(&m).cloned();
        let s = match src.as_deref().map(|s| (s, theme_of(s))) {
            Some((_, Some(t))) => t.settings(m),
            Some((sid, None)) => {
                warnings.push(format!(
                    "тему «{sid}» (модуль {m}) не знайдено — її ключі повертаються до стандартних"
                ));
                Settings::new()
            }
            None => Settings::new(),
        };
        targets.insert(m, s);
    }
    let mut theme_keys: BTreeMap<Module, BTreeSet<String>> = BTreeMap::new();
    for t in &themes {
        for m in Module::ALL {
            theme_keys
                .entry(m)
                .or_default()
                .extend(t.settings(m).into_keys());
        }
    }
    let dplan = plan::compute_dconf(&DconfInput {
        baseline: &baseline,
        current: &cur,
        prev_keys: &st.module_keys,
        theme_keys: &theme_keys,
        targets: &targets,
        affected: &affected,
    });

    // розширення (лише «вигляд» задає extensions.txt; постійні й «зламані» враховуються завжди)
    let look_theme = sel.get(&Module::Look).and_then(|s| theme_of(s));
    let target_exts = look_theme
        .as_ref()
        .map(Theme::extensions)
        .unwrap_or_default();
    let (cur_en, cur_dis, default_en, default_dis) = ext_lists(&cur);
    let base_en = baseline
        .get(EXT_KEY)
        .map(|s| dconf::parse_strv(s))
        .unwrap_or_else(|| default_en.clone());
    let mut theme_exts: BTreeSet<String> = st.applied_extensions.iter().cloned().collect();
    for t in &themes {
        theme_exts.extend(t.extensions());
    }
    // розширення, які замінив міст desk-bridge, прибираються з будь-якого набору
    theme_exts.extend(crate::RETIRED_EXTENSIONS.iter().map(|s| s.to_string()));
    let broken: BTreeSet<String> = st
        .broken_extensions
        .iter()
        .map(|b| b.uuid.clone())
        .collect();
    let (new_en, new_dis) = plan::compute_extensions(&ExtInput {
        base_en: &base_en,
        cur_en: &cur_en,
        cur_dis: &cur_dis,
        theme_exts: &theme_exts,
        target: &target_exts,
        permanent: PERMANENT_EXTENSIONS,
        broken: &broken,
    });
    let en_set: BTreeSet<&String> = new_en.iter().collect();
    let cur_set: BTreeSet<&String> = cur_en.iter().collect();
    let added: Vec<String> = new_en
        .iter()
        .filter(|u| !cur_set.contains(u))
        .cloned()
        .collect();
    let removed: Vec<String> = cur_en
        .iter()
        .filter(|u| !en_set.contains(u))
        .cloned()
        .collect();
    let ext_changed = !added.is_empty() || !removed.is_empty();

    let desired_shell_theme = dplan
        .desired
        .get(SHELL_THEME_KEY)
        .cloned()
        .unwrap_or_else(|| cur.get(SHELL_THEME_KEY).cloned());
    let shell_theme_changed = desired_shell_theme != cur.get(SHELL_THEME_KEY).cloned();

    let mut changed_keys: Vec<String> = dplan.writes.keys().cloned().collect();
    changed_keys.extend(dplan.resets.iter().cloned());
    changed_keys.sort();

    // themes whose tokens feed the affected modules: invalid tokens stop the apply here
    let mut token_themes: Vec<Theme> = Vec::new();
    for m in &affected {
        if let Some(t) = sel.get(m).and_then(|s| theme_of(s))
            && crate::tokens::has_tokens(&t)
            && !token_themes.iter().any(|x| x.id == t.id)
        {
            crate::tokens::check(&t)?;
            token_themes.push(t);
        }
    }

    let look = affected.contains(&Module::Look);
    let mut res = ApplyResult {
        ok: true,
        dry_run,
        theme: id.into(),
        name: target.name(),
        applied_modules: affected.clone(),
        modules: sel.clone(),
        previous: st.current.clone(),
        previous_modules: prev_sel.clone(),
        writes: dplan.writes.clone(),
        resets: dplan.resets.clone(),
        changed_keys,
        resets_effective: dplan.resets.len(),
        enabled_extensions: new_en.clone(),
        disabled_extensions: new_dis.clone(),
        extensions_changed: ext_changed,
        extensions_added: added.clone(),
        extensions_removed: removed.clone(),
        shell_theme_changed,
        ..Default::default()
    };
    if dry_run {
        if look {
            res.gtk_links = theme::GTK_KINDS
                .iter()
                .flat_map(|k| {
                    target
                        .gtk_entries(k)
                        .into_iter()
                        .map(move |e| format!("{k}/{e}"))
                })
                .collect();
            res.flatpak_lines = target.flatpak_lines().len();
        }
        for t in &token_themes {
            res.generated.extend(crate::tokens::plan(t)?);
        }
        for m in [Module::Look, Module::Apps] {
            if let Some(t) = affected
                .contains(&m)
                .then(|| sel.get(&m).and_then(|s| theme_of(s)))
                .flatten()
            {
                res.token_links.extend(
                    crate::tokens::links_for(ctx, &t, m)
                        .into_iter()
                        .map(|(d, _)| d.to_string_lossy().into_owned()),
                );
            }
        }
        res.shell_running = Shell::connect().is_some();
        return Ok(res);
    }

    // 0. files generated from tokens (content-hashed cache) and their links; before dconf, so
    // the shell theme exists when user-theme reads its new name
    let t0 = std::time::Instant::now();
    for t in &token_themes {
        res.generated
            .extend(crate::tokens::materialize(t, false)?.0);
    }
    for m in [Module::Look, Module::Apps] {
        if !affected.contains(&m) {
            continue;
        }
        crate::tokens::unlink_module(&mut st, m, &mut warnings);
        if let Some(t) = sel.get(&m).and_then(|s| theme_of(s)) {
            res.token_links.extend(crate::tokens::link_module(
                ctx,
                &mut st,
                m,
                &t,
                &mut warnings,
            )?);
        }
    }
    res.tokens_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // 1. dconf
    dconf::reset(&dplan.resets);
    dconf::load(&dplan.writes)?;
    // 2. розширення. Спершу даємо оболонці обробити зміни налаштувань: якщо сповіщення про зміну
    // org.gnome.desktop.interface надійде в момент вимкнення розширення з годинником, GNOME Shell 51.1
    // падає (SIGSEGV у gnome-desktop update_clock); до того ж нові розширення одразу читають свої ключі.
    let live_shell = Shell::connect();
    if live_shell.is_some() && ext_changed && !(dplan.writes.is_empty() && dplan.resets.is_empty())
    {
        std::thread::sleep(Duration::from_millis(1200));
    }
    for (key, new, def) in [
        (EXT_KEY, &new_en, &default_en),
        (DIS_KEY, &new_dis, &default_dis),
    ] {
        match ext_action(key, new, &baseline, def) {
            ExtAction::Reset => dconf::reset(&[key.to_string()]),
            ExtAction::Write(v) => {
                if cur.get(key) != Some(&v) {
                    dconf::write(key, &v)?
                }
            }
        }
    }
    for u in &target_exts {
        if !installed(ctx, u) {
            warnings.push(format!("розширення {u} не встановлене"));
        }
        if broken.contains(u) {
            warnings.push(format!(
                "розширення {u} вимкнено як несправне (див. «themes doctor»)"
            ));
        }
    }
    // 3–4. GTK і flatpak — частина «вигляду»
    if look {
        gtklinks::unlink_all(&mut st, &mut warnings)?;
        res.gtk_links = gtklinks::link_theme(ctx, &mut st, &target)?;
        crate::flatpak::remove(&mut st, &mut warnings)?;
        res.flatpak_lines = crate::flatpak::apply(ctx, &mut st, &target, &mut warnings)?;
        st.applied_extensions = target.extensions();
    }
    // 5. хуки програм
    if affected.contains(&Module::Apps) {
        let new_src = id;
        let prev_hook = st.hooks_applied.clone();
        if let Some(prev) = prev_hook.as_deref().filter(|p| *p != new_src) {
            if let Some(pt) = theme_of(prev) {
                match hooks::run_hook(ctx, &pt, "revert", Some(prev), Some(new_src)) {
                    Some(Ok(())) => res.hooks.push(format!("{prev}/revert")),
                    Some(Err(e)) => warnings.push(e),
                    None => {}
                }
            }
            st.hooks_applied = None;
        }
        let prev_apps = prev_sel.get(&Module::Apps).map(String::as_str);
        match hooks::run_hook(ctx, &target, "apply", prev_apps, None) {
            Some(Ok(())) => {
                res.hooks.push(format!("{new_src}/apply"));
                st.hooks_applied = Some(new_src.to_string());
            }
            Some(Err(e)) => warnings.push(e),
            None => {}
        }
    }
    // 6. стан
    for m in &affected {
        st.module_keys.insert(
            *m,
            targets
                .get(m)
                .map(|t| t.keys().cloned().collect())
                .unwrap_or_default(),
        );
    }
    st.modules = sel.clone();
    st.save(ctx)?;

    // 7. живе оновлення оболонки
    if let Some(shell) = live_shell {
        res.shell_running = true;
        live_check(
            ctx,
            &shell,
            &new_en,
            &added,
            &removed,
            &mut res,
            &mut warnings,
        );
        // повторне застосування тієї самої теми оболонки: перечитати її файли
        let has_shell_theme = desired_shell_theme.as_deref().is_some_and(|v| v != "''");
        if look
            && !shell_theme_changed
            && has_shell_theme
            && !added.iter().any(|u| u.starts_with("user-theme@"))
        {
            crate::shell::reload_user_theme(&shell, "");
        }
    }
    // 8. прибирання після того, як розширення відпрацювали (вони інколи пишуть ключі при вимкненні)
    let full_reset = id == crate::BUILTIN_THEME && affected.len() == Module::ALL.len();
    if res.shell_running && ext_changed {
        std::thread::sleep(Duration::from_millis(1500));
    }
    let purge: Vec<String> = if full_reset && purge_ext_state {
        theme_exts
            .iter()
            .filter(|u| !new_en.contains(u))
            .cloned()
            .collect()
    } else {
        vec![]
    };
    let mut protected: BTreeSet<String> = theme_keys.values().flatten().cloned().collect();
    protected.extend(targets.values().flat_map(|t| t.keys().cloned()));
    res.cleanup = cleanup(ctx, &dplan.desired, &baseline, &protected, &purge);
    res.warnings = warnings;
    log(
        ctx,
        &format!(
            "apply {id} modules={} (було: {}) keys_changed={} resets={} ext={} gtk_links={} flatpak={} tokens={}/{:.1}ms live={} relogin={}{}",
            affected
                .iter()
                .map(|m| m.id())
                .collect::<Vec<_>>()
                .join(","),
            fmt_sel(&prev_sel),
            res.changed_keys.len(),
            res.resets_effective,
            if new_en.is_empty() {
                "-".into()
            } else {
                new_en.join(",")
            },
            res.gtk_links.len(),
            res.flatpak_lines,
            res.generated.len(),
            res.tokens_ms,
            res.live,
            res.relogin,
            if res.warnings.is_empty() {
                String::new()
            } else {
                format!(" warn={}", res.warnings.join("; "))
            }
        ),
    );
    Ok(res)
}

pub fn fmt_sel(sel: &BTreeMap<Module, String>) -> String {
    if sel.is_empty() {
        return "-".into();
    }
    let first = sel.values().next().cloned().unwrap_or_default();
    if sel.len() == 4 && sel.values().all(|v| *v == first) {
        return first;
    }
    sel.iter()
        .map(|(m, t)| format!("{m}={t}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Розширення, після перемикання яких оболонку варто перезапустити: dash-to-dock і
/// dash-to-panel обидва переробляють панель/док; заповнюється за результатами тестів.
pub const SWAP_NEEDS_RELOGIN: &[(&str, &str)] = &[];

/// Перевіряє, чи оболонка підхопила зміни розширень наживо; вирішує, чи потрібен перевхід.
fn live_check(
    ctx: &Ctx,
    shell: &Shell,
    new_en: &[String],
    added: &[String],
    removed: &[String],
    res: &mut ApplyResult,
    warnings: &mut Vec<String>,
) {
    let mut reasons = Vec::new();
    if shell.user_extensions_enabled() == Some(false) || crate::crash::user_extensions_disabled() {
        res.extensions_disabled_by_gnome = true;
        warnings.push(
            "GNOME вимкнув розширення після збою оболонки (disable-user-extensions=true): розширення \
             теми не працюватимуть, доки ви їх не ввімкнете (themes doctor --enable-extensions або \
             кнопка у вікні «Теми»)"
                .into(),
        );
    } else {
        let locked = shell.screen_locked();
        if locked && !added.is_empty() {
            warnings
                .push("сеанс заблоковано — нові розширення ввімкнуться після розблокування".into());
        }
        let on: Vec<String> = if locked {
            vec![]
        } else {
            new_en
                .iter()
                .filter(|u| installed(ctx, u))
                .cloned()
                .collect()
        };
        let mut infos = shell.settle_targets(&on, removed, Duration::from_secs(12));
        if locked {
            for u in new_en.iter().filter(|u| installed(ctx, u)) {
                infos.insert(u.clone(), shell.ext_info(u));
            }
        }
        for u in new_en.iter().filter(|u| installed(ctx, u)) {
            match infos.get(u).cloned().flatten() {
                None => reasons.push(format!(
                    "оболонка ще не знає про розширення {u} (встановлене після входу)"
                )),
                Some(i) if i.state == ExtState::Active => {}
                Some(i) if i.state.broken() => {
                    warnings.push(format!("розширення {u}: {} {}", i.state.name(), i.error));
                }
                Some(i)
                    if locked && matches!(i.state, ExtState::Inactive | ExtState::Initialized) => {}
                Some(i) if added.contains(u) => reasons.push(format!(
                    "розширення {u} не ввімкнулося наживо (стан {})",
                    i.state.name()
                )),
                Some(_) => {}
            }
        }
        for u in removed {
            if let Some(i) = infos.get(u).cloned().flatten()
                && i.state == ExtState::Active
            {
                reasons.push(format!("розширення {u} не вимкнулося наживо"));
            }
        }
        for (a, b) in SWAP_NEEDS_RELOGIN {
            let swap = (added.iter().any(|u| u == a) && removed.iter().any(|u| u == b))
                || (added.iter().any(|u| u == b) && removed.iter().any(|u| u == a));
            if swap {
                reasons.push(format!("заміна {b} ↔ {a} наживо лишає артефакти панелі"));
            }
        }
    }
    res.live = reasons.is_empty() && !res.extensions_disabled_by_gnome;
    res.relogin = !reasons.is_empty();
    res.relogin_reasons = reasons;
}

/// Автовідкат: через ~5 с після живого застосування вимикає розширення, що перейшли в ERROR.
pub fn verify_live(ctx: &Ctx, wait: Duration) -> Result<Vec<(String, String)>> {
    std::thread::sleep(wait);
    let Some(shell) = Shell::connect() else {
        return Ok(vec![]);
    };
    let cur = dconf::dump()?;
    let (cur_en, ..) = ext_lists(&cur);
    let mut bad = Vec::new();
    for u in &cur_en {
        if let Some(i) = shell.ext_info(u)
            && i.state == ExtState::Error
        {
            bad.push((
                u.clone(),
                if i.error.is_empty() {
                    "ERROR".into()
                } else {
                    i.error
                },
            ));
        }
    }
    if !bad.is_empty() {
        disable_broken(ctx, &shell, &bad, "автовідкат після застосування")?;
    }
    Ok(bad)
}

/// Вимикає розширення і записує їх у state.broken_extensions (щоб повернути після виправлення).
pub fn disable_broken(ctx: &Ctx, shell: &Shell, bad: &[(String, String)], why: &str) -> Result<()> {
    let _lock = Lock::acquire(ctx)?;
    let mut st = State::load(ctx);
    let ver = shell.version().unwrap_or_default();
    let cur = dconf::dump()?;
    let (cur_en, ..) = ext_lists(&cur);
    let uuids: BTreeSet<&String> = bad.iter().map(|(u, _)| u).collect();
    let new_en: Vec<String> = cur_en
        .iter()
        .filter(|u| !uuids.contains(u))
        .cloned()
        .collect();
    dconf::write(EXT_KEY, &dconf::fmt_strv(&new_en))?;
    for (u, reason) in bad {
        if !st.is_broken(u) {
            st.broken_extensions.push(BrokenExtension {
                uuid: u.clone(),
                reason: format!("{why}: {reason}"),
                since: now("%Y-%m-%d %H:%M:%S"),
                shell_version: ver.clone(),
                stamp: ext_stamp(ctx, u),
            });
        }
    }
    st.save(ctx)?;
    log(
        ctx,
        &format!(
            "disable-broken {} ({why})",
            bad.iter()
                .map(|b| b.0.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ),
    );
    Ok(())
}

/// mtime і версія metadata.json — змінюються, коли розширення оновлюють.
pub fn ext_stamp(ctx: &Ctx, uuid: &str) -> String {
    let Some(dir) = ctx.extension_path(uuid) else {
        return String::new();
    };
    let meta = dir.join("metadata.json");
    let mtime = std::fs::metadata(&meta)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ver = std::fs::read_to_string(&meta)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("version").map(|x| x.to_string()))
        .unwrap_or_default();
    format!("{mtime}:{ver}")
}

/// Видалити тему (у trash/). fedora й активні теми видаляти не можна.
pub fn delete(ctx: &Ctx, id: &str) -> Result<(String, String)> {
    if id == crate::BUILTIN_THEME {
        crate::bail!("тему «fedora» видаляти не можна — це стандартний вигляд");
    }
    let t = get_theme(ctx, id)?;
    let _lock = Lock::acquire(ctx)?;
    let st = State::load(ctx);
    let used: Vec<&str> = Module::ALL
        .iter()
        .filter(|m| st.source(**m) == Some(id))
        .map(|m| m.id())
        .collect();
    if !used.is_empty() {
        crate::bail!(
            "тема «{id}» зараз активна (модулі: {}) — спершу застосуйте іншу (themes reset)",
            used.join(", ")
        );
    }
    std::fs::create_dir_all(&ctx.trash)?;
    let dst = ctx.trash.join(format!("{id}-{}", now("%Y%m%d-%H%M%S")));
    crate::util::move_path(&t.dir, &dst)?;
    log(ctx, &format!("delete {id} -> {}", dst.display()));
    Ok((t.name(), dst.to_string_lossy().into_owned()))
}

/// Шлях dconf (каталог+ключ) → (схема, ключ) для всіх нерелокованих схем системи.
fn schema_index() -> BTreeMap<String, (gio::SettingsSchema, String)> {
    let mut m = BTreeMap::new();
    let Some(src) = gio::SettingsSchemaSource::default() else {
        return m;
    };
    let (fixed, _) = src.list_schemas(true);
    for id in fixed {
        let Some(sch) = src.lookup(&id, true) else {
            continue;
        };
        let Some(path) = sch.path() else { continue };
        for k in sch.list_keys() {
            m.insert(format!("{path}{k}"), (sch.clone(), k.to_string()));
        }
    }
    m
}

/// Каталоги dconf розширення (атрибути path="…" у schemas/*.gschema.xml).
fn extension_dconf_dirs(ctx: &Ctx, uuid: &str) -> Vec<String> {
    let Some(dir) = ctx.extension_path(uuid) else {
        return vec![];
    };
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir.join("schemas")) else {
        return out;
    };
    for e in rd.flatten() {
        if !e.file_name().to_string_lossy().ends_with(".gschema.xml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        for part in text.split("path=\"").skip(1) {
            if let Some(p) = part.split('"').next()
                && p.starts_with("/org/gnome/shell/extensions/")
                && p.ends_with('/')
                && !out.contains(&p.to_string())
            {
                out.push(p.to_string());
            }
        }
    }
    out
}

/// Після застосування: (1) повернути керовані ключі, які змінили розширення;
/// (2) скинути ключі поза baseline, значення яких дорівнює типовому (на поведінку не впливає);
/// (3) при повному скиданні — прибрати власні налаштування вимкнених розширень тем.
fn cleanup(
    ctx: &Ctx,
    desired: &BTreeMap<String, Option<String>>,
    baseline: &Settings,
    protected: &BTreeSet<String>,
    purge: &[String],
) -> Vec<String> {
    let mut fixed = Vec::new();
    let Ok(cur) = dconf::dump() else { return fixed };
    for (k, d) in desired {
        match d {
            Some(v) if cur.get(k) != Some(v) => {
                if dconf::write(k, v).is_ok() {
                    fixed.push(k.clone());
                }
            }
            None if cur.contains_key(k) => {
                dconf::reset(std::slice::from_ref(k));
                fixed.push(k.clone());
            }
            _ => {}
        }
    }
    let idx = schema_index();
    for (k, v) in &cur {
        if baseline.contains_key(k)
            || desired.contains_key(k)
            || protected.contains(k)
            || k == EXT_KEY
            || k == DIS_KEY
        {
            continue;
        }
        let Some((sch, key)) = idx.get(k) else {
            continue;
        };
        let sk = sch.key(key);
        let def = sk.default_value();
        let Ok(val) = gio::glib::Variant::parse(Some(&sk.value_type()), v) else {
            continue;
        };
        if val == def {
            dconf::reset(std::slice::from_ref(k));
            fixed.push(k.clone());
        }
    }
    for uuid in purge {
        for dir in extension_dconf_dirs(ctx, uuid) {
            let in_baseline = baseline.keys().any(|k| k.starts_with(&dir));
            if !in_baseline && cur.keys().any(|k| k.starts_with(&dir)) {
                let _ = crate::util::run("dconf", &["reset", "-f", &dir], None);
                fixed.push(dir);
            }
        }
    }
    fixed
}
