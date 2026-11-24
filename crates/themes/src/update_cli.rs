// SPDX-License-Identifier: GPL-3.0-or-later
//! `themes update [--check] [--pause] [--dry-run]`: update a git-based install from the checkout
//! that `data/install.sh` recorded (`themes_core::update`), by running its `scripts/update.sh`.
//! The launcher "Update Desktop Themes" (`data/io.github.OlehDzhumyk.Themes.Update.desktop.in`)
//! runs `themes update --pause` in a terminal.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Args;
use desk_settings::{n_, tr, trn};
use serde_json::json;
use themes_core::update::{self, Behind};
use themes_core::{Ctx, Error};

// Help texts (gettext ids; cli.rs translates them at start-up).
const U01: &str = n_("Only fetch and list the new commits; change nothing");
const U02: &str = n_("Wait for Enter at the end (for a terminal opened by the launcher)");
const U03: &str = n_("Show every step of scripts/update.sh without changing anything");

/// The launcher entry; `--pause` without a terminal opens it to get one.
const LAUNCHER: &str = "io.github.OlehDzhumyk.Themes.Update.desktop";
/// Set on the relaunch, so a terminal that still gives no TTY cannot start another one.
const RELAUNCHED: &str = "DESKTOP_THEMES_UPDATE_RELAUNCHED";

#[derive(Args)]
pub struct UpdateArgs {
    #[arg(long, help = U01, conflicts_with = "dry_run")]
    check: bool,
    #[arg(long, help = U02)]
    pause: bool,
    #[arg(long, short = 'n', help = U03)]
    dry_run: bool,
}

pub fn exec(ctx: &Ctx, a: &UpdateArgs, json: bool) -> Result<i32, Error> {
    if a.pause && !json && !std::io::stdin().is_terminal() && std::env::var_os(RELAUNCHED).is_none()
    {
        // started from the desktop action (no terminal): open the launcher entry, it has one
        return relaunch_in_terminal();
    }
    let r = if a.check {
        check(ctx, json)
    } else {
        run_update(ctx, a.dry_run, json)
    };
    if !a.pause || json {
        return r;
    }
    let code = match r {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}", tr!("Error: {error}", error = e.0));
            1
        }
    };
    println!();
    if code == 0 {
        println!("{}", tr!("Update finished."));
    } else {
        println!("{}", tr!("Update failed (exit code {code}).", code = code));
        println!(
            "{}",
            tr!(
                "The messages above show the step that failed. Fix it and run “themes update” in a terminal again; “themes doctor” checks the installation."
            )
        );
    }
    print!("{}", tr!("Press Enter to close this window."));
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().lock().read_line(&mut String::new());
    Ok(code)
}

fn check(ctx: &Ctx, json: bool) -> Result<i32, Error> {
    let repo = update::recorded_repo(ctx)?;
    let b = update::check(&repo)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"ok": true, "repo": repo, "behind": b}))
                .unwrap_or_default()
        );
    } else {
        print_behind(&b);
    }
    Ok(0)
}

fn print_behind(b: &Behind) {
    if b.count == 0 {
        println!(
            "{}",
            tr!("Up to date with {upstream}.", upstream = b.upstream)
        );
        return;
    }
    println!(
        "{}",
        trn!(
            "{n} commit behind {upstream}:",
            "{n} commits behind {upstream}:",
            b.count,
            n = b.count,
            upstream = b.upstream
        )
    );
    for s in &b.subjects {
        println!("  - {s}");
    }
    if b.count > b.subjects.len() {
        println!(
            "  {}",
            tr!("… and {n} more", n = b.count - b.subjects.len())
        );
    }
    println!("{}", tr!("Update with: themes update"));
}

fn run_update(ctx: &Ctx, dry_run: bool, json: bool) -> Result<i32, Error> {
    let repo = update::recorded_repo(ctx)?;
    // update.sh checks the tree too, but only after its preflight; say it first and clearly.
    // A dry run changes nothing, so a dirty tree does not stop it.
    if !dry_run && let Some(p) = update::status(&repo)?.problem(&repo) {
        return Err(p.into());
    }
    let script = repo.join(update::UPDATE_SCRIPT);
    let mut c = Command::new(&script);
    desk_settings::i18n::restore_child_env(&mut c);
    c.args(update::update_args(dry_run)).current_dir(&repo);
    if json {
        // stdout carries only the JSON result
        c.stdout(Stdio::from(std::io::stderr()));
    }
    let status = c.status().map_err(|e| {
        Error(tr!(
            "could not start {command}: {error}",
            command = script.display(),
            error = e
        ))
    })?;
    let code = status.code().unwrap_or(1);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"ok": code == 0, "repo": repo, "exit_code": code})
            )
            .unwrap_or_default()
        );
    }
    Ok(code)
}

/// Where the launcher entry can be: `$XDG_DATA_HOME` first, then `$XDG_DATA_DIRS`.
fn launcher_candidates(data_home: &Path, data_dirs: &str) -> Vec<PathBuf> {
    std::iter::once(data_home.to_path_buf())
        .chain(
            data_dirs
                .split(':')
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
        )
        .map(|d| d.join("applications").join(LAUNCHER))
        .collect()
}

fn relaunch_in_terminal() -> Result<i32, Error> {
    let home = std::env::var_os("HOME").map_or_default(PathBuf::from);
    let data_home =
        std::env::var_os("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from);
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let Some(entry) = launcher_candidates(&data_home, &data_dirs)
        .into_iter()
        .find(|p| p.is_file())
    else {
        return Err(Error(tr!(
            "No terminal to show the update in: {launcher} is not installed. Run “themes update” in a terminal.",
            launcher = LAUNCHER
        )));
    };
    let mut c = Command::new("gio");
    desk_settings::i18n::restore_child_env(&mut c);
    let status = c
        .arg("launch")
        .arg(&entry)
        .env(RELAUNCHED, "1")
        .status()
        .map_err(|e| {
            Error(tr!(
                "could not start {command}: {error}",
                command = "gio",
                error = e
            ))
        })?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_is_looked_up_in_the_user_dir_first() {
        let c = launcher_candidates(
            Path::new("/home/user/.local/share"),
            "/usr/local/share::/usr/share",
        );
        assert_eq!(
            c,
            [
                PathBuf::from("/home/user/.local/share/applications").join(LAUNCHER),
                PathBuf::from("/usr/local/share/applications").join(LAUNCHER),
                PathBuf::from("/usr/share/applications").join(LAUNCHER),
            ]
        );
    }
}
