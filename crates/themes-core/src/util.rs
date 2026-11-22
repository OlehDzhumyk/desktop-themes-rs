// SPDX-License-Identifier: GPL-3.0-or-later
use desk_settings::tr;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};
use crate::paths::Ctx;

/// POSIX-подібний розбір рядка на слова (як Python `shlex.split(s, comments=True)`).
pub fn shlex_split(s: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '#' if !in_word => break,
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match it.next() {
                        Some('\'') => break,
                        Some(ch) => cur.push(ch),
                        None => return Err(Error(tr!("unclosed single quote"))),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match it.next() {
                        Some('"') => break,
                        Some('\\') => match it.next() {
                            Some(ch @ ('\\' | '"' | '$' | '`')) => cur.push(ch),
                            Some('\n') => {}
                            Some(ch) => {
                                cur.push('\\');
                                cur.push(ch);
                            }
                            None => return Err(Error(tr!("unclosed double quote"))),
                        },
                        Some(ch) => cur.push(ch),
                        None => return Err(Error(tr!("unclosed double quote"))),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(ch) = it.next()
                    && ch != '\n'
                {
                    cur.push(ch);
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

/// Лапкування значення для theme.conf (сумісне з shell і з `shlex_split`).
pub fn conf_quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            o.push('\\');
        }
        o.push(c);
    }
    o.push('"');
    o
}

pub fn now(fmt: &str) -> String {
    gio::glib::DateTime::now_local()
        .ok()
        .and_then(|d| d.format(fmt).ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

pub fn log(ctx: &Ctx, msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.log)
    {
        let _ = writeln!(f, "{} {}", now("%Y-%m-%d %H:%M:%S"), msg);
    }
}

/// Запуск команди з перехопленням виводу.
pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(cmd: &str, args: &[&str], input: Option<&str>) -> Result<Out> {
    let mut c = Command::new(cmd);
    desk_settings::i18n::restore_child_env(&mut c);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = c.spawn().map_err(|e| {
        Error(tr!(
            "could not start {command}: {error}",
            command = cmd,
            error = e
        ))
    })?;
    if let Some(inp) = input {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(inp.as_bytes())?;
    }
    let o = child.wait_with_output()?;
    Ok(Out {
        ok: o.status.success(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).trim().to_string(),
    })
}

pub fn run_check(cmd: &str, args: &[&str], input: Option<&str>) -> Result<String> {
    let o = run(cmd, args, input)?;
    if !o.ok {
        return Err(Error(tr!(
            "command failed: {command}\n{error}",
            command = format!("{cmd} {}", args.join(" ")),
            error = o.stderr
        )));
    }
    Ok(o.stdout)
}

pub fn which(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

/// Якщо змінна DBUS_SESSION_BUS_ADDRESS не задана (наприклад, з SSH), беремо /run/user/UID/bus.
pub fn ensure_bus() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        let uid = unsafe_getuid();
        let p = format!("/run/user/{uid}/bus");
        if Path::new(&p).exists() {
            // SAFETY: викликається на початку main, до створення потоків.
            unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={p}")) };
        }
    }
}

pub fn unsafe_getuid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(1000)
}

const TRANSLIT: &[(char, &str)] = &[
    ('а', "a"),
    ('б', "b"),
    ('в', "v"),
    ('г', "h"),
    ('ґ', "g"),
    ('д', "d"),
    ('е', "e"),
    ('є', "ie"),
    ('ж', "zh"),
    ('з', "z"),
    ('и', "y"),
    ('і', "i"),
    ('ї', "i"),
    ('й', "i"),
    ('к', "k"),
    ('л', "l"),
    ('м', "m"),
    ('н', "n"),
    ('о', "o"),
    ('п', "p"),
    ('р', "r"),
    ('с', "s"),
    ('т', "t"),
    ('у', "u"),
    ('ф', "f"),
    ('х', "kh"),
    ('ц', "ts"),
    ('ч', "ch"),
    ('ш', "sh"),
    ('щ', "shch"),
    ('ь', ""),
    ('ю', "iu"),
    ('я', "ia"),
    ('ы', "y"),
    ('э', "e"),
    ('ё', "io"),
    ('ъ', ""),
];

/// Назва → id теми (транслітерація українською).
pub fn slugify(name: &str) -> String {
    let mut s = String::new();
    for c in name.to_lowercase().chars() {
        match TRANSLIT.iter().find(|(k, _)| *k == c) {
            Some((_, v)) => s.push_str(v),
            None => s.push(c),
        }
    }
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c);
        } else {
            dash = true;
        }
    }
    out.truncate(40);
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        format!("tema-{}", now("%Y%m%d-%H%M%S"))
    } else {
        out
    }
}

pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(c))
}

pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Аналог os.path.lexists.
pub fn lexists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// shutil.move: rename, а якщо між ФС — копія з видаленням.
pub fn move_path(src: &Path, dst: &Path) -> Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    let o = run(
        "mv",
        &[&src.to_string_lossy(), &dst.to_string_lossy()],
        None,
    )?;
    if !o.ok {
        return Err(Error(tr!(
            "could not move {from} → {to}: {error}",
            from = src.display(),
            to = dst.display(),
            error = o.stderr
        )));
    }
    Ok(())
}

pub fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    // cp -rL: розіменовує посилання (як shutil.copytree(symlinks=False))
    let o = run(
        "cp",
        &[
            "-rL",
            "--preserve=timestamps",
            &src.to_string_lossy(),
            &dst.to_string_lossy(),
        ],
        None,
    )?;
    if !o.ok {
        return Err(Error(tr!(
            "could not copy {path}: {error}",
            path = src.display(),
            error = o.stderr
        )));
    }
    Ok(())
}

pub fn url_unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn url_quote_path(s: &str) -> String {
    let mut o = String::new();
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || b"/-_.~".contains(&c) {
            o.push(c as char);
        } else {
            o.push_str(&format!("%{c:02X}"));
        }
    }
    o
}

/// Розкриває `~`, `$HOME` і `${HOME}` у шляху (для PREVIEW у theme.conf).
pub fn expand_home(s: &str, home: &str) -> String {
    let mut s = s.to_string();
    if s == "~" {
        return home.to_string();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        s = format!("{home}/{rest}");
    }
    subst_home(&s, |_| home.to_string())
}

/// Підставляє $HOME / ${HOME}; `f(перед)` отримує текст перед збігом (щоб відрізнити file://).
fn subst_home(s: &str, f: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let (len, hit) = if rest.starts_with("${HOME}") {
            (7, true)
        } else if let Some(r) = rest.strip_prefix("$HOME") {
            let boundary = !r
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            (5, boundary)
        } else {
            (0, false)
        };
        if hit {
            out.push_str(&f(&out));
            i += len;
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// Розкриває домашню теку у значенні dconf (текст GVariant): `'file://${HOME}/x'`, `'$HOME/x'`, `'~/x'`.
/// У `file://` шлях кодується як URI, в інших рядках лапки й `\\` екрануються.
pub fn expand_home_value(v: &str, home: &str) -> String {
    let mut v = v.to_string();
    for q in ['\'', '"'] {
        let lit = format!("{q}~/");
        if v.contains(&lit) {
            v = v.replace(&lit, &format!("{q}${{HOME}}/"));
        }
        let uri = format!("{q}file://~/");
        if v.contains(&uri) {
            v = v.replace(&uri, &format!("{q}file://${{HOME}}/"));
        }
    }
    let plain = home.replace('\\', "\\\\").replace('\'', "\\'");
    let uri = url_quote_path(home);
    subst_home(&v, |before| {
        if before.ends_with("file://") {
            uri.clone()
        } else {
            plain.clone()
        }
    })
}

/// 'file:///…' (значення GVariant у лапках) → шлях.
pub fn file_from_uri(v: &str) -> Option<String> {
    let v = v.trim().trim_matches(|c| c == '\'' || c == '"');
    if let Some(rest) = v.strip_prefix("file://") {
        return Some(url_unquote(rest));
    }
    if v.starts_with('/') {
        Some(v.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shlex_basic() {
        assert_eq!(
            shlex_split(r#"--filesystem=xdg-config/gtk-4.0:ro  # c"#).unwrap(),
            vec!["--filesystem=xdg-config/gtk-4.0:ro"]
        );
        assert_eq!(
            shlex_split(r#""a b" 'c d' e\ f"#).unwrap(),
            vec!["a b", "c d", "e f"]
        );
        assert_eq!(shlex_split(r#""x\"y\$""#).unwrap(), vec!["x\"y$"]);
        assert!(shlex_split("\"abc").is_err());
        assert_eq!(
            shlex_split(&conf_quote("Мій \"вигляд\" $x")).unwrap(),
            vec!["Мій \"вигляд\" $x"]
        );
    }

    #[test]
    fn slug() {
        assert_eq!(slugify("Мій вигляд"), "mii-vyhliad");
        assert_eq!(slugify("  Dark  Mode!! "), "dark-mode");
        assert!(
            valid_id("macos")
                && valid_id("zz-test")
                && !valid_id("Bad")
                && !valid_id("-x")
                && !valid_id("")
        );
    }

    #[test]
    fn uri() {
        assert_eq!(
            file_from_uri("'file:///home/a%20b/x.jpg'").as_deref(),
            Some("/home/a b/x.jpg")
        );
        assert_eq!(url_quote_path("/home/a b/x.jpg"), "/home/a%20b/x.jpg");
        assert_eq!(file_from_uri("'resource:///x'"), None);
    }

    #[test]
    fn home_expansion() {
        let h = "/var/home/u";
        assert_eq!(expand_home("~/a/b.png", h), "/var/home/u/a/b.png");
        assert_eq!(expand_home("$HOME/a", h), "/var/home/u/a");
        assert_eq!(expand_home("${HOME}/a", h), "/var/home/u/a");
        assert_eq!(expand_home("/usr/share/x.jxl", h), "/usr/share/x.jxl");
        assert_eq!(expand_home("$HOMER/a", h), "$HOMER/a");
        assert_eq!(expand_home("~x/a", h), "~x/a");
        assert_eq!(
            expand_home_value("'file://${HOME}/.local/bg.png'", h),
            "'file:///var/home/u/.local/bg.png'"
        );
        assert_eq!(
            expand_home_value("'file://${HOME}/a'", "/home/a b"),
            "'file:///home/a%20b/a'"
        );
        assert_eq!(expand_home_value("'~/x'", h), "'/var/home/u/x'");
        assert_eq!(
            expand_home_value("'file://~/x'", h),
            "'file:///var/home/u/x'"
        );
        assert_eq!(expand_home_value("'$HOME/x'", "/h/o'k"), "'/h/o\\'k/x'");
        assert_eq!(expand_home_value("'Mono 12'", h), "'Mono 12'");
        assert_eq!(
            expand_home_value("['${HOME}/a', '/b']", h),
            "['/var/home/u/a', '/b']"
        );
    }
}
