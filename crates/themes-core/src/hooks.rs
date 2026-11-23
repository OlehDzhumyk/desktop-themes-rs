// SPDX-License-Identifier: GPL-3.0-or-later
//! apps/hooks/{apply,revert}: зміни програм, які не є ключами dconf.
//!
//! A hook gets a time limit ([`hook_timeout`], `DESKTOP_THEMES_HOOK_TIMEOUT` seconds, 30 by
//! default); when it runs longer, its whole process group is killed. Only the first
//! [`OUTPUT_LIMIT`] bytes of its stdout and stderr are kept; the rest is read and dropped, so a
//! talkative hook neither blocks on a full pipe nor fills memory.

use desk_settings::tr;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::paths::Ctx;
use crate::theme::Theme;

/// Default time limit of a hook.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Bytes of stdout and of stderr kept from a hook.
pub const OUTPUT_LIMIT: usize = 64 * 1024;

/// `DESKTOP_THEMES_HOOK_TIMEOUT` (whole seconds, more than 0), else [`DEFAULT_TIMEOUT`].
pub fn hook_timeout() -> Duration {
    std::env::var("DESKTOP_THEMES_HOOK_TIMEOUT")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TIMEOUT)
}

/// Запускає хук. Повертає Err з текстом помилки.
pub fn run_hook(
    ctx: &Ctx,
    theme: &Theme,
    name: &str,
    prev: Option<&str>,
    next: Option<&str>,
) -> Option<Result<(), String>> {
    run_hook_with(ctx, theme, name, prev, next, hook_timeout())
}

pub(crate) fn run_hook_with(
    ctx: &Ctx,
    theme: &Theme,
    name: &str,
    prev: Option<&str>,
    next: Option<&str>,
    timeout: Duration,
) -> Option<Result<(), String>> {
    let path = theme.hook(name)?;
    // успадкований хук працює у теці своєї (батьківської) теми
    let dir = path
        .ancestors()
        .nth(3)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| theme.dir.clone());
    let mut c = Command::new(&path);
    desk_settings::i18n::restore_child_env(&mut c);
    crate::util::child_bus_env(&mut c);
    c.current_dir(&dir)
        .env("THEME_ID", &theme.id)
        .env("THEME_DIR", &dir)
        .env("THEME_HOOK", name)
        .env("DESKTOP_THEMES_ROOT", &ctx.root)
        .env("PREV_THEME_ID", prev.unwrap_or(""))
        .env("NEXT_THEME_ID", next.unwrap_or(""))
        // its own process group: a timeout kills what the hook started, too
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = match run_bounded(c, timeout) {
        Ok(o) => o,
        Err(e) => return Some(Err(format!("{}: {e}", path.display()))),
    };
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    match out.status {
        None => Some(Err(tr!(
            "hook {hook} of the theme {theme} did not finish in {seconds} s and was stopped",
            hook = name,
            theme = theme.id,
            seconds = timeout.as_secs()
        ))),
        Some(s) if s.success() => Some(Ok(())),
        Some(s) => Some(Err(tr!(
            "hook {hook} of the theme {theme} exited with code {code}: {error}",
            hook = name,
            theme = theme.id,
            code = format!("{:?}", s.code()),
            error = err
        ))),
    }
}

struct Bounded {
    /// `None`: killed after the timeout.
    status: Option<ExitStatus>,
    stderr: Vec<u8>,
}

enum Event {
    Out,
    Err(Vec<u8>),
    Exit(std::io::Result<ExitStatus>),
}

/// Keeps the first [`OUTPUT_LIMIT`] bytes, reads the rest into nothing (until EOF).
fn read_bounded(mut r: impl Read) -> Vec<u8> {
    let mut kept = Vec::new();
    let _ = (&mut r).take(OUTPUT_LIMIT as u64).read_to_end(&mut kept);
    let _ = std::io::copy(&mut r, &mut std::io::sink());
    kept
}

/// Runs `c` (spawned in its own process group) for at most `timeout`; on timeout the group is
/// killed with SIGKILL.
fn run_bounded(mut c: Command, timeout: Duration) -> std::io::Result<Bounded> {
    let mut child = c.spawn()?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    if let Some(o) = child.stdout.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            read_bounded(o);
            let _ = tx.send(Event::Out);
        });
    }
    if let Some(e) = child.stderr.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Err(read_bounded(e)));
        });
    }
    std::thread::spawn(move || {
        let _ = tx.send(Event::Exit(child.wait()));
    });
    let deadline = Instant::now() + timeout;
    let (mut out_done, mut stderr, mut status) = (false, None, None);
    while !(out_done && stderr.is_some() && status.is_some()) {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Event::Out) => out_done = true,
            Ok(Event::Err(b)) => stderr = Some(b),
            Ok(Event::Exit(s)) => status = Some(s?),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // the group id is the hook's pid (process_group(0)); it is a process we spawned
                // and have not reaped yet (the waiter still waits on it), so the id is still ours
                if status.is_none()
                    && let Ok(pgid) = libc::pid_t::try_from(pid)
                {
                    // SAFETY: plain syscall on a process group we created.
                    unsafe { libc::killpg(pgid, libc::SIGKILL) };
                }
                return Ok(Bounded {
                    status: None,
                    stderr: stderr.unwrap_or_default(),
                });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(Bounded {
        status,
        stderr: stderr.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::get_theme;
    use crate::theme::tests::{tmp_root, write};
    use std::os::unix::fs::PermissionsExt;

    fn theme_with_hook(tag: &str, body: &str) -> (std::path::PathBuf, Ctx, Theme) {
        let root = tmp_root(tag);
        let ctx = Ctx::new(root.join("home"), root.clone());
        write(&ctx.themes.join("h/theme.conf"), "NAME=\"H\"\n");
        let hook = ctx.themes.join("h/apps/hooks/apply");
        write(&hook, &format!("#!/bin/sh\n{body}\n"));
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let t = get_theme(&ctx, "h").unwrap();
        (root, ctx, t)
    }

    #[test]
    fn failing_hook_reports_bounded_stderr() {
        // 1 MiB on stderr, then a failure: the message keeps at most OUTPUT_LIMIT bytes
        let (root, ctx, t) = theme_with_hook(
            "hook-fail",
            "yes x | head -c 1048576 >&2; yes y | head -c 1048576; exit 3",
        );
        let e = run_hook_with(&ctx, &t, "apply", None, None, Duration::from_secs(600))
            .unwrap()
            .unwrap_err();
        assert!(e.contains("Some(3)"), "{}", &e[..200.min(e.len())]);
        assert!(e.len() < OUTPUT_LIMIT + 500, "{}", e.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hook_over_the_limit_is_killed() {
        // the hook and its child wait forever; a zero limit stops them at once (no timing)
        let (root, ctx, t) = theme_with_hook("hook-timeout", "sleep 100000 & wait");
        let e = run_hook_with(&ctx, &t, "apply", None, None, Duration::ZERO)
            .unwrap()
            .unwrap_err();
        assert!(e.contains("did not finish"), "{e}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn passing_hook_and_env() {
        let (root, ctx, t) = theme_with_hook(
            "hook-ok",
            "[ \"$THEME_ID\" = h ] && [ \"$PREV_THEME_ID\" = old ] || exit 1",
        );
        assert_eq!(
            run_hook_with(&ctx, &t, "apply", Some("old"), None, Duration::from_secs(600)),
            Some(Ok(()))
        );
        assert_eq!(run_hook(&ctx, &t, "revert", None, None), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
