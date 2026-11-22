// SPDX-License-Identifier: GPL-3.0-or-later
//! Збої GNOME Shell і захист GNOME від них.
//!
//! Якщо оболонка падає невдовзі після запуску або ввімкнення розширень, GNOME ставить
//! `org.gnome.shell disable-user-extensions=true` — і жодне розширення більше не працює, хоча
//! `enabled-extensions` не змінився. Цей модуль помічає такий стан, знаходить останні падіння
//! справжньої оболонки (`coredumpctl`, headless-оболонки тестів ігноруються) і за `apply.log`
//! визначає, яка тема тоді була застосована й які розширення вона щойно додала (ймовірні винуватці).

use serde::{Deserialize, Serialize};

use crate::dconf;
use crate::paths::Ctx;
use crate::util::run;

pub const DISABLE_USER_EXT_KEY: &str = "/org/gnome/shell/disable-user-extensions";

/// GNOME вимкнув усі користувацькі розширення (захист після збою або вручну).
pub fn user_extensions_disabled() -> bool {
    dconf::read(DISABLE_USER_EXT_KEY).as_deref().map(str::trim) == Some("true")
}

/// Повернути розширення (лише за явною згодою користувача).
pub fn enable_user_extensions() -> crate::Result<()> {
    dconf::write(DISABLE_USER_EXT_KEY, "false")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShellCrash {
    /// Час падіння (unix, секунди).
    pub time: i64,
    /// Те саме, місцевим часом (`YYYY-MM-DD HH:MM:SS`).
    pub local_time: String,
    pub pid: u32,
    pub signal: String,
    /// Верхня функція стека, якщо coredumpctl її показав.
    #[serde(default)]
    pub top_frame: String,
    /// Тема, застосована останньою перед падінням.
    #[serde(default)]
    pub theme: Option<String>,
    /// Розширення, які ця тема ввімкнула порівняно з попередньою (ймовірні винуватці).
    #[serde(default)]
    pub suspects: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CoredumpRow {
    time: i64,
    pid: u32,
    #[serde(default)]
    sig: i32,
    #[serde(default)]
    exe: String,
}

fn signal_name(sig: i32) -> String {
    match sig {
        6 => "SIGABRT".into(),
        7 => "SIGBUS".into(),
        8 => "SIGFPE".into(),
        11 => "SIGSEGV".into(),
        5 => "SIGTRAP".into(),
        n => format!("сигнал {n}"),
    }
}

/// Рядки `apply` з apply.log: (місцевий час, id теми, розширення).
pub fn parse_apply_log(text: &str) -> Vec<(String, String, Vec<String>)> {
    let mut out = Vec::new();
    for line in text.lines() {
        // 2026-01-01 12:00:00 apply macos modules=... ext=a,b,c gtk_links=...
        if line.len() < 20 {
            continue;
        }
        let (ts, rest) = line.split_at(19);
        let Some(rest) = rest.strip_prefix(" apply ") else {
            continue;
        };
        let id = rest.split_whitespace().next().unwrap_or("").to_string();
        let ext = rest
            .split_whitespace()
            .find_map(|w| w.strip_prefix("ext="))
            .map(|s| {
                s.split(',')
                    .filter(|x| !x.is_empty() && *x != "-")
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        out.push((ts.to_string(), id, ext));
    }
    out
}

/// Тема й нові розширення останнього `apply` перед моментом `local_time`.
pub fn culprit(
    applies: &[(String, String, Vec<String>)],
    local_time: &str,
) -> (Option<String>, Vec<String>) {
    let before: Vec<&(String, String, Vec<String>)> = applies
        .iter()
        .filter(|(ts, ..)| ts.as_str() <= local_time)
        .collect();
    let Some((_, id, ext)) = before.last() else {
        return (None, vec![]);
    };
    let prev: Vec<String> = before
        .iter()
        .rev()
        .skip(1)
        .find(|(_, pid, _)| pid != id)
        .map(|(_, _, e)| e.clone())
        .unwrap_or_default();
    let suspects = ext.iter().filter(|u| !prev.contains(u)).cloned().collect();
    (Some(id.clone()), suspects)
}

fn local(unix: i64) -> String {
    gio::glib::DateTime::from_unix_local(unix)
        .ok()
        .and_then(|d| d.format("%Y-%m-%d %H:%M:%S").ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// `coredumpctl info PID`: (командний рядок, верхня функція стека).
fn coredump_details(pid: u32) -> (String, String) {
    let Ok(o) = run("coredumpctl", &["info", "--no-pager", &pid.to_string()], None) else {
        return (String::new(), String::new());
    };
    let mut cmdline = String::new();
    let mut frame = String::new();
    for l in o.stdout.lines() {
        let t = l.trim();
        if let Some(c) = t.strip_prefix("Command Line:") {
            cmdline = c.trim().to_string();
        } else if frame.is_empty() && t.starts_with("#0 ") {
            // "#0  0x00007f... st_theme_node_reduce_border_radius (libst-51.so + 0x...)"
            frame = t
                .split_whitespace()
                .skip(2)
                .take_while(|w| !w.starts_with('('))
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    (cmdline, frame)
}

/// Падіння справжньої (не headless) оболонки за останні `hours` годин, найновіші спершу.
pub fn recent_shell_crashes(ctx: &Ctx, hours: u32) -> Vec<ShellCrash> {
    let since = format!("-{hours}h");
    let Ok(o) = run(
        "coredumpctl",
        &[
            "--no-pager",
            "--json=short",
            "--since",
            &since,
            "list",
            "/usr/bin/gnome-shell",
        ],
        None,
    ) else {
        return vec![];
    };
    let rows: Vec<CoredumpRow> = serde_json::from_str(o.stdout.trim()).unwrap_or_default();
    let applies = std::fs::read_to_string(&ctx.log)
        .map(|t| parse_apply_log(&t))
        .unwrap_or_default();
    let mut out = Vec::new();
    for r in rows.into_iter().rev().take(5) {
        if !r.exe.is_empty() && !r.exe.ends_with("/gnome-shell") {
            continue;
        }
        let (cmdline, top_frame) = coredump_details(r.pid);
        if cmdline.contains("--headless") || cmdline.contains("--nested") {
            continue;
        }
        let time = r.time / 1_000_000;
        let local_time = local(time);
        let (theme, suspects) = culprit(&applies, &local_time);
        out.push(ShellCrash {
            time,
            local_time,
            pid: r.pid,
            signal: signal_name(r.sig),
            top_frame,
            theme,
            suspects,
        });
    }
    out
}

/// Людський опис для doctor/GUI.
pub fn describe(c: &ShellCrash) -> String {
    let mut s = format!("GNOME Shell упав {} ({}", c.local_time, c.signal);
    if !c.top_frame.is_empty() {
        s.push_str(&format!(" у {}", c.top_frame));
    }
    s.push(')');
    if let Some(t) = &c.theme {
        s.push_str(&format!("; тоді була застосована тема «{t}»"));
    }
    if !c.suspects.is_empty() {
        s.push_str(&format!(
            "; ймовірний винуватець: {}",
            c.suspects.join(", ")
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
2026-11-21 18:00:00 apply macos modules=look (було: fedora) keys_changed=1 resets=0 ext=a@x,b@x gtk_links=0 live=true
2026-11-21 18:10:00 doctor fix=true shell=true version=51.1 issues=0 fixed= restored=
2026-11-21 18:19:48 apply macos-glass modules=look (було: macos) keys_changed=1 resets=0 ext=a@x,b@x,glass@x gtk_links=0
2026-11-21 18:21:43 apply macos-glass modules=look (було: macos-glass) keys_changed=1 resets=0 ext=a@x,b@x,glass@x gtk_links=0
2026-11-21 18:22:54 apply macos modules=look (було: macos-glass) keys_changed=1 resets=0 ext=a@x,b@x gtk_links=0
";

    #[test]
    fn apply_log_and_culprit() {
        let a = parse_apply_log(LOG);
        assert_eq!(a.len(), 4);
        assert_eq!(a[1].1, "macos-glass");
        assert_eq!(a[1].2, vec!["a@x", "b@x", "glass@x"]);
        let (t, s) = culprit(&a, "2026-11-21 18:20:08");
        assert_eq!(t.as_deref(), Some("macos-glass"));
        assert_eq!(s, vec!["glass@x"]);
        // повторне застосування тієї самої теми: порівнюємо з попередньою ІНШОЮ темою
        let (t, s) = culprit(&a, "2026-11-21 18:22:00");
        assert_eq!(t.as_deref(), Some("macos-glass"));
        assert_eq!(s, vec!["glass@x"]);
        let (t, s) = culprit(&a, "2026-11-21 17:00:00");
        assert!(t.is_none() && s.is_empty());
        let c = ShellCrash {
            local_time: "2026-11-21 18:20:08".into(),
            signal: "SIGSEGV".into(),
            top_frame: "st_theme_node_reduce_border_radius".into(),
            theme: Some("macos-glass".into()),
            suspects: vec!["glass@x".into()],
            ..Default::default()
        };
        let d = describe(&c);
        assert!(d.contains("macos-glass") && d.contains("glass@x") && d.contains("SIGSEGV"));
    }
}
