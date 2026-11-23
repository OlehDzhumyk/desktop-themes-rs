// SPDX-License-Identifier: GPL-3.0-or-later
//! Формат `dconf dump` і робота з dconf через утиліту `dconf` (як у попередній версії).

use std::collections::BTreeMap;
use std::path::Path;

use gio::glib;

use crate::error::{Context, Result};
use crate::util::{run, run_check};

/// Повний шлях ключа ('/org/gnome/…/key') → значення у текстовому форматі GVariant.
pub type Settings = BTreeMap<String, String>;

pub fn parse(text: &str) -> Settings {
    let mut out = Settings::new();
    let mut section: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = Some(line[1..line.len() - 1].trim().trim_matches('/').to_string());
            continue;
        }
        let Some(sec) = &section else { continue };
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let path = if sec.is_empty() {
            format!("/{k}")
        } else {
            format!("/{sec}/{k}")
        };
        out.insert(path, v.trim().to_string());
    }
    out
}

pub fn parse_file(p: &Path) -> Result<Settings> {
    Ok(parse(&std::fs::read_to_string(p).ctx(p.display())?))
}

/// Settings → текст для `dconf load /` (секції у порядку сортування).
pub fn to_text(d: &Settings) -> String {
    let mut secs: BTreeMap<String, Vec<(&str, &str)>> = BTreeMap::new();
    for (path, val) in d {
        let (sec, key) = path.rsplit_once('/').unwrap_or(("", path));
        let sec = sec.trim_matches('/');
        secs.entry(if sec.is_empty() {
            "/".into()
        } else {
            sec.into()
        })
        .or_default()
        .push((key, val));
    }
    let mut s = String::new();
    for (sec, kvs) in secs {
        s.push_str(&format!("[{sec}]\n"));
        for (k, v) in kvs {
            s.push_str(&format!("{k}={v}\n"));
        }
        s.push('\n');
    }
    s
}

/// Розбір масиву рядків GVariant ('@as []', "['a', 'b']").
pub fn parse_strv(text: &str) -> Vec<String> {
    glib::Variant::parse(Some(glib::VariantTy::STRING_ARRAY), text.trim())
        .ok()
        .and_then(|v| v.get::<Vec<String>>())
        .unwrap_or_default()
}

pub fn fmt_strv(list: &[String]) -> String {
    if list.is_empty() {
        return "@as []".into();
    }
    let items: Vec<String> = list
        .iter()
        .map(|s| format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect();
    format!("[{}]", items.join(", "))
}

pub fn dump() -> Result<Settings> {
    Ok(parse(&run_check("dconf", &["dump", "/"], None)?))
}

pub fn load(d: &Settings) -> Result<()> {
    if d.is_empty() {
        return Ok(());
    }
    run_check("dconf", &["load", "/"], Some(&to_text(d)))?;
    Ok(())
}

/// Resets keys. Many keys go to the dconf writer as one change (one D-Bus call instead of a
/// `dconf reset` process per key); if that is not possible, key by key.
pub fn reset(keys: &[String]) {
    if keys.len() > 1 && reset_batch(keys).is_ok() {
        return;
    }
    for k in keys {
        let _ = run("dconf", &["reset", k], None);
    }
}

/// One `ca.desrt.dconf.Writer.Change` with every key set to "nothing" (what `dconf reset` sends
/// per key: a serialised `a{smv}` changeset).
fn reset_batch(keys: &[String]) -> Result<()> {
    use glib::prelude::ToVariant;
    let fail = |what: &str| crate::Error(format!("dconf writer: {what}"));
    if !keys.iter().all(|k| valid_key(k)) {
        return Err(fail("invalid key"));
    }
    let db = user_db_name().ok_or_else(|| fail("unknown dconf profile"))?;
    let entries = keys.iter().map(|k| {
        glib::Variant::from_dict_entry(
            &k.to_variant(),
            &glib::Variant::from_none(glib::VariantTy::VARIANT),
        )
    });
    let ty = glib::VariantTy::new("{smv}").map_err(|_| fail("type"))?;
    let change = glib::Variant::array_from_iter_with_type(ty, entries);
    let bytes: Vec<u8> = change.data_as_bytes().to_vec();
    let conn = crate::shell::session().ok_or_else(|| fail("no session bus"))?;
    zbus::block_on(conn.call_method(
        Some("ca.desrt.dconf"),
        format!("/ca/desrt/dconf/Writer/{db}").as_str(),
        Some("ca.desrt.dconf.Writer"),
        "Change",
        &(bytes,),
    ))?;
    Ok(())
}

/// A dconf key path: starts with `/`, does not end with `/`, no `//`.
fn valid_key(k: &str) -> bool {
    k.starts_with('/') && !k.ends_with('/') && !k.contains("//")
}

/// Name of the user database the dconf profile writes to (`user-db:NAME` on its first line),
/// found the way dconf finds the profile. `None` when the profile is not understood: the caller
/// then uses the `dconf` utility, which knows every case.
fn user_db_name() -> Option<String> {
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let find = |name: &str| -> Option<std::path::PathBuf> {
        std::iter::once(std::path::PathBuf::from("/etc/dconf/profile"))
            .chain(data_dirs.split(':').map(|d| Path::new(d).join("dconf/profile")))
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    };
    let path = match std::env::var_os("DCONF_PROFILE") {
        Some(p) if Path::new(&p).is_absolute() => std::path::PathBuf::from(p),
        Some(p) if p.to_string_lossy().contains('/') => return None,
        Some(p) => find(&p.to_string_lossy())?,
        None => {
            let runtime = std::env::var_os("XDG_RUNTIME_DIR")
                .map(|d| Path::new(&d).join("dconf.profile"))
                .filter(|p| p.is_file());
            match runtime.or_else(|| find("user")) {
                Some(p) => p,
                None => return Some("user".into()),
            }
        }
    };
    db_from_profile(&std::fs::read_to_string(path).ok()?)
}

fn db_from_profile(text: &str) -> Option<String> {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    let name = first.strip_prefix("user-db:")?.trim();
    (!name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
        .then(|| name.to_string())
}

pub fn write(key: &str, val: &str) -> Result<()> {
    run_check("dconf", &["write", key, val], None)?;
    Ok(())
}

pub fn read(key: &str) -> Option<String> {
    let o = run("dconf", &["read", key], None).ok()?;
    let s = o.stdout.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Значення схеми за замовчуванням (без звернення до dconf).
pub fn schema_default(schema: &str, key: &str) -> Option<String> {
    let src = gio::SettingsSchemaSource::default()?;
    let sch = src.lookup(schema, true)?;
    if !sch.has_key(key) {
        return None;
    }
    Some(sch.key(key).default_value().print(true).to_string())
}

/// Чинне значення ключа (dconf, а якщо не задано — типове схеми).
pub fn effective(schema: &str, key: &str) -> Option<glib::Variant> {
    let src = gio::SettingsSchemaSource::default()?;
    let sch = src.lookup(schema, true)?;
    if !sch.has_key(key) {
        return None;
    }
    use gio::prelude::SettingsExt;
    let s = gio::Settings::new_full(&sch, None::<&gio::SettingsBackend>, None);
    Some(s.value(key))
}

pub fn effective_string(schema: &str, key: &str) -> Option<String> {
    effective(schema, key).and_then(|v| v.str().map(|s| s.to_string()))
}

/// Текстове значення GVariant-рядка → сам рядок ("'WhiteSur'" → "WhiteSur").
pub fn unquote_str(v: &str) -> Option<String> {
    glib::Variant::parse(Some(glib::VariantTy::STRING), v.trim())
        .ok()
        .and_then(|v| v.str().map(String::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# коментар у стилі GKeyFile\n; ще один\n\n[org/gnome/Ptyxis]\nuse-system-font=false\nfont-name='JetBrains Mono 12'\n\n[org/gnome/Ptyxis/Profiles/875876f6c288142aa66c55026af351de]\npalette='Pro'\n[/]\nroot-key=1\n";

    #[test]
    fn parse_skips_comments_and_builds_paths() {
        let d = parse(SAMPLE);
        assert_eq!(d.len(), 4);
        assert_eq!(d["/org/gnome/Ptyxis/use-system-font"], "false");
        assert_eq!(d["/org/gnome/Ptyxis/font-name"], "'JetBrains Mono 12'");
        assert_eq!(
            d["/org/gnome/Ptyxis/Profiles/875876f6c288142aa66c55026af351de/palette"],
            "'Pro'"
        );
        assert_eq!(d["/root-key"], "1");
    }

    #[test]
    fn value_with_equals_sign_kept() {
        let d = parse("[a/b]\nk=['x=y', 'z']\n");
        assert_eq!(d["/a/b/k"], "['x=y', 'z']");
    }

    #[test]
    fn roundtrip() {
        let d = parse(SAMPLE);
        assert_eq!(parse(&to_text(&d)), d);
    }

    #[test]
    fn strv() {
        assert_eq!(parse_strv("@as []"), Vec::<String>::new());
        assert_eq!(parse_strv("['a@b', 'c']"), vec!["a@b", "c"]);
        assert_eq!(parse_strv("garbage"), Vec::<String>::new());
        let v = vec!["it's".to_string(), "x".to_string()];
        assert_eq!(parse_strv(&fmt_strv(&v)), v);
        assert_eq!(fmt_strv(&[]), "@as []");
        assert_eq!(fmt_strv(&["a".into(), "b".into()]), "['a', 'b']");
    }

    #[test]
    fn profile_user_db() {
        assert_eq!(db_from_profile("user-db:user\nsystem-db:local\n").as_deref(), Some("user"));
        assert_eq!(db_from_profile("# c\n\n  user-db:e2e_1\n").as_deref(), Some("e2e_1"));
        assert_eq!(db_from_profile("service-db:x\nuser-db:user\n"), None);
        assert_eq!(db_from_profile("user-db:../x\n"), None);
        assert_eq!(db_from_profile(""), None);
        assert!(valid_key("/a/b") && !valid_key("/a/") && !valid_key("a/b") && !valid_key("/a//b"));
    }

    #[test]
    fn unquote() {
        assert_eq!(
            unquote_str("'WhiteSur-Dark'").as_deref(),
            Some("WhiteSur-Dark")
        );
        assert_eq!(unquote_str("42"), None);
    }
}
