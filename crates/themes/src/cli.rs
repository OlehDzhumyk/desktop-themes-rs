// SPDX-License-Identifier: GPL-3.0-or-later
//! Командний рядок: themes list|current|apply|reset|snapshot|delete|doctor|gui.

use std::time::Duration;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use desk_settings::i18n::gettext;
use desk_settings::{n_, tr};
use serde_json::json;
use themes_core::apply::{ApplyResult, apply_opts, delete, verify_live};
use themes_core::doctor::{DoctorOptions, doctor};
use themes_core::modules::Module;
use themes_core::state::State;
use themes_core::theme::all_themes;
use themes_core::{Ctx, Error};

// Тексти довідки (ідентифікатори gettext): атрибути clap не бачить xgettext, тож вони тут.
const H01: &str = n_("GNOME desktop theme switcher (modules: look, behavior, sounds, apps)");
const H02: &str = n_(
    "Without arguments it opens the Themes window. --json gives machine-readable output.\nRoot: ~/.local/share/desktop-themes (DESKTOP_THEMES_ROOT)",
);
const H03: &str = n_("Machine-readable output (JSON)");
const H04: &str = n_("List the themes (* = active for the look)");
const H05: &str = n_("Where every module comes from");
const H06: &str = n_("Apply a theme (all modules or the chosen ones)");
const H07: &str = n_("MODULES");
const H08: &str = n_("Only these modules: look,behavior,sounds,apps");
const H09: &str = n_("MODULE");
const H10: &str = n_("One module (can be repeated)");
const H11: &str = n_("Show what would change without changing anything");
const H12: &str = n_("Do not wait 5 s to check the extensions after a live apply");
const H13: &str = n_("Go back to the standard look (= apply fedora)");
const H14: &str = n_("Keep the settings of the themes' own extensions on a full reset");
const H15: &str = n_("Save the current look as a new theme");
const H16: &str = n_("Delete a theme (into trash/); not fedora and not active ones");
const H17: &str = n_("Check extensions, the journal, themes/icons/sounds, the GNOME version");
const H18: &str = n_("Turn broken extensions off (and repaired ones back on), send a notification");
const H19: &str = n_("SEC");
const H20: &str = n_("Wait up to N seconds for GNOME Shell to start (for running at login)");
const H21: &str = n_("Do not send notifications");
const H22: &str = n_(
    "Turn the extensions back on that GNOME turned off after a shell crash (disable-user-extensions). Do it only after removing the cause of the crash.",
);
const H23: &str = n_("Open the Themes window");
const H24: &str = n_("Save a screenshot of the window as PNG and exit");
const H25: &str = n_("Window size, e.g. 1180x780");
const H27: &str = n_("Confirm applying an experimental theme (for scripts)");
const H28: &str = n_("Design tokens (tokens.toml): check, generate, a theme from a wallpaper");
const H26: &str = n_("Page: themes | mix | features | clipboard | general | credits");

#[derive(Parser)]
#[command(
    name = "themes",
    version,
    about = H01,
    after_help = H02
)]
struct Cli {
    #[arg(long, global = true, help = H03)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = H04)]
    List,
    #[command(about = H05)]
    Current,
    #[command(about = H06)]
    Apply {
        id: String,
        #[arg(long, value_name = H07, help = H08)]
        only: Option<String>,
        #[arg(long, value_name = H09, help = H10)]
        module: Vec<String>,
        #[arg(long, help = H11)]
        dry_run: bool,
        #[arg(long, help = H12)]
        no_verify: bool,
        #[arg(long, short = 'y', help = H27)]
        yes: bool,
    },
    #[command(about = H13)]
    Reset {
        #[arg(long, value_name = H07)]
        only: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_verify: bool,
        #[arg(
            long,
            help = H14
        )]
        keep_extension_settings: bool,
    },
    #[command(about = H15)]
    Snapshot {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        force: bool,
    },
    #[command(about = H16)]
    Delete { id: String },
    #[command(about = H17)]
    Doctor {
        #[arg(
            long,
            help = H18
        )]
        fix: bool,
        #[arg(
            long,
            value_name = H19,
            default_value_t = 0,
            help = H20
        )]
        wait_shell: u64,
        #[arg(long, help = H21)]
        no_notify: bool,
        #[arg(
            long,
            help = H22
        )]
        enable_extensions: bool,
    },
    #[command(about = H28)]
    Tokens {
        #[command(subcommand)]
        cmd: crate::tokens_cli::TokensCmd,
    },
    #[command(about = H23)]
    Gui {
        #[arg(long, help = H24)]
        screenshot: Option<String>,
        #[arg(long, value_name = "dark|light")]
        color_scheme: Option<String>,
        #[arg(long, help = H25)]
        size: Option<String>,
        #[arg(
            long,
            help = H26
        )]
        page: Option<String>,
    },
}

/// Перекласти тексти довідки clap (вони записані англійською як ідентифікатори gettext).
fn translate_help(cmd: clap::Command) -> clap::Command {
    let t = |s: &clap::builder::StyledStr| gettext(s.to_string());
    let mut cmd = cmd.mut_args(|a| {
        let mut a = a;
        if let Some(h) = a.get_help() {
            let h = t(h);
            a = a.help(h);
        }
        if let Some(names) = a.get_value_names() {
            // clap хоче &'static str; рядків кілька, витік один раз за запуск
            let names: Vec<&'static str> = names
                .iter()
                .map(|n| &*Box::leak(gettext(n.as_str()).into_boxed_str()))
                .collect();
            a = a.value_names(names);
        }
        a
    });
    if let Some(s) = cmd.get_about() {
        let s = t(s);
        cmd = cmd.about(s);
    }
    if let Some(s) = cmd.get_after_help() {
        let s = t(s);
        cmd = cmd.after_help(s);
    }
    cmd.mut_subcommands(translate_help)
}

fn out_json(v: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

pub fn run(args: Vec<String>) -> i32 {
    let json_flag = args.iter().any(|a| a == "--json");
    // сумісність зі старим `theme` без аргументів: довідка
    let parsed = translate_help(Cli::command())
        .try_get_matches_from(&args)
        .and_then(|m| Cli::from_arg_matches(&m));
    let cli = match parsed {
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
                eprintln!("{}", tr!("Error: {error}", error = e.0));
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
                            "experimental": t.experimental(),
                            "warning": if t.experimental() { t.warning() } else { String::new() },
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
                            format!(" {}", tr!("active: {modules}", modules = act.join(",")))
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
                println!("{}", tr!("(unknown: no theme has been applied yet)"));
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
                    println!(
                        "{}",
                        tr!("(all modules from the theme “{theme}”)", theme = u)
                    );
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
            yes,
        } => {
            let mods = modules_arg(only.as_deref(), &module)?;
            if !dry_run && !yes {
                confirm_experimental(ctx, &id, json)?;
            }
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
                    "{}\n  {}",
                    tr!(
                        "Saved the theme “{name}” ({id}): dconf keys {keys} ({per_module}), extensions {extensions}",
                        name = r.name,
                        id = r.theme,
                        keys = r.keys,
                        per_module = per.join(", "),
                        extensions = r.extensions.len()
                    ),
                    r.path
                );
                for w in &r.warnings {
                    println!("  {}", tr!("warning: {text}", text = w));
                }
            }
            Ok(0)
        }
        Cmd::Delete { id } => {
            let (name, moved) = delete(ctx, &id)?;
            if json {
                out_json(&json!({"ok": true, "theme": id, "name": name, "moved_to": moved}));
            } else {
                println!(
                    "{}",
                    tr!(
                        "Theme “{name}” deleted (moved to {path})",
                        name = name,
                        path = moved
                    )
                );
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
                    println!(
                        "{}",
                        tr!("Extensions turned on again (disable-user-extensions=false).")
                    );
                } else {
                    println!("{}", tr!("Extensions are already on."));
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
                        tr!("running")
                    } else {
                        tr!("not running")
                    }
                );
                println!(
                    "{}",
                    tr!("Extensions checked: {count}", count = r.extensions_checked)
                );
                if r.issues.is_empty() {
                    println!("{}", tr!("No problems found."));
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
                            format!("  [{}]", tr!("fix with: themes doctor --fix"))
                        } else {
                            String::new()
                        }
                    );
                }
                for f in &r.fixed {
                    println!("  {}", tr!("fixed: {what}", what = f));
                }
                for u in &r.restored {
                    println!("  {}", tr!("turned on again: {uuid}", uuid = u));
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

/// Експериментальна тема: спитати y/N у терміналі; без терміналу (скрипти, --json) — відмова, якщо
/// не передано --yes.
fn confirm_experimental(ctx: &Ctx, id: &str, json: bool) -> Result<(), Error> {
    use std::io::{BufRead, IsTerminal, Write};
    let t = themes_core::theme::get_theme(ctx, id)?;
    if !t.experimental() {
        return Ok(());
    }
    let interactive = !json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if !interactive {
        return Err(Error(tr!(
            "“{name}” is an experimental theme: {warning}\nRun it again with --yes to apply it anyway.",
            name = t.name(),
            warning = t.warning()
        )));
    }
    eprintln!(
        "{}",
        tr!("“{name}” is an experimental theme.", name = t.name())
    );
    eprintln!("{}", t.warning());
    // TRANSLATORS: the answer is accepted as y/Y/yes or the translated "yes" below
    eprint!("{} ", tr!("Apply it anyway? [y/N]"));
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let a = line.trim().to_lowercase();
    // TRANSLATORS: short and long "yes" accepted at the [y/N] prompt, separated by "|"
    let yes_words = tr!("y|yes");
    if a == "y" || a == "yes" || yes_words.split('|').any(|w| !w.is_empty() && a == w) {
        Ok(())
    } else {
        Err(Error(tr!("Cancelled: the theme was not applied.")))
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
                r.warnings.push(tr!(
                    "automatic rollback: extension {uuid} turned off ({reason})",
                    uuid = u,
                    reason = why
                ));
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
            "{}",
            tr!(
                "Dry run for “{name}” ({id}), modules: {modules}; nothing changed",
                name = r.name,
                id = r.theme,
                modules = mods.join(",")
            )
        );
        println!("{}", tr!("Write ({count}):", count = r.writes.len()));
        for (k, v) in &r.writes {
            println!("  {k} = {v}");
        }
        println!(
            "{} {}",
            tr!("Reset ({count}):", count = r.resets.len()),
            if r.resets.is_empty() {
                "-".into()
            } else {
                r.resets.join(", ")
            }
        );
        println!(
            "{}",
            tr!("Extensions: {list}", list = r.enabled_extensions.join(", "))
        );
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
        "{}",
        tr!(
            "Theme “{name}” ({id}) applied; modules: {modules}.",
            name = r.name,
            id = r.theme,
            modules = mods.join(", ")
        )
    );
    // підписи вирівнюються за найдовшим (довжина залежить від мови)
    let mut rows: Vec<(String, String)> = vec![
        (
            tr!("before:"),
            themes_core::apply::fmt_sel(&r.previous_modules),
        ),
        (tr!("now:"), themes_core::apply::fmt_sel(&r.modules)),
        (
            tr!("dconf keys changed:"),
            tr!(
                "{count} (reset to defaults: {resets})",
                count = r.changed_keys.len(),
                resets = r.resets_effective
            ),
        ),
        (tr!("extensions:"), r.enabled_extensions.join(", ")),
    ];
    if r.extensions_changed {
        rows.push((
            String::new(),
            tr!(
                "turned on: {added}   turned off: {removed}",
                added = or_dash(&r.extensions_added),
                removed = or_dash(&r.extensions_removed)
            ),
        ));
    }
    if !r.gtk_links.is_empty() {
        rows.push((tr!("GTK links:"), r.gtk_links.join(", ")));
    }
    if r.flatpak_lines > 0 {
        rows.push(("flatpak overrides:".into(), r.flatpak_lines.to_string()));
    }
    if !r.cleanup.is_empty() {
        rows.push((
            tr!("cleaned up after extensions:"),
            r.cleanup.len().to_string(),
        ));
    }
    if !r.hooks.is_empty() {
        rows.push((tr!("hooks:"), r.hooks.join(", ")));
    }
    let width = rows
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0);
    for (l, v) in rows {
        let pad = width.saturating_sub(l.chars().count());
        println!("  {l}{}  {v}", " ".repeat(pad));
    }
    print_generated(r);
    for w in &r.warnings {
        println!("  {}", tr!("warning: {text}", text = w));
    }
    if !r.shell_running {
        println!(
            "{}",
            tr!("GNOME Shell is not running now; the shell changes take effect at the next login.")
        );
    } else if r.extensions_disabled_by_gnome {
        println!(
            "{}",
            tr!(
                "The settings are applied, but GNOME turned extensions off after a shell crash, so \
                 the theme's extensions do not work. Check “themes doctor” and turn them on: themes \
                 doctor --enable-extensions"
            )
        );
    } else if r.relogin {
        println!("{}", tr!("You MUST log out and log in again:"));
        for why in &r.relogin_reasons {
            println!("  • {why}");
        }
    } else {
        println!(
            "{}",
            tr!(
                "Applied live, no new login needed (open GTK4/libadwaita apps should be restarted)."
            )
        );
    }
}

/// Files generated from tokens.toml and their links.
fn print_generated(r: &ApplyResult) {
    if r.generated.is_empty() {
        return;
    }
    if r.dry_run {
        println!(
            "{}",
            tr!(
                "Generated from tokens.toml ({count}):",
                count = r.generated.len()
            )
        );
    } else {
        println!(
            "  {}",
            tr!(
                "from tokens.toml ({count}, {ms} ms):",
                count = r.generated.len(),
                ms = format!("{:.1}", r.tokens_ms)
            )
        );
    }
    for f in &r.generated {
        let st = match f.status {
            "cached" => tr!("cache up to date"),
            "written" => tr!("generated"),
            _ => tr!("will be generated"),
        };
        println!("    {} ({st})", f.path);
    }
    for l in &r.token_links {
        println!("    {}", tr!("link: {path}", path = l));
    }
}

fn or_dash(v: &[String]) -> String {
    if v.is_empty() {
        "-".into()
    } else {
        v.join(", ")
    }
}
