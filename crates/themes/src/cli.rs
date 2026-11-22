// SPDX-License-Identifier: GPL-3.0-or-later
//! Командний рядок: themes list|current|apply|reset|snapshot|delete|doctor|gui.

use std::time::Duration;

use clap::{Parser, Subcommand};
use serde_json::json;
use themes_core::apply::{ApplyResult, apply_opts, delete, verify_live};
use themes_core::doctor::{DoctorOptions, doctor};
use themes_core::modules::Module;
use themes_core::state::State;
use themes_core::theme::all_themes;
use themes_core::{Ctx, Error};

#[derive(Parser)]
#[command(
    name = "themes",
    version,
    about = "Перемикач тем робочого столу GNOME (модулі: look, behavior, sounds, apps)",
    after_help = "Без аргументів відкриває програму «Теми». Параметр --json — машинозчитуваний вивід.\nКорінь: ~/.local/share/desktop-themes (DESKTOP_THEMES_ROOT)"
)]
struct Cli {
    /// Машинозчитуваний вивід (JSON)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Список тем (* — активна для «вигляду»)
    List,
    /// Звідки взято кожен модуль
    Current,
    /// Застосувати тему (усі модулі або вибрані)
    Apply {
        id: String,
        /// Лише ці модулі: look,behavior,sounds,apps
        #[arg(long, value_name = "МОДУЛІ")]
        only: Option<String>,
        /// Один модуль (можна повторювати)
        #[arg(long, value_name = "МОДУЛЬ")]
        module: Vec<String>,
        /// Показати, що зміниться, нічого не змінюючи
        #[arg(long)]
        dry_run: bool,
        /// Не чекати 5 с на перевірку розширень після живого застосування
        #[arg(long)]
        no_verify: bool,
    },
    /// Повернути стандартний вигляд (= apply fedora)
    Reset {
        #[arg(long, value_name = "МОДУЛІ")]
        only: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_verify: bool,
        /// Не прибирати власні налаштування розширень тем при повному скиданні
        #[arg(long)]
        keep_extension_settings: bool,
    },
    /// Зберегти поточний вигляд як нову тему
    Snapshot {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Видалити тему (у trash/); fedora й активні — ні
    Delete { id: String },
    /// Перевірити розширення, журнал, теми/значки/звуки, версію GNOME
    Doctor {
        /// Вимкнути зламані розширення (і повернути полагоджені), надіслати сповіщення
        #[arg(long)]
        fix: bool,
        /// Чекати до N секунд на запуск GNOME Shell (для запуску під час входу)
        #[arg(long, value_name = "СЕК", default_value_t = 0)]
        wait_shell: u64,
        /// Не надсилати сповіщення
        #[arg(long)]
        no_notify: bool,
        /// Знову ввімкнути розширення, які GNOME вимкнув після збою оболонки
        /// (disable-user-extensions). Робіть це, лише прибравши причину збою.
        #[arg(long)]
        enable_extensions: bool,
    },
    /// Дизайн-токени (tokens.toml): перевірка, генерація, тема з шпалер
    Tokens {
        #[command(subcommand)]
        cmd: crate::tokens_cli::TokensCmd,
    },
    /// Відкрити програму «Теми»
    Gui {
        /// Зберегти знімок вікна у PNG і вийти
        #[arg(long)]
        screenshot: Option<String>,
        #[arg(long, value_name = "dark|light")]
        color_scheme: Option<String>,
        /// Розмір вікна, напр. 1180x780
        #[arg(long)]
        size: Option<String>,
        /// Сторінка: themes | mix | features | clipboard | credits
        #[arg(long)]
        page: Option<String>,
    },
}

fn out_json(v: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

pub fn run(args: Vec<String>) -> i32 {
    let json_flag = args.iter().any(|a| a == "--json");
    // сумісність зі старим `theme` без аргументів: довідка
    let cli = match Cli::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) => {
            if json_flag
                && !matches!(
                    e.kind(),
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                )
            {
                out_json(&json!({"ok": false, "error": e.to_string().trim()}));
                return 1;
            }
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };
    themes_core::util::ensure_bus();
    let ctx = Ctx::from_env();
    match exec(&ctx, cli.cmd, cli.json) {
        Ok(code) => code,
        Err(e) => {
            if cli.json {
                out_json(&json!({"ok": false, "error": e.0}));
            } else {
                eprintln!("Помилка: {}", e.0);
            }
            1
        }
    }
}

fn modules_arg(only: Option<&str>, module: &[String]) -> Result<Vec<Module>, Error> {
    let mut s = only.unwrap_or("").to_string();
    for m in module {
        s.push(',');
        s.push_str(m);
    }
    Module::parse_list(&s)
}

fn exec(ctx: &Ctx, cmd: Cmd, json: bool) -> Result<i32, Error> {
    match cmd {
        Cmd::Tokens { cmd } => crate::tokens_cli::exec(ctx, cmd, json),
        Cmd::List => {
            let st = State::load(ctx);
            let look = st.source(Module::Look).map(String::from);
            let ts = all_themes(ctx);
            if json {
                let v: Vec<_> = ts
                    .iter()
                    .map(|t| {
                        let active_modules: Vec<Module> = Module::ALL
                            .into_iter()
                            .filter(|m| st.source(*m) == Some(t.id.as_str()))
                            .collect();
                        json!({
                            "id": t.id, "name": t.name(), "description": t.description(),
                            "preview": t.preview(), "preview_dark": t.preview_dark(),
                            "active": look.as_deref() == Some(t.id.as_str()),
                            "active_modules": active_modules,
                            "modules": t.modules(),
                            "builtin": t.builtin(), "extensions": t.extensions(),
                            "path": t.dir,
                        })
                    })
                    .collect();
                out_json(&v);
            } else {
                for t in &ts {
                    let act: Vec<&str> = Module::ALL
                        .iter()
                        .filter(|m| st.source(**m) == Some(t.id.as_str()))
                        .map(|m| m.id())
                        .collect();
                    let mods: Vec<&str> = t.modules().iter().map(|m| m.id()).collect();
                    println!(
                        "{} {:<14} {:<14} [{}]{}  {}",
                        if look.as_deref() == Some(t.id.as_str()) {
                            "*"
                        } else {
                            " "
                        },
                        t.id,
                        t.name(),
                        mods.join(","),
                        if act.is_empty() {
                            String::new()
                        } else {
                            format!(" активні: {}", act.join(","))
                        },
                        t.description()
                    );
                }
            }
            Ok(0)
        }
        Cmd::Current => {
            let st = State::load(ctx);
            if json {
                out_json(
                    &json!({"current": st.source(Module::Look), "modules": st.modules, "mixed": st.uniform().is_none() && !st.modules.is_empty()}),
                );
            } else if st.modules.is_empty() {
                println!("(невідомо — тему ще не застосовували)");
            } else {
                for m in Module::ALL {
                    println!(
                        "{:<10} {:<9} {}",
                        format!("{}:", m.label()),
                        m.id(),
                        st.source(m).unwrap_or("-")
                    );
                }
                if let Some(u) = st.uniform() {
                    println!("(усі модулі з теми «{u}»)");
                }
            }
            Ok(0)
        }
        Cmd::Apply {
            id,
            only,
            module,
            dry_run,
            no_verify,
        } => {
            let mods = modules_arg(only.as_deref(), &module)?;
            do_apply(ctx, &id, &mods, dry_run, no_verify, true, json)
        }
        Cmd::Reset {
            only,
            dry_run,
            no_verify,
            keep_extension_settings,
        } => {
            let mods = modules_arg(only.as_deref(), &[])?;
            do_apply(
                ctx,
                themes_core::BUILTIN_THEME,
                &mods,
                dry_run,
                no_verify,
                !keep_extension_settings,
                json,
            )
        }
        Cmd::Snapshot { id, name, force } => {
            let r = themes_core::snapshot::snapshot(ctx, &id, name.as_deref(), force)?;
            if json {
                out_json(&r);
            } else {
                let per: Vec<String> = r
                    .module_keys
                    .iter()
                    .map(|(m, n)| format!("{m}: {n}"))
                    .collect();
                println!(
                    "Збережено тему «{}» ({}): ключів dconf {} ({}), розширень {}\n  {}",
                    r.name,
                    r.theme,
                    r.keys,
                    per.join(", "),
                    r.extensions.len(),
                    r.path
                );
                for w in &r.warnings {
                    println!("  увага: {w}");
                }
            }
            Ok(0)
        }
        Cmd::Delete { id } => {
            let (name, moved) = delete(ctx, &id)?;
            if json {
                out_json(&json!({"ok": true, "theme": id, "name": name, "moved_to": moved}));
            } else {
                println!("Тему «{name}» видалено (переміщено до {moved})");
            }
            Ok(0)
        }
        Cmd::Doctor {
            fix,
            wait_shell,
            no_notify,
            enable_extensions,
        } => {
            if enable_extensions {
                if themes_core::crash::user_extensions_disabled() {
                    themes_core::crash::enable_user_extensions()?;
                    println!("Розширення знову ввімкнено (disable-user-extensions=false).");
                } else {
                    println!("Розширення й так увімкнені.");
                }
            }
            let r = doctor(
                ctx,
                &DoctorOptions {
                    fix,
                    wait_shell: Duration::from_secs(wait_shell),
                    notify: !no_notify,
                },
            )?;
            if json {
                out_json(&r);
            } else {
                println!(
                    "GNOME Shell: {} ({})",
                    r.gnome_version.clone().unwrap_or("?".into()),
                    if r.shell_running {
                        "запущено"
                    } else {
                        "не запущено"
                    }
                );
                println!("Перевірено розширень: {}", r.extensions_checked);
                if r.issues.is_empty() {
                    println!("Проблем не знайдено.");
                }
                for i in &r.issues {
                    let mark = match i.severity {
                        "error" => "✗",
                        "warning" => "!",
                        _ => "i",
                    };
                    println!(
                        "  {mark} {}{}",
                        i.message,
                        if i.fixable && !fix {
                            "  [виправить: themes doctor --fix]"
                        } else {
                            ""
                        }
                    );
                }
                for f in &r.fixed {
                    println!("  виправлено: {f}");
                }
                for u in &r.restored {
                    println!("  знову увімкнено: {u}");
                }
            }
            Ok(if r.ok { 0 } else { 3 })
        }
        Cmd::Gui {
            screenshot,
            color_scheme,
            size,
            page,
        } => {
            let mut o = crate::gui::GuiOptions {
                screenshot,
                color_scheme,
                page,
                ..Default::default()
            };
            if let Some(s) = size {
                if let Some((w, h)) = s.split_once('x') {
                    o.size = (w.parse().unwrap_or(1180), h.parse().unwrap_or(780));
                }
            }
            Ok(crate::gui::run(o))
        }
    }
}

fn do_apply(
    ctx: &Ctx,
    id: &str,
    mods: &[Module],
    dry_run: bool,
    no_verify: bool,
    purge: bool,
    json: bool,
) -> Result<i32, Error> {
    let mut r = apply_opts(ctx, id, mods, dry_run, purge)?;
    if !dry_run && !no_verify && r.shell_running && r.extensions_changed {
        if let Ok(bad) = verify_live(ctx, Duration::from_secs(5)) {
            for (u, why) in &bad {
                r.warnings
                    .push(format!("автовідкат: розширення {u} вимкнено ({why})"));
            }
            r.rolled_back = bad.into_iter().map(|b| b.0).collect();
        }
    }
    if json {
        out_json(&r);
    } else {
        print_apply(&r);
    }
    Ok(0)
}

fn print_apply(r: &ApplyResult) {
    let mods: Vec<&str> = r.applied_modules.iter().map(|m| m.id()).collect();
    if r.dry_run {
        println!(
            "Пробний запуск для «{}» ({}), модулі: {} — нічого не змінено",
            r.name,
            r.theme,
            mods.join(",")
        );
        println!("Записати ({}):", r.writes.len());
        for (k, v) in &r.writes {
            println!("  {k} = {v}");
        }
        println!(
            "Скинути ({}): {}",
            r.resets.len(),
            if r.resets.is_empty() {
                "-".into()
            } else {
                r.resets.join(", ")
            }
        );
        println!("Розширення: {}", r.enabled_extensions.join(", "));
        if r.extensions_changed {
            println!(
                "  + {}   − {}",
                r.extensions_added.join(", "),
                r.extensions_removed.join(", ")
            );
        }
        println!(
            "GTK: {}   flatpak: {}",
            if r.gtk_links.is_empty() {
                "-".into()
            } else {
                r.gtk_links.join(", ")
            },
            r.flatpak_lines
        );
        print_generated(r);
        return;
    }
    println!(
        "Тему «{}» ({}) застосовано; модулі: {}.",
        r.name,
        r.theme,
        mods.join(", ")
    );
    println!(
        "  попередньо:            {}",
        themes_core::apply::fmt_sel(&r.previous_modules)
    );
    println!(
        "  тепер:                 {}",
        themes_core::apply::fmt_sel(&r.modules)
    );
    println!(
        "  змінено ключів dconf:  {} (з них скинуто до типових: {})",
        r.changed_keys.len(),
        r.resets_effective
    );
    println!(
        "  розширення:            {}",
        r.enabled_extensions.join(", ")
    );
    if r.extensions_changed {
        println!(
            "    увімкнено: {}   вимкнено: {}",
            or_dash(&r.extensions_added),
            or_dash(&r.extensions_removed)
        );
    }
    if !r.gtk_links.is_empty() {
        println!("  посилання GTK:         {}", r.gtk_links.join(", "));
    }
    if r.flatpak_lines > 0 {
        println!("  flatpak overrides:     {}", r.flatpak_lines);
    }
    print_generated(r);
    if !r.cleanup.is_empty() {
        println!("  прибрано після розширень: {}", r.cleanup.len());
    }
    if !r.hooks.is_empty() {
        println!("  хуки:                  {}", r.hooks.join(", "));
    }
    for w in &r.warnings {
        println!("  увага: {w}");
    }
    if !r.shell_running {
        println!(
            "GNOME Shell зараз не запущено — зміни оболонки набудуть чинності при наступному вході."
        );
    } else if r.extensions_disabled_by_gnome {
        println!(
            "Налаштування застосовано, але GNOME вимкнув розширення після збою оболонки — розширення \
             теми не працюють. Перевірте «themes doctor» і ввімкніть їх: themes doctor --enable-extensions"
        );
    } else if r.relogin {
        println!("ПОТРІБНО вийти з сесії та увійти знову:");
        for why in &r.relogin_reasons {
            println!("  • {why}");
        }
    } else {
        println!(
            "Застосовано наживо, перевхід не потрібен (відкриті програми GTK4/libadwaita варто перезапустити)."
        );
    }
}

/// Files generated from tokens.toml and their links.
fn print_generated(r: &ApplyResult) {
    if r.generated.is_empty() {
        return;
    }
    if r.dry_run {
        println!("Згенеровано з tokens.toml ({}):", r.generated.len());
    } else {
        println!(
            "  з tokens.toml ({}, {:.1} мс):",
            r.generated.len(),
            r.tokens_ms
        );
    }
    for f in &r.generated {
        let st = match f.status {
            "cached" => "кеш актуальний",
            "written" => "згенеровано",
            _ => "буде згенеровано",
        };
        println!("    {} ({st})", f.path);
    }
    for l in &r.token_links {
        println!("    посилання: {l}");
    }
}

fn or_dash(v: &[String]) -> String {
    if v.is_empty() {
        "-".into()
    } else {
        v.join(", ")
    }
}
