// SPDX-License-Identifier: GPL-3.0-or-later
//! i18n lint over the whole repository (no gettext tools needed):
//!
//! * no Ukrainian (Cyrillic) string literals in program code: user-visible text is English in the
//!   source and Ukrainian in po/uk.po (tests, comments and lines marked `// i18n-ok` are exempt);
//! * no raw string literals passed to the GTK/libadwaita text setters in the window and the CLI,
//!   nor to the St/PopupMenu text properties in the bridge (they must go through `tr!` / `_()`);
//! * every `tr!`/`trn!`/`n_` id in Rust and every `_()` id in the bridge has a translation in
//!   po/uk.po, nothing is fuzzy, and translations keep the `{placeholders}` of their ids.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if p.is_dir() {
            if !matches!(name.as_str(), "target" | "node_modules" | "build" | "tests") {
                files(&p, ext, out);
            }
        } else if name.ends_with(ext) {
            out.push(p);
        }
    }
}

/// Program code of a Rust file: up to its `#[cfg(test)]` module.
fn rust_code(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// String literals (`quote`-delimited) on a line, comments cut off.
fn literals(line: &str, quote: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match &mut cur {
            None if c == '/' && chars.peek() == Some(&'/') => break,
            None if c == quote => cur = Some(String::new()),
            None => {}
            Some(s) if c == '\\' => {
                if let Some(n) = chars.next() {
                    s.push('\\');
                    s.push(n);
                }
            }
            Some(s) if c == quote => {
                out.push(std::mem::take(s));
                cur = None;
            }
            Some(s) => s.push(c),
        }
    }
    out
}

#[test]
fn no_ukrainian_literals_in_code() {
    let root = repo();
    let mut bad = Vec::new();
    let mut rs = Vec::new();
    files(&root.join("crates"), ".rs", &mut rs);
    for p in rs {
        let src = std::fs::read_to_string(&p).unwrap();
        for (i, line) in rust_code(&src).lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || line.contains("i18n-ok") {
                continue;
            }
            if literals(line, '"').iter().any(|l| cyrillic(l)) {
                bad.push(format!("{}:{}: {}", p.display(), i + 1, t));
            }
        }
    }
    let mut ts = Vec::new();
    files(&root.join("bridge/src"), ".ts", &mut ts);
    for p in ts {
        let src = std::fs::read_to_string(&p).unwrap();
        for (i, line) in src.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
                continue;
            }
            let lits: Vec<String> = ['\'', '"', '`'].iter().flat_map(|q| literals(line, *q)).collect();
            if lits.iter().any(|l| cyrillic(l)) {
                bad.push(format!("{}:{}: {}", p.display(), i + 1, t));
            }
        }
    }
    assert!(bad.is_empty(), "translatable text must be English + po/uk.po:\n{}", bad.join("\n"));
}

#[test]
fn no_raw_literals_in_ui_setters() {
    let root = repo();
    let setters = [
        ".title(\"", ".subtitle(\"", ".label(\"", ".description(\"", ".tooltip_text(\"",
        ".heading(\"", ".body(\"", ".placeholder_text(\"", ".button_label(\"", ".set_title(\"",
        ".set_subtitle(\"", ".set_label(\"", ".set_description(\"", ".application_name(\"",
        ".comments(\"", "Toast::new(\"", "Some(\"",
    ];
    let mut bad = Vec::new();
    let mut rs = Vec::new();
    files(&root.join("crates/themes/src"), ".rs", &mut rs);
    for p in rs {
        let src = std::fs::read_to_string(&p).unwrap();
        for (i, line) in rust_code(&src).lines().enumerate() {
            if line.trim_start().starts_with("//") || line.contains("i18n-ok") {
                continue;
            }
            for s in setters {
                if let Some(pos) = line.find(s) {
                    let rest = &line[pos + s.len()..];
                    let lit: String = rest.chars().take_while(|c| *c != '"').collect();
                    // ids such as Some("cancel") are fine: only text with spaces or capitals is flagged
                    let looks_like_text =
                        lit.contains(' ') || lit.chars().next().is_some_and(char::is_uppercase);
                    if looks_like_text {
                        bad.push(format!("{}:{}: {}", p.display(), i + 1, line.trim()));
                    }
                }
            }
        }
    }
    let props = [
        "label: '", "text: '", "hint_text: '", "accessible_name: '", "title: '",
        "PopupMenuItem('", "addAction('", "notify('", "notifyError('", "SwitchMenuItem('",
        "setHeader(THEME_ICON, '", "setHeader(CLIP_ICON, '",
    ];
    let mut ts = Vec::new();
    files(&root.join("bridge/src"), ".ts", &mut ts);
    for p in ts {
        let src = std::fs::read_to_string(&p).unwrap();
        for (i, line) in src.lines().enumerate() {
            for s in props {
                if let Some(pos) = line.find(s) {
                    let lit: String = line[pos + s.len()..].chars().take_while(|c| *c != '\'').collect();
                    if lit.chars().any(char::is_alphabetic) {
                        bad.push(format!("{}:{}: {}", p.display(), i + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "user-visible literals must be translated:\n{}", bad.join("\n"));
}

/// msgid → (msgstr, fuzzy) from a .po file.
fn parse_po(text: &str) -> BTreeMap<String, (String, bool)> {
    fn unq(l: &str) -> String {
        let s = &l[l.find('"').unwrap() + 1..l.rfind('"').unwrap()];
        unescape(s)
    }
    let mut out = BTreeMap::new();
    for block in text.split("\n\n") {
        let fuzzy = block.lines().any(|l| l.starts_with("#,") && l.contains("fuzzy"));
        let (mut id, mut st, mut cur) = (String::new(), String::new(), 0);
        for l in block.lines() {
            if l.starts_with("msgid ") {
                cur = 1;
            } else if l.starts_with("msgstr") {
                cur = 2;
            } else if !l.starts_with('"') {
                continue;
            }
            match cur {
                1 => id.push_str(&unq(l)),
                2 => st.push_str(&unq(l)),
                _ => {}
            }
        }
        if !id.is_empty() {
            out.insert(id, (st, fuzzy));
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut o = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            o.push(c);
            continue;
        }
        match it.next() {
            Some('n') => o.push('\n'),
            Some('t') => o.push('\t'),
            Some('\n') => {
                // Rust line continuation: skip the next line's indentation
                while it.peek().is_some_and(|c| c.is_whitespace()) {
                    it.next();
                }
            }
            Some(x) => o.push(x),
            None => {}
        }
    }
    o
}

/// First string argument of every `marker` call in `src` (quote char `q`).
fn calls(src: &str, marker: &str, q: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find(marker) {
        // the marker must not be the tail of a longer identifier
        let before = rest[..i].chars().last();
        rest = &rest[i + marker.len()..];
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let t = rest.trim_start();
        if !t.starts_with(q) {
            continue;
        }
        let mut lit = String::new();
        let mut chars = t[1..].chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                lit.push('\\');
                if let Some(n) = chars.next() {
                    lit.push(n);
                }
            } else if c == q {
                break;
            } else {
                lit.push(c);
            }
        }
        out.push(unescape(&lit));
    }
    out
}

fn placeholders(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        rest = &rest[i + 1..];
        if let Some(j) = rest.find('}') {
            let name = &rest[..j];
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                out.insert(name.to_string());
            }
        }
    }
    out
}

#[test]
fn every_message_is_translated() {
    fn strip_comments(src: &str) -> String {
        src.lines()
            .filter(|l| {
                let t = l.trim_start();
                !(t.starts_with("//") || t.starts_with('*') || t.starts_with("/*"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    let root = repo();
    let po = parse_po(&std::fs::read_to_string(root.join("po/uk.po")).expect("po/uk.po"));
    let mut ids = BTreeSet::new();
    let mut rs = Vec::new();
    files(&root.join("crates"), ".rs", &mut rs);
    for p in rs {
        let src = std::fs::read_to_string(&p).unwrap();
        let code = strip_comments(rust_code(&src));
        for m in ["tr!(", "trn!(", "n_("] {
            for id in calls(&code, m, '"') {
                ids.insert((id, p.display().to_string()));
            }
        }
    }
    let mut ts = Vec::new();
    files(&root.join("bridge/src"), ".ts", &mut ts);
    for p in ts {
        let src = std::fs::read_to_string(&p).unwrap();
        for id in calls(&strip_comments(&src), "_(", '\'') {
            ids.insert((id, p.display().to_string()));
        }
    }
    assert!(ids.len() > 200, "the scanner found too few messages: {}", ids.len());
    let mut missing = Vec::new();
    for (id, file) in &ids {
        match po.get(id) {
            Some((s, false)) if !s.is_empty() => {}
            _ => missing.push(format!("{file}: {id:?}")),
        }
    }
    assert!(missing.is_empty(), "not translated in po/uk.po (run `cargo xtask pot`, then translate):\n{}", missing.join("\n"));
    let mut bad = Vec::new();
    for (id, (s, _)) in &po {
        if !s.is_empty() && placeholders(id) != placeholders(s) {
            bad.push(format!("{id:?} → {s:?}"));
        }
    }
    assert!(bad.is_empty(), "translations must keep the placeholders:\n{}", bad.join("\n"));
}
