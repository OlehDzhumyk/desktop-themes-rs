// SPDX-License-Identifier: GPL-3.0-or-later
//! Користувацькі flatpak overrides з обліком того, що саме змінила тема.

use desk_settings::tr;
use std::collections::BTreeMap;
use std::path::Path;

use crate::error::Result;
use crate::paths::Ctx;
use crate::state::{FlatpakChange, State};
use crate::theme::Theme;
use crate::util::{run, which};

/// Read-only paths a theme may open to Flatpak apps: what GTK needs to find its theme and icons.
/// Anything else (a different path, `:rw`, `host`, `home`) would be a hole in the sandbox.
const ALLOWED_PATHS: [&str; 6] = [
    "xdg-config/gtk-3.0",
    "xdg-config/gtk-4.0",
    "xdg-data/themes",
    "xdg-data/icons",
    "~/.themes",
    "~/.icons",
];

/// Why `args` (one line of `flatpak.conf`, one `flatpak override --user` call) is not allowed, or
/// `None` when every argument is on the allowlist (S-4): `--filesystem=<allowed path>:ro`,
/// `--env=GTK_THEME=<name>`, and an app id as target.
pub fn refusal(args: &[String]) -> Option<String> {
    args.iter().find_map(|a| refusal_of(a))
}

fn refusal_of(arg: &str) -> Option<String> {
    if let Some(v) = arg.strip_prefix("--filesystem=") {
        let ok = v
            .strip_suffix(":ro")
            .is_some_and(|p| ALLOWED_PATHS.contains(&p));
        return (!ok).then(|| {
            tr!(
                "{arg}: only read-only access (:ro) to the GTK, theme and icon folders is allowed",
                arg = arg
            )
        });
    }
    if let Some(v) = arg.strip_prefix("--env=GTK_THEME=") {
        let ok = !v.is_empty()
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | ':' | '-'));
        return (!ok).then(|| tr!("{arg}: not a valid theme name", arg = arg));
    }
    if arg.starts_with('-') {
        return Some(tr!(
            "{arg}: this Flatpak option is not allowed in a theme (only read-only GTK, theme and icon folders and GTK_THEME)",
            arg = arg
        ));
    }
    (!is_app_id(arg)).then(|| tr!("{arg}: not a Flatpak application id", arg = arg))
}

/// `org.example.App`: at least two dot-separated parts of letters, digits, `_` and `-`.
fn is_app_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

type KeyFile = (BTreeMap<String, Vec<(String, String)>>, Vec<String>);

fn kf_read(path: &Path) -> KeyFile {
    let mut data: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut order = Vec::new();
    let mut sec: Option<String> = None;
    if let Ok(text) = std::fs::read_to_string(path) {
        for s in text.lines() {
            if s.starts_with('[') && s.ends_with(']') {
                let name = s[1..s.len() - 1].to_string();
                if !order.contains(&name) {
                    order.push(name.clone());
                }
                data.entry(name.clone()).or_default();
                sec = Some(name);
            } else if let (Some(sc), Some((k, v))) = (&sec, s.split_once('='))
                && !s.starts_with('#')
            {
                let entry = data.entry(sc.clone()).or_default();
                match entry.iter_mut().find(|(kk, _)| kk == k) {
                    Some(e) => e.1 = v.to_string(),
                    None => entry.push((k.to_string(), v.to_string())),
                }
            }
        }
    }
    (data, order)
}

fn kf_get<'a>(kf: &'a KeyFile, sec: &str, key: &str) -> Option<&'a str> {
    kf.0.get(sec)?
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn kf_write(path: &Path, kf: &KeyFile) -> Result<()> {
    let (data, order) = kf;
    let mut secs: Vec<&String> = order
        .iter()
        .filter(|s| data.get(*s).is_some_and(|v| !v.is_empty()))
        .collect();
    for s in data.keys() {
        if !order.contains(s) && !data[s].is_empty() {
            secs.push(s);
        }
    }
    if secs.is_empty() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }
    let mut out = String::new();
    for s in secs {
        out.push_str(&format!("[{s}]\n"));
        for (k, v) in &data[s] {
            out.push_str(&format!("{k}={v}\n"));
        }
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

fn target_file(ctx: &Ctx, args: &[String]) -> std::path::PathBuf {
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    ctx.flatpak_overrides
        .join(pos.last().map(|s| s.as_str()).unwrap_or("global"))
}

pub fn remove(st: &mut State, warnings: &mut Vec<String>) -> Result<()> {
    for ch in st.flatpak.iter().rev() {
        let p = Path::new(&ch.file);
        let mut kf = kf_read(p);
        if kf_get(&kf, &ch.section, &ch.key) != Some(ch.new.as_str()) {
            warnings.push(tr!(
                "flatpak override {file} [{section}] {key} was changed by hand; not touching it",
                file = p.file_name().unwrap_or_default().to_string_lossy(),
                section = ch.section,
                key = ch.key
            ));
            continue;
        }
        let sec = kf.0.entry(ch.section.clone()).or_default();
        match &ch.old {
            None => sec.retain(|(k, _)| k != &ch.key),
            Some(old) => {
                match sec.iter_mut().find(|(k, _)| k == &ch.key) {
                    Some(e) => e.1 = old.clone(),
                    None => sec.push((ch.key.clone(), old.clone())),
                }
                if !kf.1.contains(&ch.section) {
                    kf.1.push(ch.section.clone());
                }
            }
        }
        kf_write(p, &kf)?;
    }
    st.flatpak.clear();
    Ok(())
}

pub fn apply(
    ctx: &Ctx,
    st: &mut State,
    theme: &Theme,
    warnings: &mut Vec<String>,
) -> Result<usize> {
    for (line, why) in theme.flatpak_refused() {
        warnings.push(tr!(
            "flatpak override “{line}” of the theme is refused: {why}",
            line = line,
            why = why
        ));
    }
    let lines = theme.flatpak_lines();
    if !lines.is_empty() && !which("flatpak") {
        warnings.push(tr!("flatpak not found; overrides skipped"));
        return Ok(0);
    }
    for args in &lines {
        let path = target_file(ctx, args);
        let before = kf_read(&path);
        let mut a: Vec<&str> = vec!["override", "--user"];
        a.extend(args.iter().map(String::as_str));
        let r = run("flatpak", &a, None)?;
        if !r.ok {
            warnings.push(format!("flatpak override {}: {}", args.join(" "), r.stderr));
            continue;
        }
        let after = kf_read(&path);
        for (sec, kvs) in &after.0 {
            for (k, v) in kvs {
                let old = kf_get(&before, sec, k).map(String::from);
                if old.as_deref() != Some(v.as_str()) {
                    st.flatpak.push(FlatpakChange {
                        file: path.to_string_lossy().into(),
                        section: sec.clone(),
                        key: k.clone(),
                        old,
                        new: v.clone(),
                    });
                }
            }
        }
    }
    Ok(lines.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str) -> Vec<String> {
        crate::util::shlex_split(s).unwrap()
    }

    #[test]
    fn the_bundled_overrides_are_allowed() {
        for l in [
            "--filesystem=xdg-config/gtk-4.0:ro",
            "--filesystem=xdg-config/gtk-3.0:ro",
            "--filesystem=xdg-data/themes:ro",
            "--filesystem=xdg-data/icons:ro",
            "--filesystem=~/.themes:ro",
            "--env=GTK_THEME=Adwaita:dark",
            "org.gnome.Calculator --filesystem=xdg-data/icons:ro",
            "--filesystem=xdg-data/icons:ro --env=GTK_THEME=Colloid-Dark",
        ] {
            assert_eq!(refusal(&line(l)), None, "{l}");
        }
    }

    /// The sandbox escapes the audit names, and the near misses of the allowed forms.
    #[test]
    fn everything_else_is_refused() {
        for l in [
            "--filesystem=host",
            "--filesystem=host:ro",
            "--filesystem=home",
            "--filesystem=xdg-config/gtk-4.0",
            "--filesystem=xdg-config/gtk-4.0:rw",
            "--filesystem=xdg-config/gtk-4.0:create",
            "--filesystem=xdg-config/gtk-4.0/../..:ro",
            "--filesystem=xdg-config:ro",
            "--filesystem=/etc:ro",
            "--socket=session-bus",
            "--socket=x11",
            "--talk-name=org.freedesktop.Flatpak",
            "--system-talk-name=org.freedesktop.systemd1",
            "--device=all",
            "--share=network",
            "--env=LD_PRELOAD=/home/u/x.so",
            "--env=GTK_THEME=",
            "--env=GTK_THEME='a b'",
            "--env=GTK_THEME='$(id)'",
            "--persist=.",
            "--reset",
            "not-an-app-id",
            "--filesystem=xdg-data/icons:ro --filesystem=host",
            "--filesystem=xdg-data/icons:ro --socket=session-bus",
        ] {
            assert!(refusal(&line(l)).is_some(), "{l} must be refused");
        }
    }
}
