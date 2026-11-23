# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- deskd serves clipboard content, history changes and the mixer only to GNOME Shell and its own
  binaries (same user, outside any sandbox); Flatpak apps with session-bus access and tools such as
  `gdbus` get `AccessDenied` ([ADR 0008](docs/adr/0008-deskd-serves-content-to-trusted-callers.md)).
- deskd's bus name can no longer be taken over (only `deskd --replace`, for development, is
  replaceable), and the bridge pushes and pastes nothing unless the name's owner is the installed
  deskd.
- Clipboard: image names in `history.json` are validated (a crafted name could read or delete files
  outside deskd's directory); SVG is no longer captured and images must be PNG, JPEG, GIF or WebP by
  their bytes; more password-manager hints are honoured; text that looks like a password or token is
  kept in memory only for 120 s and never saved; the bridge's own paste is not recorded again.

### Added

- Skin-aware on-screen displays (stage 3): with the `windows11` and `macos` skins the bridge draws
  the volume, mute, microphone, brightness and keyboard backlight OSD itself (Windows 11: acrylic
  bar at the bottom centre; macOS: translucent centred square or a pill), click-through, with a
  distinct segment, marker and text above 100 %, high-contrast outline and number, reduced-motion
  support and a polite screen-reader announcement. Settings `io.github.OlehDzhumyk.Desk.Osd`
  (`enabled`, `show-percentage`, `timeout-ms`, `position`) and an «Екранні індикатори» page in the
  window. With the `default` skin, `enabled` off or a manager another extension patched, GNOME's
  own OSD stays untouched; skin switch and `disable()` restore it exactly.
- Colour picker engine (stage 3): `deskd` hosts `io.github.OlehDzhumyk.Desk.ColorPicker` at
  `/io/github/OlehDzhumyk/Desk/ColorPicker` (`Record`, `List`, `Remove`, `Clear`, `Format`,
  `Contrast`, signal `Changed`), formatting with `theme_tokens::picker` and keeping the newest
  `history-size` picks in `~/.local/share/deskd/colors.json` (0600; nothing is written with
  `history-size=0`). Settings `io.github.OlehDzhumyk.Desk.ColorPicker` (`enabled`, `format`, `zoom`,
  `history-size`, `copy-on-pick`, `color-picker-shortcut`), shortcut `color-picker-toggle`
  (Shift+Super+C; Ctrl+Shift+Super+C in the macOS theme) and a «Піпетка» page in the window.
- Colour picker in the shell (stage 3): the bridge binds `color-picker-shortcut` (normal mode only, so
  not on the lock screen) and opens an eyedropper with a crosshair, a loupe, a live swatch and the
  value in the chosen format; click or `Enter` copies it with a «Copied …» toast, `Esc` cancels,
  arrows move the pointer by 1 px (`Shift`: 10 px), `F` cycles the format, `+`/`-` or scroll change
  the zoom. `H` or a right click opens the history panel (copy, `Delete`, format buttons, «Clear»,
  WCAG contrast of the two newest picks), styled for the `windows11`, `macos` and `default` skins.
- Flatpak-compatible GTK copies: `themes apply` now copies all theme `gtk4/` and `gtk3/` files
  (including `assets/` subdirectories for `macos` and `windows11`) into `~/.config/gtk-{4,3}.0`
  instead of symlinking them, so Flatpak apps with `xdg-config/gtk-4.0:ro` can read them.
  `themes/lavender` now ships `flatpak.conf`, and `themes doctor --fix` migrates legacy GTK
  symlinks to copies and checks Flatpak read access to `~/.config/gtk-4.0`.
- Focus modes (`focus`): `deskd` hosts the focus engine (`io.github.OlehDzhumyk.Desk.Focus` at
  `/io/github/OlehDzhumyk/Desk/Focus`) with built-in Do not disturb, Work and Sleep modes plus custom
  ones (allowed apps, mute sounds, weekday schedules with midnight wrap), timed starts and focus
  sessions with a break. Ending a mode, a restart or a crash restores exactly the
  `show-banners`/`event-sounds` value it changed; turning GNOME's own do-not-disturb off ends the
  mode ([ADR 0009](docs/adr/0009-deskd-writes-do-not-disturb-keys.md)). Entry points: a «Фокус»
  quick settings tile and panel icon for every skin (focus sessions with a countdown in
  `windows11`), `themes focus`, a «Фокус» page in the window, and a `themes doctor` line. Settings
  `io.github.OlehDzhumyk.Desk.Focus` (`enabled`, `active-mode`, `summary-on-end`, `session-length`,
  `session-break`).
- Theme from wallpaper (stage 2): live "accent from wallpaper" in `deskd` (settings
  `accent-from-wallpaper`, `accent-source`; follows `org.gnome.desktop.background` with a 500 ms
  debounce and sets `accent-color` to the nearest GNOME named accent, decoding the image in a
  `themes tokens seed` child process; turning it off restores the previous accent), D-Bus
  `Appearance.WallpaperAccent()`, `themes tokens seed PATH`, «Create theme from wallpaper…» in the
  window (seed and light/dark preview with contrast numbers, name, «Apply»), an «Accent from
  wallpaper» switch in the quick settings «Theme» menu, and a `themes doctor` line while it is on.
  `themes tokens from-image` now reads slideshow XML (first image), refuses images under 16×16,
  notes near-monochrome images, and with `--force` moves the old theme to `trash/`.
- Theme from wallpaper, live palette and skin pickers: token themes with a generated libadwaita
  stylesheet get the wallpaper seed's full light/dark palette while "accent from wallpaper" is on
  (`themes tokens overlay SEED | --remove`, run by `deskd`; recorded in `state.json`, reverted on
  off, theme switch, `themes reset` and `themes doctor --fix`; ADR 0010). D-Bus
  `WallpaperAccent()` also returns the theme whose palette follows. The quick settings «Theme»
  menu shows a dot grid with a rainbow «From wallpaper» dot (`macos`), an «Accent colour:
  Automatic / Manual» drop-down (`windows11`) or the switch (`default`).
- Emoji and symbol picker (`docs/spec/features/emoji.md`): offline `deskd` engine
  (`io.github.OlehDzhumyk.Desk.Emoji`, `crates/deskd/data/emoji.tsv`) with 300+ emoji, Unicode
  symbols and text faces searchable in English and Ukrainian, Fitzpatrick skin-tone modifiers, and
  0600 recents persistence (`~/.local/share/deskd/emoji.json`). In `desk-bridge`, the Windows 11
  Win+V / Win+. flyout now has live «Емодзі» and «Символи» tabs, while `macos` (`Ctrl+Super+Space`)
  and `default` (`Super+.`) open a floating keyboard-first picker near the caret; insertion uses
  `application/x-desk-internal` and restores the previous clipboard text so clipboard history is
  untouched. Settings `io.github.OlehDzhumyk.Desk.Emoji` and shortcut `emoji-toggle`.
- Automatic light/dark appearance (`auto-dark`): `deskd` now hosts the `appearance` engine
  (`io.github.OlehDzhumyk.Desk.Appearance` at `/io/github/OlehDzhumyk/Desk/Appearance`) with a
  shared one-shot wake scheduler (`crates/deskd/src/schedule.rs`) that reacts to suspend/resume
  and timezone changes. Supports `manual`, `light`, `dark`, `auto-sun` (offline NOAA solar
  sunrise/sunset with a built-in world and Ukrainian city table, decimal coordinates, or timezone
  fallback), and `auto-time` (fixed 24-hour times), plus manual override until the next transition
  and settings `io.github.OlehDzhumyk.Desk.Appearance` (`mode`, `dark-from`, `light-from`,
  `location`, `twilight-offset`, `override-until-next`) with an «Appearance» page in the window.
- Logo menu in `desk-bridge` for the `macos` skin (replaces the third-party `logomenu@aryan_k`
  extension): system menu on the left of the top bar with About This System, System Settings,
  Applications, Force Quit (with confirmation dialog), Sleep, Restart, Shut Down, Lock Screen and
  Log Out, plus settings `io.github.OlehDzhumyk.Desk.LogoMenu` (`enabled`, `hide-activities`) and a
  «Logo menu» page in the window. `logomenu@aryan_k` is retired during apply and migration.
- Settings hub «Робочий стіл» ("Desktop"): renamed the window, `.desktop` and metainfo entries from
  Themes to Desktop; replaced the top view switcher with a responsive `adw::NavigationSplitView`
  sidebar organizing Appearance (Themes, Mix), Features (all registry feature pages), Application
  (Keyboard shortcuts, General, System) and About (Credits); added `Ctrl+F` search across all pages
  and settings (matching both translated text and English msgids, with row jump and reduced-motion
  highlight); added per-page «Reset page», an inline System page (doctor check/fix, baseline info,
  «Reset all»), and an editable string-list widget for `excluded-apps` and `hide-apps`.
- Volume mixer: one slider (and mute, «Play on…») per application that plays sound, output and input
  device switching, in the quick settings sound menu, skinned for Windows 11 («Мікшер гучності»,
  Ctrl+Super+V), macOS (Control Center «Звук» page with a collapsed «Програми» disclosure) and Fedora
  (GNOME's chooser plus «Програми»). deskd follows PipeWire through events only and exposes
  `io.github.OlehDzhumyk.Desk.Mixer`; settings `io.github.OlehDzhumyk.Desk.Mixer` (shortcut, entry point,
  silent apps, optional per-app volume memory, arrow-key step). Building deskd now needs
  `pipewire-devel` and `clang-devel` (cargo feature `pipewire`, on by default).
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

- Clipboard: `image-budget-mb` (default 256 MiB) caps all session images together; the oldest
  unpinned images are dropped first, and nothing is written when `$XDG_RUNTIME_DIR` has less than
  32 MiB left after the image.
- Themes: `flatpak.conf` lines are limited to an allowlist (read-only GTK, theme and icon folders,
  `GTK_THEME`, an app id); other lines are refused with a warning in `apply`, `apply --dry-run` and
  `themes doctor`. Dconf keys that run commands (custom shortcut `command`, terminal `exec`,
  autostart) are listed in `apply --dry-run` and need `--yes` or a dialog in the window the first
  time; the yes is remembered per key and command. The bundled windows11 shortcut asks once.
  The D-Bus service refuses unapproved command keys on `Apply` (`AccessDenied`), exposes `PendingCommands`, and the quick-settings toggle asks before calling `ApplyConfirmed`. See `docs/theme-contract.md`.
- `docs/maintainer.md`: releases, CI, build environment, pinned versions, known workarounds.
- CI: a check that no `.rs` file is outside every crate's module tree
  (`tools/check-orphan-rs.sh`), `tools/update-image-digests.sh`, `tools/ego-pin.sh`.

### Changed

- `deskd` start-up is split into one engine per feature behind an `Engine` trait (register, start,
  shutdown in reverse order); the shared resume and time zone watcher wakes every engine that asks.
  No behavior changes: the same D-Bus paths, `Features`, bus name handling and shutdown writes.
- Stage 1.5 (foundations) is done: the spec tree marks `lavender` Implemented (every generated
  output is pinned by snapshot tests), the Stage 2 features by their own specs, and records the
  follow-ups (report-only performance job, no automated AT-SPI audit, `dconf` tool still used).
  Stale spec gaps fixed by the bridge i18n, the accessibility work, ADR 0006 and the settings hub
  are marked resolved, and `macos-glass` is no longer described as bundled.
- Release builds unwind on panic (`panic = "unwind"`, about 0.5 MB more per binary) so that `deskd`
  survives a panic in one handler or thread; `deskd.service` limits restarts to 5 in 60 seconds.
- The minimum Rust version is 1.100 (it was 1.89 on paper but 1.92 in fact): `rust-version` equals
  `rust-toolchain.toml`, and CI checks that they stay equal.
- Container images in CI and the Containerfile are pinned by digest; Dependabot watches `docker`.
- Selected clippy lints are on for all crates: `cast_possible_truncation`, `cast_sign_loss`,
  `too_many_lines` (150), `map_unwrap_or`.
- Extension ZIPs from extensions.gnome.org are verified by SHA-256 before they are unpacked;
  `install-extensions.sh --latest` is removed (it could not be verified), and `bootstrap.sh --rustup`
  verifies a pinned `rustup-init` instead of piping `curl` into `sh`.

### Fixed

- `themes apply` is transactional: an intent journal in `state.json` (`pending`) is saved before
  every step with side effects. A failing step rolls back the steps done so far (dconf values,
  extension list, GTK links and copies, flatpak overrides, token links) and keeps the old
  selection; GTK backups are recorded before the user's file is moved, so `themes reset` always
  gets it back. After a crash the next apply or `themes doctor --fix` completes or rolls back the
  interrupted apply, and `themes doctor` reports it.
- Experimental themes need confirmation in the library and the D-Bus service too: `Apply` refuses
  them (`AccessDenied`), the new `ApplyConfirmed` applies them after the user said yes; the
  quick-settings toggle applies nothing when it cannot ask the service whether a theme is
  experimental (ADR 0006).
- Theme hooks have a time limit (30 s, `DESKTOP_THEMES_HOOK_TIMEOUT`) and bounded output.
- The Themes D-Bus service no longer blocks its executor during an apply, exits when the bus
  connection closes, counts running operations, and watches `state.json` instead of polling it.
- The fixed delays around extension toggles (the GNOME Shell 51.1 crash workaround) are replaced by
  waits on the dconf change notification and `ExtensionStateChanged`, with an upper bound.
- Many dconf keys are reset with one change instead of one `dconf reset` process per key; a
  command's input is written from its own thread, so a child that fills its output first cannot
  deadlock.

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
