// SPDX-License-Identifier: GPL-3.0-or-later
//! Snapshot tests: every generator's output for the bundled themes' tokens.toml is compared with
//! tests/snapshots/<theme>/. After an intended change run
//! `UPDATE_SNAPSHOTS=1 cargo test -p theme-tokens --test snapshots` and review the diff.

use std::path::{Path, PathBuf};

use theme_tokens::generate::{self, Target};
use theme_tokens::{KeyModule, keys, load_chain};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn target(id: &str, name: &str) -> Target {
    Target {
        theme_id: id.into(),
        theme_name: name.into(),
        home: "/home/user".into(),
        cache_dir: PathBuf::from(format!("/home/user/.cache/desktop-themes/{id}")),
        handmade_gtk4: false,
        handmade_shell_theme: false,
    }
}

fn check(dir: &Path, name: &str, actual: &str, failures: &mut Vec<String>) {
    let p = dir.join(name);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&p, actual).unwrap();
        return;
    }
    match std::fs::read_to_string(&p) {
        Ok(expected) if expected == actual => {}
        Ok(expected) => {
            let line = expected
                .lines()
                .zip(actual.lines())
                .position(|(a, b)| a != b)
                .map_or(
                    expected.lines().count().min(actual.lines().count()) + 1,
                    |i| i + 1,
                );
            failures.push(format!("{} differs from line {line}", p.display()));
        }
        Err(_) => failures.push(format!(
            "{} is missing (run with UPDATE_SNAPSHOTS=1)",
            p.display()
        )),
    }
}

fn keys_text(m: &std::collections::BTreeMap<KeyModule, theme_tokens::Keys>) -> String {
    let mut s = String::new();
    for (module, keys) in m {
        s.push_str(&format!("[{module:?}]\n"));
        for (k, v) in keys {
            s.push_str(&format!("{k}={v}\n"));
        }
    }
    s
}

#[test]
fn bundled_theme_snapshots() {
    let mut failures = Vec::new();
    for (id, name) in [
        ("lavender", "Lavender"),
        ("macos", "macOS"),
        ("windows11", "Windows 11"),
    ] {
        let path = repo().join("themes").join(id).join("tokens.toml");
        let text = std::fs::read_to_string(&path).unwrap();
        let l = load_chain(&[(format!("themes/{id}/tokens.toml"), text)])
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(l.warnings.is_empty(), "{id}: {:?}", l.warnings);
        let tg = target(id, name);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(id);
        for f in generate::render(&l.tokens, &tg) {
            let file = Path::new(&f.rel)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            check(&dir, &file, &f.contents, &mut failures);
        }
        check(
            &dir,
            "keys.txt",
            &keys_text(&keys(&l.tokens, &tg)),
            &mut failures,
        );
        // determinism
        assert_eq!(
            generate::render(&l.tokens, &tg),
            generate::render(&l.tokens, &tg)
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn handmade_files_suppress_generation() {
    let text = std::fs::read_to_string(repo().join("themes/lavender/tokens.toml")).unwrap();
    let l = load_chain(&[("t".into(), text)]).unwrap();
    let mut tg = target("lavender", "Lavender");
    tg.handmade_gtk4 = true;
    tg.handmade_shell_theme = true;
    let files: Vec<String> = generate::render(&l.tokens, &tg)
        .into_iter()
        .map(|f| f.rel)
        .collect();
    assert!(
        !files
            .iter()
            .any(|f| f.starts_with("gtk4/") || f.starts_with("shell/")),
        "{files:?}"
    );
    let k = keys(&l.tokens, &tg);
    assert!(!k[&KeyModule::Look].contains_key("/org/gnome/shell/extensions/user-theme/name"));
}

#[test]
fn macos_keys_match_previous_dconf_files() {
    // the migration must keep what macos/settings.dconf, apps.dconf and sounds.dconf used to set
    let text = std::fs::read_to_string(repo().join("themes/macos/tokens.toml")).unwrap();
    let l = load_chain(&[("t".into(), text)]).unwrap();
    let k = keys(&l.tokens, &target("macos", "macOS"));
    let look = &k[&KeyModule::Look];
    assert_eq!(look["/org/gnome/desktop/interface/font-name"], "'Inter 11'");
    assert_eq!(
        look["/org/gnome/desktop/interface/document-font-name"],
        "'Inter 11'"
    );
    assert_eq!(
        look["/org/gnome/desktop/interface/monospace-font-name"],
        "'JetBrains Mono 11'"
    );
    assert_eq!(
        look["/org/gnome/desktop/wm/preferences/titlebar-font"],
        "'Inter Bold 11'"
    );
    assert_eq!(
        k[&KeyModule::Sounds]["/org/gnome/desktop/sound/theme-name"],
        "'macos-like'"
    );
    let apps = &k[&KeyModule::Apps];
    assert_eq!(apps["/org/gnome/Ptyxis/use-system-font"], "false");
    assert_eq!(apps["/org/gnome/Ptyxis/font-name"], "'JetBrains Mono 12'");
}
