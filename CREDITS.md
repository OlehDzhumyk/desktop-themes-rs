# Credits and third-party licences

desktop-themes-rs itself is GPL-3.0-or-later. It **does not redistribute** the projects below:
`scripts/` downloads them from upstream at pinned revisions onto the user's machine. Licences are
those stated by the upstream projects (checked against their repositories); consult the upstream
`LICENSE` file for the authoritative text.

## GTK / GNOME Shell themes, icons, cursors, wallpapers

| Project | Used for | Pinned | Licence |
|---|---|---|---|
| [WhiteSur GTK theme](https://github.com/vinceliuice/WhiteSur-gtk-theme) (vinceliuice) | macOS look: GTK 3/4 and shell theme, libadwaita CSS | `d578265` | MIT |
| [WhiteSur icon theme](https://github.com/vinceliuice/WhiteSur-icon-theme) | macOS look: icons | `73d8040` | GPL-3.0 |
| [WhiteSur cursors](https://github.com/vinceliuice/WhiteSur-cursors) | macOS look: cursors | `e190baf` | GPL-3.0 |
| [WhiteSur wallpapers](https://github.com/vinceliuice/WhiteSur-wallpapers) | macOS look: wallpapers (2k) | `5c1d7ca` | MIT |
| [Fluent GTK theme](https://github.com/vinceliuice/Fluent-gtk-theme) | Windows 11 look: GTK 3/4 and shell theme (`round` tweak) | `7a49a46` | GPL-3.0 |
| [Fluent icon theme and cursors](https://github.com/vinceliuice/Fluent-icon-theme) | Windows 11 look: icons, cursors | `c8a244f` | GPL-3.0 |

## Fonts

| Font | Used for | Version | Licence |
|---|---|---|---|
| [Inter](https://github.com/rsms/inter) (Rasmus Andersson) | macOS look UI font | 4.1 | OFL-1.1 |
| [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) | macOS look monospace font | 2.304 | OFL-1.1 |
| [Cascadia Code / Cascadia Mono](https://github.com/microsoft/cascadia-code) | Windows 11 terminal font | 2407.24 | OFL-1.1 |
| Noto Sans (system font, `google-noto-vf`) | Windows 11 UI font | distribution | OFL-1.1 |

No Apple (SF) or Microsoft (Segoe) fonts are used.

## GNOME Shell extensions

Downloaded from extensions.gnome.org at the pinned version tags in `scripts/versions.sh`.

| Extension | Author / upstream | Licence |
|---|---|---|
| [User Themes](https://gitlab.gnome.org/GNOME/gnome-shell-extensions) | GNOME | GPL-2.0-or-later |
| [AppIndicator and KStatusNotifierItem Support](https://github.com/ubuntu/gnome-shell-extension-appindicator) | Ubuntu / rgcjonas | GPL-2.0-or-later |
| [Blur my Shell](https://github.com/aunetx/blur-my-shell) | aunetx | GPL-3.0 |
| [Dash to Dock](https://github.com/micheleg/dash-to-dock) | Michele Gaio et al. | GPL-2.0-or-later |
| [Logo Menu](https://github.com/Aryan20/Logomenu) | Aryan Kaushik | GPL-2.0-or-later |
| [Compiz alike magic lamp effect](https://github.com/hermes83/compiz-alike-magic-lamp-effect) | hermes83 | GPL-3.0-or-later |
| [Spotlight](https://github.com/itsnin/spotlight) | itsnin | GPL-3.0-or-later |
| [Dash to Panel](https://github.com/home-sweet-gnome/dash-to-panel) | jderose9 et al. | GPL-2.0-or-later |
| [ArcMenu](https://gitlab.com/arcmenu/ArcMenu) | ArcMenu team | GPL-2.0-or-later |
| [Tiling Shell](https://github.com/domferr/tilingshell) | Domenico Ferraro | GPL-3.0-or-later |
| [Desktop Icons NG (gtk4-ding)](https://gitlab.com/smedius/desktop-icons-ng) | Sergio Costas, Marco Trevisan, Sundeep Mediratta et al. | GPL-3.0-or-later |
| [Liquid Glass](https://github.com/ryohsuke1231/liquid-glass) (commit `9ec82ed`, from the repository, not extensions.gnome.org) | ryohsuke1231 | MIT |

## Code adapted into this repository

| Project | Used for | Licence |
|---|---|---|
| [Clipboard Indicator](https://github.com/Tudmotu/gnome-shell-extension-clipboard-indicator) (Yotam Bar-On / Tudmotu and contributors) | clipboard capture, paste-on-select and image handling, ported to TypeScript in `bridge/src/clipboard/watcher.ts` and `paste.ts` (attribution in the file headers; see docs/features.md) | MIT |

| [ddterm](https://github.com/ddterm/gnome-shell-extension-ddterm) (Aleksandr Mezin and contributors) | end-to-end test harness: `tests/e2e/harness/logparser.py` and `github_annotations.py` copied unchanged with their SPDX headers (commit `dacfd3f`); the approach of `harness/shell.py` and `harness/hook.js` (shell in a systemd container, `--unsafe-mode` + Eval, helper module loaded into the shell) after `tests/fixtures.py`, `shelldbus.py`, `shellhook.js`; the CI job after `.github/workflows/ci.yml` (CC0-1.0) | GPL-3.0-or-later |
| [ddterm/gnome-shell-image](https://github.com/ddterm/gnome-shell-image) (Aleksandr Mezin, Simon Schneegans) | base image of the end-to-end tests, `ghcr.io/ddterm/gnome-shell-image/fedora-45` pinned by digest in `tests/e2e/Containerfile` (pulled, not redistributed) | MIT |

`patches/spotlight@nin/` contains two small unified diffs against Spotlight (GPL-3.0-or-later), kept
under the same licence; see [patches/README.md](patches/README.md).

Licences were read from the LICENSE/COPYING files and source headers of the installed copies; the
upstream repository is authoritative.

## Libraries (Rust crates)

[libadwaita](https://crates.io/crates/libadwaita) and [gtk4](https://crates.io/crates/gtk4)
(gtk-rs, MIT), [clap](https://crates.io/crates/clap) (MIT OR Apache-2.0),
[serde](https://crates.io/crates/serde) and [serde_json](https://crates.io/crates/serde_json)
(MIT OR Apache-2.0), [zbus](https://crates.io/crates/zbus) (MIT), [gio](https://crates.io/crates/gio)
(gtk-rs, MIT), [futures-util](https://crates.io/crates/futures-util) (MIT OR Apache-2.0),
[toml](https://crates.io/crates/toml) (MIT OR Apache-2.0, design tokens) and their
transitive dependencies, see `Cargo.lock` and `cargo tree`. GTK 4 and libadwaita themselves are
LGPL-2.1-or-later.

## Build tools of the shell bridge (npm, not shipped)

| Project | Used for | Licence |
|---|---|---|
| [ts-for-gir / @girs](https://github.com/gjsify/ts-for-gir) (`@girs/gnome-shell` 51, `@girs/gjs`) | TypeScript types for GJS and GNOME Shell 51 | MIT |
| [TypeScript](https://github.com/microsoft/TypeScript) | compiler (`tsc`) | Apache-2.0 |
| [ESLint](https://github.com/eslint/eslint) and [typescript-eslint](https://github.com/typescript-eslint/typescript-eslint) | linting | MIT |

## Terminal palettes

The macOS and Windows 11 themes reference Ptyxis' own `Pro` and `campbell` palettes by name
(`[terminal] builtin` in their tokens.toml); [Ptyxis](https://gitlab.gnome.org/chergert/ptyxis) is
GPL-3.0-or-later and nothing of it is copied. The `Lavender` palette is original. The palette file format
written by the token generator follows Ptyxis' `ptyxis-palette.c`.

## Design inspiration

The macOS, macOS Liquid Glass and Windows 11 themes are inspired by the look and behaviour of
Apple's macOS and Microsoft's Windows 11. No Apple or Microsoft assets are included or downloaded:
no logos, wallpapers, fonts or sounds. "macOS", "Apple", "Windows" and "Segoe" are trademarks of
their respective owners; this project is not affiliated with or endorsed by them.

## Original assets

* UI sound themes `macos-like` and `windows11-like`: synthesized by `tools/synth-sounds.py`
  (additive synthesis, no samples). CC0-1.0.
* Windows 11 "bloom" wallpapers: generated by `tools/generate-bloom-wallpaper.py`. CC0-1.0.
* Lavender theme (tokens, terminal palette) and the gradient wallpapers generated from tokens:
  original. The generated GNOME Shell layer `@import`s the shell's own stylesheet from its resource
  at run time (nothing copied); `crates/theme-tokens/src/adwaita.rs` lists libadwaita 1.10's default
  colour values (LGPL-2.1-or-later) as facts for validation.

## Developer workflow

| Project | Used for | Licence |
|---|---|---|
| [T3 Code](https://github.com/pingdotgg/t3code) (Ping.gg) | agent workflow conventions adapted (not copied): `AGENTS.md` structure, "hit every surface" checklist, the agent context budget check | MIT |
| [just](https://github.com/casey/just), [cargo-nextest](https://github.com/nextest-rs/nextest), [ast-grep](https://github.com/ast-grep/ast-grep), [insta](https://github.com/mitsuhiko/insta), [gitleaks](https://github.com/gitleaks/gitleaks) | developer tooling, not redistributed | CC0-1.0, Apache-2.0 / MIT, MIT, Apache-2.0, MIT |
