// SPDX-License-Identifier: GPL-3.0-or-later
//! Theme keys that run commands (audit S-5). A theme may write any dconf key, and a few keys make
//! the desktop start a program: a custom shortcut's `command`, the terminal's `exec`, autostart-like
//! keys. SECURITY.md treats "a theme that runs code other than the documented hooks" as a
//! vulnerability, so such keys are listed (`apply --dry-run`) and need an explicit yes (CLI `--yes`,
//! the window's dialog) the first time. A yes is remembered for that exact key and value in
//! `<root>/approved-commands.json`; a theme that changes the command asks again.
//!
//! The check is a function of the planned writes (`ApplyResult::writes` of a dry run), so every
//! entry point can use it before it applies.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dconf::Settings;
use crate::error::Result;
use crate::paths::Ctx;

/// A dconf key that makes the desktop run a command, with the value the theme wants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandKey {
    pub key: String,
    pub value: String,
}

/// Whether the dconf key `path` (`/org/…/name`) is one that runs a command: by its own name, not
/// by a list of known schemas, so extensions the project has never seen are covered too.
pub fn runs_command(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name == "command"
        || name == "cmd"
        || name == "exec"
        || name == "script"
        || name.starts_with("command-")
        || name.starts_with("exec-")
        || name.ends_with("-command")
        || name.ends_with("-commands")
        || name.ends_with("-cmd")
        || name.ends_with("-exec")
        || name.ends_with("-script")
        || name.contains("autostart")
        || path.contains("/autostart/")
}

/// A value that runs nothing: empty string or empty list.
fn is_empty_value(v: &str) -> bool {
    let v = v.trim();
    v.is_empty() || v == "''" || v == "\"\"" || v == "[]" || v.ends_with(" []")
}

/// The keys of `writes` that run commands.
pub fn command_keys(writes: &Settings) -> Vec<CommandKey> {
    writes
        .iter()
        .filter(|(k, v)| runs_command(k) && !is_empty_value(v))
        .map(|(k, v)| CommandKey {
            key: k.clone(),
            value: v.clone(),
        })
        .collect()
}

fn approved_path(ctx: &Ctx) -> PathBuf {
    ctx.root.join("approved-commands.json")
}

fn approved(ctx: &Ctx) -> Vec<CommandKey> {
    std::fs::read_to_string(approved_path(ctx))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The command keys of `writes` that the user has not approved yet (same key and same value).
pub fn pending(ctx: &Ctx, writes: &Settings) -> Vec<CommandKey> {
    let ok = approved(ctx);
    command_keys(writes)
        .into_iter()
        .filter(|c| !ok.contains(c))
        .collect()
}

/// Remember the user's yes for these keys and values.
pub fn approve(ctx: &Ctx, keys: &[CommandKey]) -> Result<()> {
    let mut all = approved(ctx);
    for k in keys {
        if !all.contains(k) {
            all.push(k.clone());
        }
    }
    let path = approved_path(ctx);
    std::fs::create_dir_all(&ctx.root)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&all).unwrap_or_default())?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pairs: &[(&str, &str)]) -> Settings {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    const CUSTOM: &str =
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/win11-system-monitor";

    #[test]
    fn keys_that_run_commands_are_found_by_name() {
        for k in [
            format!("{CUSTOM}/command"),
            "/org/gnome/desktop/applications/terminal/exec".to_string(),
            "/org/gnome/desktop/applications/terminal/exec-arg".to_string(),
            "/org/gnome/shell/extensions/x/post-switch-command".to_string(),
            "/org/gnome/shell/extensions/x/run-script".to_string(),
            "/org/gnome/shell/extensions/x/autostart-apps".to_string(),
            "/org/gnome/shell/extensions/x/autostart/entry".to_string(),
        ] {
            assert!(runs_command(&k), "{k}");
        }
        for k in [
            format!("{CUSTOM}/name"),
            format!("{CUSTOM}/binding"),
            "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings".to_string(),
            "/org/gnome/desktop/interface/gtk-theme".to_string(),
            "/org/gnome/shell/extensions/x/commanders".to_string(),
        ] {
            assert!(!runs_command(&k), "{k}");
        }
    }

    #[test]
    fn empty_values_run_nothing() {
        let w = s(&[
            (&format!("{CUSTOM}/command"), "''"),
            ("/org/x/autostart-apps", "@as []"),
            ("/org/x/exec", "'gnome-system-monitor'"),
        ]);
        let c = command_keys(&w);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].key, "/org/x/exec");
    }

    /// The bundled windows11 theme's shortcut is found, and a yes is remembered for exactly that
    /// command: another command for the same key asks again.
    #[test]
    fn an_approval_covers_one_key_and_value() {
        let root = crate::theme::tests::tmp_root("commands");
        let ctx = Ctx::new(root.join("home"), root.clone());
        let w = s(&[
            (&format!("{CUSTOM}/command"), "'gnome-system-monitor'"),
            (&format!("{CUSTOM}/name"), "'System Monitor'"),
        ]);
        let p = pending(&ctx, &w);
        assert_eq!(p.len(), 1);
        approve(&ctx, &p).unwrap();
        assert!(pending(&ctx, &w).is_empty());
        // survives a second approve (no duplicates) and a reload
        approve(&ctx, &p).unwrap();
        assert_eq!(approved(&ctx).len(), 1);
        let w2 = s(&[(&format!("{CUSTOM}/command"), "'sh -c evil'")]);
        assert_eq!(pending(&ctx, &w2).len(), 1);
    }
}
