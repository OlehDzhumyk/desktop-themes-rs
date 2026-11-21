// SPDX-License-Identifier: GPL-3.0-or-later
//! Дані сторінки «Подяки»: таблиці з CREDITS.md (вбудовується під час збирання), тож список у
//! програмі й у сховищі завжди однаковий.

pub const CREDITS_MD: &str = include_str!("../../../../CREDITS.md");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub url: Option<String>,
    pub used_for: String,
    pub licence: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub entries: Vec<Entry>,
}

/// `[Name](url) extra` → ("Name extra", Some(url)); прибирає markdown-позначки.
fn link(cell: &str) -> (String, Option<String>) {
    let c = cell.trim();
    if let Some(rest) = c.strip_prefix('[')
        && let Some((name, after)) = rest.split_once("](")
        && let Some((url, tail)) = after.split_once(')')
    {
        let tail = tail.trim();
        let name = if tail.is_empty() {
            name.to_string()
        } else {
            format!("{name} {tail}")
        };
        return (clean(&name), Some(url.to_string()));
    }
    (clean(c), None)
}

fn clean(s: &str) -> String {
    s.replace(['`', '*'], "").trim().to_string()
}

/// Розібрати таблиці markdown: заголовок `## …` → розділ, рядки `| … |` → записи
/// (перша комірка — назва/посилання, друга — для чого, остання — ліцензія).
pub fn parse(md: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut header_seen = false;
    for line in md.lines() {
        let l = line.trim();
        if let Some(t) = l.strip_prefix("## ") {
            out.push(Section {
                title: t.trim().to_string(),
                entries: vec![],
            });
            header_seen = false;
            continue;
        }
        if !l.starts_with('|') {
            header_seen = false;
            continue;
        }
        let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
        if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':')) {
            continue; // |---|---|
        }
        if !header_seen {
            header_seen = true; // перший рядок таблиці — заголовки стовпців
            continue;
        }
        let Some(sec) = out.last_mut() else { continue };
        let (name, url) = link(cells[0]);
        let licence = if cells.len() > 1 {
            clean(cells[cells.len() - 1])
        } else {
            String::new()
        };
        let used_for = if cells.len() > 2 { clean(cells[1]) } else { String::new() };
        sec.entries.push(Entry {
            name,
            url,
            used_for,
            licence,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tables() {
        let md = "# T\n\n## Fonts\n\n| Font | Used for | Version | Licence |\n|---|---|---|---|\n\
                  | [Inter](https://x/inter) (R. A.) | UI font | 4.1 | OFL-1.1 |\n| Noto | UI | d | OFL |\n\n\
                  ## Text\nno table\n";
        let s = parse(md);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title, "Fonts");
        assert_eq!(s[0].entries.len(), 2);
        let e = &s[0].entries[0];
        assert_eq!(e.name, "Inter (R. A.)");
        assert_eq!(e.url.as_deref(), Some("https://x/inter"));
        assert_eq!(e.used_for, "UI font");
        assert_eq!(e.licence, "OFL-1.1");
        assert!(s[1].entries.is_empty());
    }

    #[test]
    fn real_credits_has_clipboard_indicator() {
        let all: Vec<Entry> = parse(CREDITS_MD).into_iter().flat_map(|s| s.entries).collect();
        assert!(all.len() > 10);
        assert!(all.iter().any(|e| e.name.contains("Clipboard Indicator")));
    }
}
