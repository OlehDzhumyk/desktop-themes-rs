// SPDX-License-Identifier: GPL-3.0-or-later
//! apps/hooks/{apply,revert}: зміни програм, які не є ключами dconf.

use desk_settings::tr;
use std::process::Command;

use crate::paths::Ctx;
use crate::theme::Theme;

/// Запускає хук. Повертає Err з текстом помилки.
pub fn run_hook(
    ctx: &Ctx,
    theme: &Theme,
    name: &str,
    prev: Option<&str>,
    next: Option<&str>,
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
    c.current_dir(&dir)
        .env("THEME_ID", &theme.id)
        .env("THEME_DIR", &dir)
        .env("THEME_HOOK", name)
        .env("DESKTOP_THEMES_ROOT", &ctx.root)
        .env("PREV_THEME_ID", prev.unwrap_or(""))
        .env("NEXT_THEME_ID", next.unwrap_or(""))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = match c.output() {
        Ok(o) => o,
        Err(e) => return Some(Err(format!("{}: {e}", path.display()))),
    };
    if out.status.success() {
        Some(Ok(()))
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Some(Err(tr!(
            "hook {hook} of the theme {theme} exited with code {code}: {error}",
            hook = name,
            theme = theme.id,
            code = format!("{:?}", out.status.code()),
            error = err
        )))
    }
}
