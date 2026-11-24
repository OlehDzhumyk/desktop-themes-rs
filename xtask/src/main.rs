// SPDX-License-Identifier: GPL-3.0-or-later
//! `cargo xtask <task>`:
//!
//! * `gen`: write the artefacts generated from the settings registry (GSettings schema,
//!   typed accessors, docs/settings.md)
//! * `check`: fail when a generated file is out of date (what the tests check, for scripts)
//! * `pot`: extract translatable strings (Rust, TypeScript, .desktop, metainfo) into
//!   po/desktop-themes-rs.pot and merge them into every po/*.po (needs gettext)
//!
//! Compiling the catalogues is done by `data/install.sh` (msgfmt into ~/.local/share/locale, plus
//! the translated .desktop and metainfo files) and by `bridge/assemble.mjs` (the extension's copy).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use desk_settings::{generate as gen_, i18n::DOMAIN};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("repository root")
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let st = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed: {st}"))
    }
}

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if !matches!(name, "target" | "node_modules" | "build") {
                files(&p, ext, out);
            }
        } else if name.ends_with(ext) {
            out.push(p);
        }
    }
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

fn generate(write: bool) -> Result<(), String> {
    let root = repo();
    let mut stale = vec![];
    for (path, content) in gen_::all() {
        let p = root.join(&path);
        if std::fs::read_to_string(&p).ok().as_deref() == Some(content.as_str()) {
            continue;
        }
        if write {
            std::fs::write(&p, content).map_err(|e| format!("{path}: {e}"))?;
            println!("wrote {path}");
        } else {
            stale.push(path);
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "out of date (run `cargo xtask gen`): {}",
            stale.join(", ")
        ))
    }
}

fn pot() -> Result<(), String> {
    let root = repo();
    let tmp = std::env::temp_dir().join(format!("dtr-pot-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let common = |c: &mut Command| {
        c.current_dir(&root)
            .args([
                "--from-code=UTF-8",
                "--add-comments=TRANSLATORS",
                "--sort-by-file",
            ])
            .arg(format!("--package-name={DOMAIN}"))
            .arg("--msgid-bugs-address=https://github.com/OlehDzhumyk/desktop-themes-rs/issues");
    };
    let mut rs = vec![];
    files(&root.join("crates"), ".rs", &mut rs);
    let mut ts = vec![];
    files(&root.join("bridge/src"), ".ts", &mut ts);
    let mut parts = vec![];

    let out = tmp.join("rust.pot");
    let mut c = Command::new("xgettext");
    common(&mut c);
    c.args([
        "-L",
        "Rust",
        "-k",
        "-ktr!",
        "-ktrn!:1,2",
        "-kn_",
        "-kgettext",
        "-kngettext:1,2",
    ])
    .arg("-o")
    .arg(&out)
    .args(rs.iter().map(|p| rel(&root, p)));
    run(&mut c)?;
    parts.push(out);

    let out = tmp.join("ts.pot");
    let mut c = Command::new("xgettext");
    common(&mut c);
    c.args(["-L", "TypeScript", "-k", "-k_", "-kngettext:1,2", "-kN_"])
        .arg("-o")
        .arg(&out)
        .args(ts.iter().map(|p| rel(&root, p)));
    run(&mut c)?;
    parts.push(out);

    let out = tmp.join("data.pot");
    let mut c = Command::new("xgettext");
    common(&mut c);
    c.arg("-o").arg(&out).args([
        "data/io.github.OlehDzhumyk.Themes.desktop.in",
        "data/io.github.OlehDzhumyk.Themes.Update.desktop.in",
        "data/io.github.OlehDzhumyk.Themes.metainfo.xml.in",
    ]);
    run(&mut c)?;
    parts.push(out);

    let pot = root.join("po").join(format!("{DOMAIN}.pot"));
    run(Command::new("msgcat")
        .arg("--use-first")
        .arg("-o")
        .arg(&pot)
        .args(&parts))?;
    let _ = std::fs::remove_dir_all(&tmp);
    println!("wrote {}", rel(&root, &pot));
    let mut pos = vec![];
    files(&root.join("po"), ".po", &mut pos);
    for po in pos {
        run(Command::new("msgmerge")
            .args([
                "--quiet",
                "--update",
                "--backup=none",
                "--no-fuzzy-matching",
                "--sort-by-file",
            ])
            .arg(&po)
            .arg(&pot))?;
        println!("merged {}", rel(&root, &po));
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("gen") => generate(true),
        Some("check") => generate(false),
        Some("pot") => pot(),
        _ => Err("usage: cargo xtask gen | check | pot".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}
