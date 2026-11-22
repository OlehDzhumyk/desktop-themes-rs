# HANDOFF: stage1.5-tokens (design tokens agent)

Branch `stage1.5-tokens`, worktree `~/Developer/dtr-tokens`. Build/test in toolbox `themes-rs-build`
with `CARGO_TARGET_DIR=$PWD/target`.

## Done
* New crate `crates/theme-tokens` (no GNOME deps): typed `tokens.toml` schema (serde, all optional,
  `deny_unknown_fields`), INHERITS deep-merge, validation (ranges + WCAG AA contrast: 4.5 text,
  3.0 filled controls; only pairs the theme touches), colour math (OKLab/OKLCH, gamut mapping),
  GNOME accent mapping, generators: libadwaita 1.10 CSS (CSS vars + @define-color, light/dark via
  `@media (prefers-color-scheme: light|dark)`), GNOME Shell layer (`@import` of stock resource +
  overrides), Ptyxis palette (format verified in ptyxis-palette.c), gradient wallpaper SVG, dconf keys
  per module; content-hashed cache (`.stamp`, atomic swap); from-image (k-means in OKLab + tonal palette).
* themes-core integration: `src/tokens.rs`; `Theme::settings` merges token keys under *.dconf
  (hand-made wins); generated `gtk4/` used only without a hand-made one; shell theme linked as
  `~/.local/share/themes/desktop-themes-<id>` (look), Ptyxis palette link (apps), state
  `token_links`; apply validates tokens before changing anything, generates before dconf;
  `--dry-run` lists generated files + links; doctor reports token errors and missing cache (`--fix`).
* CLI (`crates/themes/src/tokens_cli.rs`): `themes tokens check|generate [--force] [--repeat N]|keys|from-image`.
* Themes: new `themes/lavender` (theme.conf + tokens.toml only); macos/windows11 fonts, accent,
  sounds, terminal moved to tokens.toml (WhiteSur/Fluent stay base); fedora unchanged (no tokens).
* Tests: theme-tokens 16 unit + 3 snapshot (tests/snapshots/*), themes-core 29 (3 new token tests),
  perf test (ignored). Headless: `tools/headless/tokens-screenshot.sh` + `adw-demo.py`; screenshots
  in `~/.local/share/desktop-themes/screenshots/stage1.5/lavender-*` (not committed).
* Timings: release parse+validate 0.12 ms, render+write 0.16 ms, cache check 0.02 ms (lavender);
  debug binary end-to-end ~1.5 ms. Budget 50 ms.

## In progress / remaining
* (done) docs/design-tokens.md, theme-contract.md, themes.md.
* README.uk.md: optional short note.

## Known issues
* Flatpak apps: generated gtk4 symlinks point into ~/.cache (not exposed by flatpak.conf overrides).
* GTK3 not themed by tokens; shell layer uses one scheme (dark default; St has no media queries).
* Headless env has no settings portal: demo uses ADW_DEBUG_COLOR_SCHEME.
* Linked-button radius restore ignores RTL.
* Quick-settings screenshots show the real network name: off by default (SHOT_QUICK_SETTINGS=1).

## Next commands
```sh
cd ~/Developer/dtr-tokens
toolbox run -c themes-rs-build sh -c 'export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=$PWD/target; cargo test --offline -p theme-tokens -p themes-core'
UPDATE_SNAPSHOTS=1 cargo test -p theme-tokens --test snapshots   # after intended generator changes
HS_TOOLBOX=themes-rs-build tools/headless/tokens-screenshot.sh lavender ~/.local/share/desktop-themes/screenshots/stage1.5
DESKTOP_THEMES_ROOT=<scratch root> target/debug/themes tokens generate lavender --repeat 50
```
