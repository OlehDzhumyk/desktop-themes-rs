// SPDX-License-Identifier: GPL-3.0-or-later
//! gettext for the Rust programs of desktop-themes-rs (domain [`DOMAIN`]).
//!
//! Source strings are English; `po/uk.po` carries the Ukrainian texts. The language follows the
//! session (LANGUAGE / LC_MESSAGES) unless the `language` setting (or the environment variable
//! `DESKTOP_THEMES_LANGUAGE`) says `uk` or `en`: then [`init`] re-executes the program once with
//! `LANGUAGE` set, before any thread could read the environment, and [`restore_child_env`] gives
//! child processes the user's own `LANGUAGE` back.

use std::path::PathBuf;
use std::process::Command;

pub use gettextrs::{gettext, ngettext};

pub use crate::registry::GETTEXT_DOMAIN as DOMAIN;

/// Overrides the setting: `system`, `uk` or `en` (tests, screenshots).
pub const ENV_LANGUAGE: &str = "DESKTOP_THEMES_LANGUAGE";
/// Where translations are looked up first (development builds).
pub const ENV_LOCALEDIR: &str = "DESKTOP_THEMES_LOCALEDIR";
/// Set by [`init`] when it changed `LANGUAGE`: the original value ("" = unset).
const ENV_ORIG: &str = "DESKTOP_THEMES_ORIG_LANGUAGE";

/// Translate and substitute named placeholders: `tr!("Theme “{name}” applied", name = n)`.
/// `xgettext -k'tr!'` extracts the first argument (flagged `rust-format`, so `msgfmt -c` checks
/// that translations keep the placeholders).
#[macro_export]
macro_rules! tr {
    ($msgid:literal $(,)?) => {
        $crate::i18n::gettext($msgid)
    };
    ($msgid:literal, $($name:ident = $val:expr),+ $(,)?) => {{
        let s = $crate::i18n::gettext($msgid);
        $( let s = s.replace(concat!("{", stringify!($name), "}"), &::std::string::ToString::to_string(&$val)); )+
        s
    }};
}

/// Plural form: `trn!("{n} item", "{n} items", n, n = n)`.
#[macro_export]
macro_rules! trn {
    ($one:literal, $many:literal, $n:expr $(, $name:ident = $val:expr)* $(,)?) => {{
        let s = $crate::i18n::ngettext($one, $many, ($n) as u32);
        $( let s = s.replace(concat!("{", stringify!($name), "}"), &::std::string::ToString::to_string(&$val)); )*
        s
    }};
}

/// `LANGUAGE` value for a setting value; `None` = follow the session.
pub fn language_env(setting: &str) -> Option<&'static str> {
    match setting {
        "uk" => Some("uk"),
        "en" => Some("en"),
        _ => None,
    }
}

/// Directory with `<lang>/LC_MESSAGES/desktop-themes-rs.mo`.
pub fn localedir() -> PathBuf {
    if let Some(d) = std::env::var_os(ENV_LOCALEDIR) {
        return PathBuf::from(d);
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(d) = data.map(|d| d.join("locale"))
        && d.join("uk/LC_MESSAGES")
            .join(format!("{DOMAIN}.mo"))
            .exists()
    {
        return d;
    }
    PathBuf::from("/usr/share/locale")
}

/// Set up gettext. Call first thing in `main`. `setting` reads the `language` key (it may start
/// threads: the re-exec happens before anything else runs).
pub fn init(setting: impl FnOnce() -> Option<String>) {
    if std::env::var_os(ENV_ORIG).is_none() {
        let wanted = std::env::var(ENV_LANGUAGE).ok().or_else(setting);
        if let Some(lang) = wanted.as_deref().and_then(language_env) {
            let cur = std::env::var("LANGUAGE").unwrap_or_default();
            if cur != lang {
                reexec(lang, &cur);
            }
        }
    }
    // SAFETY: called at the start of main, before other threads use locale-dependent functions.
    unsafe {
        gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "");
    }
    let _ = gettextrs::bindtextdomain(DOMAIN, localedir());
    let _ = gettextrs::bind_textdomain_codeset(DOMAIN, "UTF-8");
    let _ = gettextrs::textdomain(DOMAIN);
}

/// Replace the process with itself, `LANGUAGE=lang`. Returns only if exec failed.
fn reexec(lang: &str, orig: &str) {
    use std::os::unix::process::CommandExt;
    let mut args = std::env::args_os();
    let Some(argv0) = args.next() else { return };
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let err = Command::new(exe)
        .arg0(argv0)
        .args(args)
        .env("LANGUAGE", lang)
        .env(ENV_ORIG, orig)
        .exec();
    eprintln!("desktop-themes-rs: cannot apply the language setting: {err}");
}

/// Give a child process the user's own `LANGUAGE` (and let a restarted `themes` decide anew).
pub fn restore_child_env(cmd: &mut Command) -> &mut Command {
    if let Some(orig) = std::env::var_os(ENV_ORIG) {
        if orig.is_empty() {
            cmd.env_remove("LANGUAGE");
        } else {
            cmd.env("LANGUAGE", orig);
        }
        cmd.env_remove(ENV_ORIG);
    }
    cmd
}

/// The `language` setting from GSettings (`None` without the schema).
#[cfg(feature = "gio")]
pub fn language_from_gsettings() -> Option<String> {
    crate::accessors::DeskSettings::new().map(|s| s.language().as_str().to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn tr_substitutes() {
        // aliases, so xgettext does not extract these test strings
        use crate::tr as t;
        use crate::trn as tn;
        // no catalogue bound in tests: the English id comes back
        assert_eq!(
            t!("Theme “{name}” applied", name = "macOS"),
            "Theme “macOS” applied"
        );
        assert_eq!(t!("{a} → {b}", a = 1, b = "x"), "1 → x");
        assert_eq!(tn!("{n} item", "{n} items", 3, n = 3), "3 items");
        assert_eq!(super::language_env("system"), None);
        assert_eq!(super::language_env("uk"), Some("uk"));
    }
}
