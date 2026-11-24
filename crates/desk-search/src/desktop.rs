// SPDX-License-Identifier: GPL-3.0-or-later
//! The applications provider: XDG desktop entries (Desktop Entry Specification 1.5) read from the
//! XDG data directories. Only the `[Desktop Entry]` group matters; entries that are not
//! applications, are hidden, have `NoDisplay=true` or are not shown in the current desktop are
//! skipped. Launching is the adapter's job (the GNOME bridge starts the entry by its desktop id).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// How deep `applications/` is searched for entries (vendor sub-folders).
const MAX_DEPTH: usize = 4;
/// Entries larger than this are not read (a desktop entry is a few kilobytes).
const MAX_ENTRY_BYTES: u64 = 256 * 1024;

/// One application.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DesktopEntry {
    /// The desktop file id: the path below `applications/` with `/` as `-`
    /// (`org.gnome.TextEditor.desktop`).
    pub id: String,
    /// `Name` (the untranslated name).
    pub name: String,
    /// `Name[<lang>]` for the interface language, when the entry has one.
    pub localized_name: Option<String>,
    /// `GenericName`, localized when possible.
    pub generic_name: Option<String>,
    /// `Comment`, localized when possible.
    pub comment: Option<String>,
    /// `Keywords` and the localized `Keywords[<lang>]` together.
    pub keywords: Vec<String>,
    /// `Exec` as written (with field codes such as `%U`).
    pub exec: String,
    /// `Icon`: a theme icon name or an absolute path.
    pub icon: String,
    /// `StartupWMClass`: window class or Wayland `app_id`, when declared by the entry.
    pub startup_wm_class: Option<String>,
}

impl DesktopEntry {
    /// The name to show: the localized one, else `Name`.
    pub fn display_name(&self) -> &str {
        self.localized_name.as_deref().unwrap_or(&self.name)
    }

    /// The program of `Exec` without its path (`gnome-text-editor`), to match a typed command.
    pub fn program(&self) -> &str {
        let first = self.exec.split_whitespace().next().unwrap_or("");
        let first = first.trim_matches('"');
        first.rsplit('/').next().unwrap_or(first)
    }

    /// `Exec` with the field codes removed (`%f %F %u %U %i %c %k` dropped, `%%` as `%`).
    pub fn command_line(&self) -> String {
        let mut out = String::with_capacity(self.exec.len());
        let mut chars = self.exec.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%' {
                // a field code is dropped; `%%` is a literal percent sign
                if chars.next() == Some('%') {
                    out.push('%');
                }
            } else {
                out.push(c);
            }
        }
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// What decides which entries are shown and which translation is read.
#[derive(Clone, Debug, Default)]
pub struct ParseOptions {
    /// The interface language as a locale (`uk_UA.UTF-8`, `uk`, `en`).
    pub lang: String,
    /// The names in `XDG_CURRENT_DESKTOP` (`GNOME`), for `OnlyShowIn` and `NotShowIn`.
    pub desktops: Vec<String>,
}

impl ParseOptions {
    /// The options of this session: `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` or `LANG`, and
    /// `XDG_CURRENT_DESKTOP`.
    pub fn from_env() -> ParseOptions {
        let lang = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .map(|v| v.split(':').next().unwrap_or("").to_owned())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        let desktops = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|d| !d.is_empty())
            .map(str::to_owned)
            .collect();
        ParseOptions { lang, desktops }
    }

    /// The locale keys to try, most specific first: `uk_UA`, `uk`.
    fn locale_keys(&self) -> Vec<String> {
        let base = self.lang.split(['.', '@']).next().unwrap_or("");
        if base.is_empty() || base == "C" || base == "POSIX" {
            return Vec::new();
        }
        let mut keys = vec![base.to_owned()];
        if let Some((language, _)) = base.split_once('_') {
            keys.push(language.to_owned());
        }
        keys
    }
}

/// Undo the escapes of a desktop entry string value (`\s`, `\n`, `\t`, `\r`, `\\`).
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Split a list value on `;` (but not `\;`), unescape, drop empty items.
fn split_list(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut item = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    if n == ';' {
                        item.push(';');
                    } else {
                        item.push('\\');
                        item.push(n);
                    }
                }
            }
            ';' => {
                let v = unescape(item.trim());
                if !v.is_empty() {
                    out.push(v);
                }
                item.clear();
            }
            c => item.push(c),
        }
    }
    let v = unescape(item.trim());
    if !v.is_empty() {
        out.push(v);
    }
    out
}

/// The `key=value` pairs of the `[Desktop Entry]` group.
fn main_group(text: &str) -> Vec<(&str, &str)> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if inside {
                break; // the main group ended
            }
            inside = line == "[Desktop Entry]";
            continue;
        }
        if inside && let Some((k, v)) = line.split_once('=') {
            out.push((k.trim(), v.trim()));
        }
    }
    out
}

/// What the parser decided about one file.
#[allow(
    clippy::large_enum_variant,
    reason = "short-lived return value immediately unpacked by scan"
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    /// An application to list.
    Shown(DesktopEntry),
    /// A valid entry that must not be listed (`Hidden`, `NoDisplay`, `OnlyShowIn`, not an
    /// application). It still masks an entry with the same id in a later data directory.
    Masked,
}

/// Parse a desktop entry. `None` when the text is not a desktop entry or misses `Name`.
pub fn parse(id: &str, text: &str, opts: &ParseOptions) -> Option<Parsed> {
    let pairs = main_group(text);
    if pairs.is_empty() {
        return None;
    }
    let get = |key: &str| pairs.iter().rev().find(|(k, _)| *k == key).map(|(_, v)| *v);
    let locales = opts.locale_keys();
    let localized = |key: &str| {
        locales
            .iter()
            .find_map(|l| get(&format!("{key}[{l}]")))
            .or_else(|| get(key))
            .map(unescape)
            .filter(|v| !v.is_empty())
    };
    let truthy = |key: &str| get(key).is_some_and(|v| v.eq_ignore_ascii_case("true"));

    let name = unescape(get("Name")?);
    if name.is_empty() {
        return None;
    }
    if get("Type") != Some("Application") || truthy("Hidden") || truthy("NoDisplay") {
        return Some(Parsed::Masked);
    }
    let in_desktop = |list: &str| {
        split_list(list)
            .iter()
            .any(|d| opts.desktops.iter().any(|o| o.eq_ignore_ascii_case(d)))
    };
    if get("OnlyShowIn").is_some_and(|l| !in_desktop(l)) || get("NotShowIn").is_some_and(in_desktop)
    {
        return Some(Parsed::Masked);
    }
    let exec = get("Exec").map_or_default(unescape);
    if exec.is_empty() && !truthy("DBusActivatable") {
        return Some(Parsed::Masked);
    }
    let localized_name = locales
        .iter()
        .find_map(|l| get(&format!("Name[{l}]")))
        .map(unescape)
        .filter(|v| !v.is_empty() && *v != name);
    let mut keywords = get("Keywords").map_or_default(split_list);
    if let Some(local) = locales.iter().find_map(|l| get(&format!("Keywords[{l}]"))) {
        for k in split_list(local) {
            if !keywords.contains(&k) {
                keywords.push(k);
            }
        }
    }
    Some(Parsed::Shown(DesktopEntry {
        id: id.to_owned(),
        name,
        localized_name,
        generic_name: localized("GenericName"),
        comment: localized("Comment"),
        keywords,
        exec,
        icon: get("Icon").map_or_default(unescape),
        startup_wm_class: get("StartupWMClass")
            .map(unescape)
            .filter(|v| !v.is_empty()),
    }))
}

/// The XDG data directories in precedence order: `$XDG_DATA_HOME` (default
/// `~/.local/share`), then `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`).
pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        Some(p) => dirs.push(p),
        None => dirs.extend(home.map(|h| h.join(".local/share"))),
    }
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    for d in system.split(':').map(PathBuf::from) {
        if d.is_absolute() && !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

/// The `applications/` folders of `data_dirs` (existing or not), in the same order.
pub fn application_dirs(data_dirs: &[PathBuf]) -> Vec<PathBuf> {
    data_dirs.iter().map(|d| d.join("applications")).collect()
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<_> = read.flatten().collect();
    items.sort_by_key(std::fs::DirEntry::file_name);
    for item in items {
        let path = item.path();
        let Ok(kind) = item.file_type() else { continue };
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                walk(root, &path, depth + 1, out);
            }
        } else if path.extension().is_some_and(|e| e == "desktop")
            && let Ok(rel) = path.strip_prefix(root)
            && let Some(rel) = rel.to_str()
        {
            out.push((rel.replace('/', "-"), path));
        }
    }
}

/// Read every application of `app_dirs` (earlier folders win for the same desktop id; a hidden
/// entry in an earlier folder hides the later ones). Sorted by display name.
pub fn scan(app_dirs: &[PathBuf], opts: &ParseOptions) -> Vec<DesktopEntry> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for root in app_dirs {
        let mut files = Vec::new();
        walk(root, root, 0, &mut files);
        for (id, path) in files {
            if seen.contains(&id) {
                continue;
            }
            let small = std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_ENTRY_BYTES);
            let Some(parsed) = small
                .then(|| std::fs::read_to_string(&path).ok())
                .flatten()
                .and_then(|text| parse(&id, &text, opts))
            else {
                continue;
            };
            seen.insert(id);
            if let Parsed::Shown(entry) = parsed {
                out.push(entry);
            }
        }
    }
    out.sort_by(|a, b| {
        a.display_name()
            .to_lowercase()
            .cmp(&b.display_name().to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDITOR: &str = "\
# a comment
[Desktop Entry]
Name=Text Editor
Name[uk]=Текстовий редактор
Name[de]=Texteditor
GenericName=Text Editor
Comment=Edit text files
Comment[uk]=Редагування текстових файлів
Keywords=text;editor;plaintext;write;
Keywords[uk]=текст;редактор;
Exec=gnome-text-editor %U
Icon=org.gnome.TextEditor
Type=Application
Categories=GNOME;GTK;Utility;TextEditor;

[Desktop Action new-window]
Name=New Window
Exec=gnome-text-editor --new-window
";

    fn uk() -> ParseOptions {
        ParseOptions {
            lang: "uk_UA.UTF-8".into(),
            desktops: vec!["GNOME".into()],
        }
    }

    fn shown(id: &str, text: &str, opts: &ParseOptions) -> DesktopEntry {
        match parse(id, text, opts) {
            Some(Parsed::Shown(e)) => e,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_the_main_group_with_translations() {
        let e = shown("org.gnome.TextEditor.desktop", EDITOR, &uk());
        assert_eq!(e.name, "Text Editor");
        assert_eq!(e.localized_name.as_deref(), Some("Текстовий редактор"));
        assert_eq!(e.display_name(), "Текстовий редактор");
        assert_eq!(e.comment.as_deref(), Some("Редагування текстових файлів"));
        assert_eq!(
            e.keywords,
            ["text", "editor", "plaintext", "write", "текст", "редактор"]
        );
        assert_eq!(e.exec, "gnome-text-editor %U");
        assert_eq!(e.command_line(), "gnome-text-editor");
        assert_eq!(e.program(), "gnome-text-editor");
        assert_eq!(e.icon, "org.gnome.TextEditor");

        let en = shown("x.desktop", EDITOR, &ParseOptions::default());
        assert_eq!(en.localized_name, None);
        assert_eq!(en.display_name(), "Text Editor");
        assert_eq!(en.comment.as_deref(), Some("Edit text files"));
        assert_eq!(en.keywords.len(), 4);
    }

    #[test]
    fn hidden_nodisplay_and_desktop_filters_mask() {
        for extra in [
            "NoDisplay=true",
            "Hidden=true",
            "OnlyShowIn=KDE;",
            "NotShowIn=GNOME;Unity;",
        ] {
            let text = EDITOR.replace("Type=Application", &format!("Type=Application\n{extra}"));
            assert_eq!(
                parse("x.desktop", &text, &uk()),
                Some(Parsed::Masked),
                "{extra}"
            );
        }
        let link = EDITOR.replace("Type=Application", "Type=Link");
        assert_eq!(parse("x.desktop", &link, &uk()), Some(Parsed::Masked));
        let only_gnome = EDITOR.replace("Type=Application", "Type=Application\nOnlyShowIn=GNOME;");
        assert!(matches!(
            parse("x.desktop", &only_gnome, &uk()),
            Some(Parsed::Shown(_))
        ));
        let no_exec = EDITOR.replace("Exec=gnome-text-editor %U\n", "");
        assert_eq!(parse("x.desktop", &no_exec, &uk()), Some(Parsed::Masked));
        let dbus = no_exec.replace("Type=Application", "Type=Application\nDBusActivatable=true");
        assert!(matches!(
            parse("x.desktop", &dbus, &uk()),
            Some(Parsed::Shown(_))
        ));
        assert_eq!(parse("x.desktop", "[Other]\nName=x", &uk()), None);
        assert_eq!(parse("x.desktop", "not an entry", &uk()), None);
        let nameless = EDITOR.replace("Name=Text Editor\n", "");
        assert_eq!(parse("x.desktop", &nameless, &uk()), None);
    }

    #[test]
    fn escapes_and_lists() {
        assert_eq!(unescape(r"a\sb\\c\n"), "a b\\c\n");
        assert_eq!(
            split_list(r"one;two\;three; ;four"),
            ["one", "two;three", "four"]
        );
        let e = DesktopEntry {
            exec: r#""/opt/My App/bin/app" --x %f 100%%"#.into(),
            ..DesktopEntry::default()
        };
        assert_eq!(e.command_line(), r#""/opt/My App/bin/app" --x 100%"#);
    }

    #[test]
    fn locale_keys_from_lang() {
        let keys = |lang: &str| {
            ParseOptions {
                lang: lang.into(),
                desktops: vec![],
            }
            .locale_keys()
        };
        assert_eq!(keys("uk_UA.UTF-8"), ["uk_UA", "uk"]);
        assert_eq!(keys("uk"), ["uk"]);
        assert_eq!(keys("sr_RS@latin"), ["sr_RS", "sr"]);
        assert!(keys("C").is_empty());
        assert!(keys("").is_empty());
    }

    #[test]
    fn scan_respects_precedence_and_sub_folders() {
        let base = std::env::temp_dir().join(format!("desk-search-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let user = base.join("user/applications");
        let system = base.join("system/applications");
        std::fs::create_dir_all(user.join("vendor")).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        // the user hides the system calculator and overrides the editor's name
        std::fs::write(
            user.join("calc.desktop"),
            "[Desktop Entry]\nName=Calc\nType=Application\nExec=calc\nHidden=true\n",
        )
        .unwrap();
        std::fs::write(
            user.join("org.gnome.TextEditor.desktop"),
            EDITOR.replace("Name=Text Editor\n", "Name=My Editor\n"),
        )
        .unwrap();
        std::fs::write(
            user.join("vendor/tool.desktop"),
            "[Desktop Entry]\nName=Tool\nType=Application\nExec=tool\n",
        )
        .unwrap();
        std::fs::write(
            system.join("calc.desktop"),
            "[Desktop Entry]\nName=Calc\nType=Application\nExec=calc\n",
        )
        .unwrap();
        std::fs::write(system.join("org.gnome.TextEditor.desktop"), EDITOR).unwrap();
        std::fs::write(system.join("readme.txt"), "not an entry").unwrap();

        let apps = scan(&[user, system], &ParseOptions::default());
        let ids: Vec<(&str, &str)> = apps
            .iter()
            .map(|a| (a.id.as_str(), a.name.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                ("org.gnome.TextEditor.desktop", "My Editor"),
                ("vendor-tool.desktop", "Tool")
            ]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn application_dirs_join_applications() {
        let dirs = application_dirs(&[PathBuf::from("/usr/share")]);
        assert_eq!(dirs, [PathBuf::from("/usr/share/applications")]);
    }

    #[test]
    fn parses_startup_wm_class() {
        let opts = ParseOptions::default();
        let base = shown("org.gnome.TextEditor.desktop", EDITOR, &opts);
        assert_eq!(base.startup_wm_class, None);

        let with_wm = EDITOR.replace("Type=Application", "Type=Application\nStartupWMClass=Code");
        let e = shown("code.desktop", &with_wm, &opts);
        assert_eq!(e.startup_wm_class.as_deref(), Some("Code"));

        let esc = EDITOR.replace(
            "Type=Application",
            "Type=Application\nStartupWMClass=My\\sApp",
        );
        let e = shown("myapp.desktop", &esc, &opts);
        assert_eq!(e.startup_wm_class.as_deref(), Some("My App"));

        let empty = EDITOR.replace("Type=Application", "Type=Application\nStartupWMClass=");
        let e = shown("empty.desktop", &empty, &opts);
        assert_eq!(e.startup_wm_class, None);
    }
}
