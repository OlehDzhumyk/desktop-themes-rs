# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `themes doctor` checks the GSettings schemas: ours are installed and compiled, and every installed
  extension that ships `schemas/*.gschema.xml` has `gschemas.compiled`; `--fix` compiles it.
- Shortcut conflict detection also sees GNOME's own defaults (schema defaults of the shell, window
  manager, media-keys and mutter keybinding schemas, read through a private in-memory GSettings
  backend), so e.g. Super+V of the notification list clashes with a clipboard shortcut even when
  dconf does not set it. Clashes between two system keys are no longer reported by `themes apply`.

- The lock screen settings (`lockscreen-layout`) are in the settings registry: generated schema, a
  «Lock screen» page in the window, docs/settings.md and Ukrainian texts. The clock position, font
  weight and colours are validated choices and CSS colours (`#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`,
  `rgba()`) instead of free strings; the extension, `themes apply` (values of a theme) and
  `themes doctor` (values in dconf) refuse anything else. Keys, types and defaults are unchanged, so
  stored values keep working; a value outside the accepted set (for example a colour name such as
  `red`, or a weight such as `350`) is ignored and the standard look is used.
- CI: typos, cargo-machete and zizmor jobs (`hygiene.yml`) and a smoke test of `data/install.sh` in a
  throw-away HOME (`tools/install-smoke.sh`).

- End-to-end tests of the bridge, deskd and `themes` in GNOME Shell 51 running headless in a podman
  container (`tests/e2e`, `just e2e`, CI job `e2e.yml`; ADR 0007): enable on every skin, clipboard
  copy → history → shortcut → paste, shortcut overrides and conflicts, live `themes apply` +
  `themes doctor`, the experimental-theme confirmation, and nothing left behind after disable or
  logout. Harness approach and two modules from ddterm (see CREDITS.md).

### Fixed

- Bridge: disabling it left the menus of its quick settings toggles (theme, clipboard), their items
  and a GSettings handler on the stage (GNOME Shell 51 does not destroy an item's menu with the
  item); switching the skin or language added another set each time.
- Bridge: a D-Bus reply or signal arriving after disable or during logout touched already destroyed
  actors ("has been already disposed" critical messages in the journal); the deskd proxy set-up is now
  cancelled on disable.
- `data/install.sh` compiles the GSettings schemas of repository extensions after copying them
  (`lockscreen-layout` failed to load with a missing `gschemas.compiled` after a reinstall).

## [0.3.0] - 2026-11-22

### Changed

- desk-bridge: strict TypeScript (`noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`,
  `noImplicitReturns`, `noFallthroughCasesInSwitch`, `noPropertyAccessFromIndexSignature`), target
  ES2025 (gjs 1.90 / SpiderMonkey 140), TypeScript 7 for the build and type check, typescript-eslint
  `strictTypeChecked` + `stylisticTypeChecked`, no `any`; typed D-Bus proxies. eslint 10. Replaces
  the Dependabot bump of the bridge group.

### Fixed

- rustfmt and clippy are clean and blocking in CI now; the minimum Rust version is 1.89
  (`File::try_lock` was already used).
- `themes apply` exits 4 when the automatic rollback turned an extension off (was 0).
- `themes doctor` checks deskd (service file, running, same version).
- Snapshots include the skin and the clipboard shortcut of the look module.
- deskd writes the clipboard history debounced (2 s quiet, 10 s at most) and flushes it on
  SIGTERM/SIGINT/SIGHUP, so nothing is lost on logout and bursts of copies cost one write.
- Clipboard history and thumbnail cache use a stable hash (FNV-1a) instead of `DefaultHasher`;
  an existing `history.json` is migrated at load (store version 2). The `no-default-hasher`
  ast-grep rule is an error now.

### Added

- Keyboard shortcuts in the settings registry: one «Keyboard shortcuts» page (`themes gui --page
  shortcuts`) and `themes shortcut list|set|reset`; a theme only supplies the default, the user's
  choice wins and survives theme switches, "reset to theme default" per shortcut, a taken
  accelerator is refused. New schema `io.github.OlehDzhumyk.Desk.Shortcuts`; existing keys unchanged.
- `themes apply` detects one accelerator owned twice (mixing look and behavior, e.g. `Super+Space`
  or `Super+V`) and gives it to the feature, with a warning.
- Continuous integration (`.github/workflows/ci.yml`): rustfmt and clippy (report only), tests,
  `cargo deny`, the desk-bridge build (`tsc`, eslint), shellcheck, GSettings schema, desktop file and
  D-Bus XML validation, actionlint and an offline documentation link check.
- Headless GNOME Shell workflow (`.github/workflows/headless.yml`): the clipboard end-to-end test in a
  Fedora 45 container; optional performance run.
- Performance harness `tools/perf/` with budgets (`tools/perf/budgets.toml`): deskd memory, idle
  CPU and wakeups, clipboard panel latency, `themes list` and `themes apply` times, shell memory
  cost of the bridge.
- Release workflow (`.github/workflows/release.yml`) and `scripts/release.sh`.
- `scripts/uninstall.sh`: removes everything bootstrap installed, after `themes reset`, with
  `--dry-run` and `--purge`.
- `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, issue and pull request templates,
  Dependabot configuration.
- `tools/headless/ci-env.sh`, `HS_TOOLBOX=none` and `HS_NO_SHOTS=1` to run the headless harness in a
  plain container.

## [0.2.0] - 2026-11-21

Stage 1: the desktop features daemon, the shell bridge and clipboard history with per-theme skins.

### Added

- `deskd` (`io.github.OlehDzhumyk.Desk`), a session daemon hosting the clipboard history engine:
  text and images with size caps, pins, search, delete, clear, private mode, persistence under
  `~/.local/share/deskd/clipboard` (images opt-in), password-manager hints and excluded apps.
- `desk-proto`: shared D-Bus names, interface XML, the `ClipItem` type and a client proxy.
- `desk-bridge@desktop-themes-rs`, one GNOME Shell extension written in TypeScript: the quick
  settings "Theme" toggle (absorbing `themes-quick`), clipboard capture through `Meta.Selection`,
  paste-on-select, and three skins chosen by the `skin` GSettings key: `windows11` (Win+V flyout),
  `macos` (Spotlight-like panel) and `default` (menu). Capture and paste are adapted from
  Clipboard Indicator (MIT).
- GSettings schema `io.github.OlehDzhumyk.Desk` shared by deskd, the bridge and the window.
- "Features" page in the window (clipboard settings, credits).
- `themes doctor` and `themes apply` detect GNOME's crash protection (`disable-user-extensions`) and
  name the last real shell crash and the theme and extensions applied then; the window shows a
  banner that re-enables extensions only after confirmation.
- Theme inheritance: `INHERITS=<id>` in `theme.conf`, with `+uuid` / `-uuid` lines in
  `extensions.txt`.
- Headless test tools with full isolation from the live session (own bus, display, runtime
  directory, dconf database and data directory) and an end-to-end clipboard test.
- `scripts/migrate-from-legacy.sh` for installations of older private builds.
- Architecture, skin and inheritance, features and credits documentation.

### Changed

- The code base is a Cargo workspace: `themes-core` (library), `themes` (CLI, GUI, theme service),
  `desk-proto`, `deskd`.
- `macos-glass` now inherits `macos` (experimental: it crashed GNOME Shell 51.1 on real hardware).
- Bundled themes set the clipboard skin in `desk.dconf`; Clipboard Indicator is no longer enabled
  by any bundled theme.
- `themes doctor` reports `disable-user-extensions` once, read from dconf.

### Removed

- The `themes-quick` extension (replaced by desk-bridge).

## [0.1.0] - 2026-11-21

Initial version: the theme manager.

### Added

- Rust theme manager: library, CLI, GTK 4 / libadwaita window and an on-demand D-Bus theme service.
- Four modules per theme, switchable on their own: `look`, `behavior`, `sounds`, `apps`; live
  switching of shell extensions, baseline snapshot of the user's dconf, automatic rollback of
  extensions that error out, `themes doctor`, `themes reset`, `themes snapshot`.
- Bundled themes: `fedora`, `macos`, `macos-glass` (experimental) and `windows11`; contract files
  only, third-party assets are downloaded from upstream at pinned revisions.
- Own GNOME Shell extensions (`themes-quick`, `lockscreen-layout`) and patches for `spotlight@nin`.
- Original CC0 sound themes and wallpapers generated by `tools/`.
- `scripts/bootstrap.sh` and the individual installers, all with `--dry-run`.
- README, CREDITS and documentation (English, with a Ukrainian user guide).

[Unreleased]: https://github.com/OlehDzhumyk/desktop-themes-rs/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/OlehDzhumyk/desktop-themes-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/OlehDzhumyk/desktop-themes-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/OlehDzhumyk/desktop-themes-rs/releases/tag/v0.1.0
