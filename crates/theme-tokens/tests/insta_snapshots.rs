// SPDX-License-Identifier: GPL-3.0-or-later
//! Reviewable snapshots (insta) of the generated libadwaita CSS and the Ptyxis palette for bundled
//! themes. After an intended generator change run `cargo insta review` (or
//! `INSTA_UPDATE=always cargo test -p theme-tokens --test insta_snapshots`) and read the diff.
//! The older `tests/snapshots.rs` keeps byte-exact copies of every output; these add readable
//! diffs for the two outputs people review most.

use std::path::{Path, PathBuf};

use theme_tokens::generate::{self, Target};
use theme_tokens::load_chain;

fn render(id: &str, name: &str) -> Vec<generate::File> {
    render_over(id, name, None)
}

/// `over`: a layer above the theme (the live wallpaper palette).
fn render_over(id: &str, name: &str, over: Option<String>) -> Vec<generate::File> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../themes")
        .join(id)
        .join("tokens.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut layers: Vec<(String, String)> = over
        .map(|o| (theme_tokens::palette::OVERLAY_LAYER.to_string(), o))
        .into_iter()
        .collect();
    layers.push((format!("themes/{id}/tokens.toml"), text));
    let loaded = load_chain(&layers).unwrap();
    let target = Target {
        theme_id: id.into(),
        theme_name: name.into(),
        home: "/home/user".into(),
        cache_dir: PathBuf::from(format!("/home/user/.cache/desktop-themes/{id}")),
        handmade_gtk4: false,
        handmade_shell_theme: false,
    };
    generate::render(&loaded.tokens, &target)
}

fn file(outputs: &[generate::File], suffix: &str) -> String {
    outputs
        .iter()
        .find(|f| f.rel.ends_with(suffix))
        .unwrap_or_else(|| panic!("no generated file ending in {suffix}"))
        .contents
        .clone()
}

macro_rules! snap {
    ($name:expr, $value:expr) => {
        insta::with_settings!({ snapshot_path => "insta", prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!($name, $value);
        });
    };
}

#[test]
fn lavender_libadwaita_css() {
    snap!(
        "lavender_gtk_css",
        file(&render("lavender", "Lavender"), "gtk.css")
    );
}

#[test]
fn lavender_ptyxis_palette() {
    snap!(
        "lavender_ptyxis_palette",
        file(&render("lavender", "Lavender"), ".palette")
    );
}

#[test]
fn macos_libadwaita_css() {
    snap!("macos_gtk_css", file(&render("macos", "macOS"), "gtk.css"));
}

/// AC-WALL-07/08: the live wallpaper palette over a token theme (only colours change).
#[test]
fn lavender_wallpaper_overlay_css() {
    let seed = theme_tokens::color::Rgba::parse("#2a7f62").unwrap();
    snap!(
        "lavender_wallpaper_overlay_gtk_css",
        file(
            &render_over(
                "lavender",
                "Lavender",
                Some(theme_tokens::palette::overlay_toml(seed))
            ),
            "gtk.css"
        )
    );
}
