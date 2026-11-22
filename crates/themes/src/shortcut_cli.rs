// SPDX-License-Identifier: GPL-3.0-or-later
//! `themes shortcut list|set|reset`: the per-feature shortcuts of the settings registry.
//! The theme only provides the default; `set` is the user's override, `reset` goes back to the
//! theme default (rules and storage: `themes_core::shortcuts`).

use clap::Subcommand;
use desk_settings::shortcuts::DISABLED;
use desk_settings::{n_, tr};
use serde_json::json;
use themes_core::shortcuts::{self, describe, source_name};
use themes_core::{Ctx, Error, dconf};

// Help texts (gettext ids; cli.rs translates them at start-up).
const S01: &str = n_("List the shortcuts of the features and where each comes from");
const S02: &str = n_("Set your own shortcut (it wins over the theme and survives theme switches)");
const S03: &str = n_("Shortcut id, see `themes shortcut list`");
const S04: &str = n_("Accelerators such as <Super>v; “disabled” turns the shortcut off");
const S05: &str = n_("Forget your shortcut and go back to the theme default");

#[derive(Subcommand)]
pub enum ShortcutCmd {
    #[command(about = S01)]
    List,
    #[command(about = S02)]
    Set {
        #[arg(value_name = "ID", help = S03)]
        id: String,
        #[arg(value_name = "ACCEL", required = true, num_args = 1.., help = S04)]
        accels: Vec<String>,
    },
    #[command(about = S05)]
    Reset {
        #[arg(value_name = "ID", help = S03)]
        id: String,
    },
}

fn show(a: &[String]) -> String {
    if a.is_empty() {
        tr!("none")
    } else {
        a.join(", ")
    }
}

pub fn exec(ctx: &Ctx, cmd: ShortcutCmd, json: bool) -> Result<i32, Error> {
    match cmd {
        ShortcutCmd::List => {
            let rows = shortcuts::status(ctx, &dconf::dump()?);
            if json {
                let v: Vec<_> = rows
                    .iter()
                    .map(|r| {
                        json!({
                            "id": r.id, "in_force": r.in_force,
                            "source": format!("{:?}", r.resolved.source).to_lowercase(),
                            "theme_default": r.theme_default, "override": r.user_override,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                for r in &rows {
                    println!(
                        "{:<18} {:<24} ({})",
                        r.id,
                        show(&r.in_force),
                        source_name(r.resolved.source)
                    );
                    if !r.user_override.is_empty() {
                        println!(
                            "  {}",
                            tr!("theme default: {accels}", accels = show(&r.theme_default))
                        );
                    }
                }
            }
            Ok(0)
        }
        ShortcutCmd::Set { id, accels } => {
            let conflicts = shortcuts::set_override(ctx, &id, &accels)?;
            if conflicts.is_empty() {
                if json {
                    println!("{}", json!({"ok": true, "id": id, "override": accels}));
                } else if accels.iter().all(|a| a == DISABLED) {
                    println!("{}", tr!("Shortcut {id} turned off", id = id));
                } else {
                    println!("{}", tr!("Shortcut {id}: {accels}", id = id, accels = show(&accels)));
                }
                return Ok(0);
            }
            let text: Vec<String> = conflicts.iter().map(describe).collect();
            if json {
                println!("{}", json!({"ok": false, "conflicts": text}));
            } else {
                eprintln!("{}", tr!("Refused: the shortcut is taken"));
                for t in &text {
                    eprintln!("  {t}");
                }
            }
            Ok(1)
        }
        ShortcutCmd::Reset { id } => {
            let conflicts = shortcuts::reset(ctx, &id)?;
            if json {
                println!("{}", json!({"ok": true, "id": id}));
            } else {
                println!("{}", tr!("Shortcut {id}: back to the theme default", id = id));
                for c in &conflicts {
                    println!("  {}", tr!("warning: {text}", text = describe(c)));
                }
            }
            Ok(0)
        }
    }
}
