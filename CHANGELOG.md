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
- The theme service (`io.github.OlehDzhumyk.Themes`) applies themes, reports pending commands and
  runs `Doctor` only for GNOME Shell and our own binaries, and its bus name can no longer be taken
  over (zbus requested it replaceable by default); `List`, `Current` and `Experimental` stay open
  ([ADR 0008](docs/adr/0008-deskd-serves-content-to-trusted-callers.md)).
- deskd keeps at most 4 MiB of the OCR engine's output and 64 KiB of its error output; a runaway
  engine can no longer grow deskd's memory without bound.
- deskd writes its private files (clipboard history, images, recents, the wallpaper state) through
  a new `0600` temporary file with a unique name (`O_EXCL`) and an atomic rename; it never writes
  through a file or link left at the old fixed `.tmp` name, and the wallpaper state is no longer
  written in place.
- desk-shell checks the owner of deskd's bus name like the bridge (same user, same root, the
  `deskd` next to it or `/usr/bin/deskd`) and sends calls only to that verified unique name; the
  connection is cached and checked again when the owner changes.

### Added

- Update to the latest git version in one click: the launcher «Update Desktop Themes» («Оновити
  теми») in the app grid, and the same action on the Desktop icon, opens a terminal with
  `themes update --pause`. `themes update` runs `scripts/update.sh` of the checkout that
  `data/install.sh` now records in `~/.local/share/desktop-themes/repo-path`, after checking that the
  checkout is clean and not diverged; `--check` fetches and lists the new commits, `--dry-run` and
  `--json` work as elsewhere. `themes doctor` reports a recorded checkout that is missing or dirty.
  Git checkout installs only: the RPM and release archives leave the launcher and action out. A
  button in the window follows after the GTK 4 settings hub.
- desk-shell: built-in system tray (`org.kde.StatusNotifierWatcher` / `StatusNotifierItem` icons in
  the taskbar with left/right/middle click and ARGB32 pixmap support, degrading silently if another
  watcher owns the name), built-in `org.freedesktop.Notifications` daemon with Windows 11 Fluent
  `Layer::Overlay` toast popups, `Desk.Focus` Do Not Disturb suppression (still showing critical
  notifications), Notification History with per-item dismiss and "Clear all" inside the
  Clock/Calendar flyout, and an unread indicator dot on the taskbar clock; `Win+Shift+S` in the
  Windows 11 labwc config now invokes `desk-shell --capture-region`.
- Packaging foundation: `packaging/desktop-themes-rs.spec` for Fedora 44/45/rawhide (core
  `desktop-themes-rs` plus `desktop-themes-rs-labwc-session` and `desktop-themes-rs-gnome-bridge`
  subpackages), `@BIN@` templating in `data/deskd.service` and `data/themes-doctor.service` so
  system (`/usr/bin`) and user (`~/.local/bin`) installs share the same units, enriched AppStream
  metadata (`<releases>`, `<categories>`, `<branding>`, `<requires>`) with strict
  `appstreamcli validate --no-net --explain` enforcement in CI, and packaging documentation in
  [docs/packaging.md](docs/packaging.md).
- desk-shell: live token CSS reloading (`$XDG_CONFIG_HOME/desk-shell/desk-shell.css` loaded at
  `APPLICATION + 1` priority and watched via `gio::FileMonitor`, clearing overrides cleanly when
  removed) and live system status indicators on the taskbar tray pill and in Quick Settings:
  UPower battery icon and footer badge (with low-battery warning at ≤ 15 %), NetworkManager Wi-Fi
  icon and toggle, BlueZ Bluetooth adapter toggle, and sysfs/`systemd-logind` display brightness
  slider watched live for hardware key changes (all hidden silently when hardware or services are
  absent).
- `themes-core`: wire `theme-tokens` extra generators (`ExtraTarget::LabwcTheme`,
  `ExtraTarget::DeskShellCss`) into `apply`, `LabwcBackend`, `OtherBackend` and transaction
  rollback. Applying a token-based `look` theme (honouring `INHERITS`, module mixing and
  `--dry-run`) writes the generated `openbox-3/themerc` (on `LabwcBackend`) and
  `$XDG_CONFIG_HOME/desk-shell/desk-shell.css` (on `LabwcBackend` and other Wayland desktops)
  atomically, and reconfigures a live labwc only when changed. Applying a theme without
  `tokens.toml`, running `themes reset`, or rolling back a failed apply removes the generated
  `desk-shell.css` and restores the bundled `themerc`. GNOME behaviour and JSON output stay
  unchanged.
- Windows 11 labwc session: `autostart` starts the first installed polkit authentication agent and
  falls back to `mako` or `dunst` when `org.freedesktop.Notifications` has no owner;
  `desk-win11-session` keeps a token-generated `themerc` across logins and stops `deskd.service`
  before clearing the Wayland environment on exit; `data/install.sh --no-gnome-bridge` installs
  the core programs, `desk-shell` and the labwc session without `desk-bridge@desktop-themes-rs` or
  `npm`.
- desk-shell: Windows 11 on-screen display (`Layer::Overlay` pill above the taskbar) that follows
  `deskd`'s `Mixer` volume and mute changes (quiet on start-up) and shows toasts via `--show-osd`;
  Emoji picker (`Win+.`, `--toggle-emoji`, or the header switch in the clipboard history) backed by
  `deskd`'s `Emoji` engine with EN/UK search and categories; and fast one-shot capture, OCR, and
  colour picker tools (`--capture-region` / `Win+Shift+S`, `--ocr-region` / `Win+Shift+T`,
  `--pick-color` / `Win+Shift+C`) wired to `deskd` (`Capture`, `Ocr`, `ColorPicker`) and Quick
  Settings tiles.
- `themes-core` gets the `DesktopBackend` boundary of [ADR 0011](docs/adr/0011-wayland-agnostic-rust-core.md):
  the desktop comes from `XDG_CURRENT_DESKTOP` (GNOME, labwc, other Wayland). In a labwc session
  `themes doctor` checks labwc, desk-shell, deskd, `libgtk4-layer-shell.so.0`, swaylock, swaybg and
  the session config instead of GNOME Shell extensions, and its report gains `"desktop": "labwc"`.
  `themes apply` there installs the skin's labwc theme and runs `labwc --reconfigure`. On GNOME
  nothing changes, and the report has no `desktop` field.
- desk-shell: the taskbar shows the running windows (Wayland `zwlr_foreign_toplevel_management_v1`
  on labwc, sway, Hyprland, niri and Wayfire, read by a small built-in client, with no new
  dependency). A short bar under an icon shows that the application runs, a long accent bar that
  it has the focus. A pinned application gets the bar on its own icon; other applications get a
  button after the pins, with the icon of their desktop entry (else a generic icon) and the window
  titles as tooltip. A click brings the window forward or minimizes it, and a right click (or
  `Menu`, `Shift+F10`) opens Restore/Minimize, Maximize/Unmaximize and Close. Without the protocol
  (GNOME, KWin, `--windowed`) the taskbar shows the pins only.
- desk-shell: one taskbar on every monitor (`gtk_layer_set_monitor`); taskbars are added and
  removed when monitors are plugged in or out. The Start, quick settings and clipboard buttons of
  every taskbar show the state of their flyout.
- Clipboard history without GNOME Shell: deskd captures the clipboard itself on compositors with
  `ext-data-control-v1` or `wlr-data-control-unstable-v1` (labwc, sway, niri, Hyprland, KWin,
  COSMIC), with the bridge's rules: password-manager hints and our own paste are never read, reads
  are bounded in size and time. Under GNOME Shell the watcher ends at once and the bridge captures.
- theme-tokens: two generators for the desktop shell session, rendered on request with
  `generate::render_extra` (`ExtraTarget::LabwcTheme`, `ExtraTarget::DeskShellCss`): a labwc
  `themerc` (titlebars, 1 px border, buttons with a red close hover, menus, the window switcher,
  the snap overlay; spacing and button radius from the tokens; no deprecated `titlebar.height`)
  and desk-shell CSS (`--ds-*` light and dark palettes, `@define-color` names, radii and the
  taskbar, Start menu and flyout surfaces; high contrast stays desk-shell's own). `render` and the
  GNOME outputs are unchanged.
- Wayland end-to-end tests (`just e2e-wayland`, `tests/e2e-wayland/`, CI `e2e-wayland.yml`):
  headless labwc (wlroots headless backend, pixman renderer) with `deskd` and
  `desk-shell --skin windows11` in a Fedora 45 container without systemd. They check that the
  taskbar and the flyouts are real layer surfaces, that `--toggle-start`,
  `--toggle-quick-settings` and `--toggle-clipboard` reach the running shell, and that `deskd`
  and `desk-shell` own their bus names and log no critical errors. The suite takes about a second
  after the build.
- Wayland end-to-end tests, more checks: the taskbar's exclusive zone is exactly 48 px; labwc
  logs no `[ERROR]` for the bundled `rc.xml` and `themerc`; text copied with `wl-copy` reaches
  `deskd` (`Clipboard.Changed`) and `history.json` (mode `0600`); a copy with
  `x-kde-passwordManagerHint` is never recorded; an untrusted `Clipboard.List` call gets
  `AccessDenied`; a GTK 4 window that opens and closes updates the taskbar. The test image adds
  `wl-clipboard`, `gobject-introspection` and Xwayland.
- desk-shell: the Start menu searches through deskd (files, clipboard history, emoji, usage
  ranking), with the built-in catalog when deskd is missing; file and clipboard results open.
  The look follows the `skin` setting and deskd's light/dark changes live.
- desk-shell: `--toggle-*` reaches the running shell over D-Bus before any start-up work, so
  the Windows key, Win+A and Win+V answer at once.
- Windows 11 labwc session: `swayidle` locks the screen with `swaylock` for Lock and before
  suspend; the session imports `WAYLAND_DISPLAY` and `XDG_CURRENT_DESKTOP` into the systemd user
  manager and removes them when labwc exits. Release archives contain `desk-shell`.
- Search: deskd answers `. text` / `file:` (files), `# text` (clipboard history) and `: text`
  (emoji, English and Ukrainian), and mixes these results into plain queries below applications.
  Files come from a bounded in-memory index of the XDG user folders (Desktop, Documents,
  Downloads, Pictures, Music, Videos): at most 3 folder levels and 2000 entries, hidden names and
  `node_modules`, `target`, `__pycache__` skipped, symbolic links never followed; GIO folder
  watchers and a 10-minute age limit drop it. Clipboard results follow the history's private mode
  and are hidden while it is on or the history is off.
- `desk-shell`, a DE-agnostic Wayland desktop shell for compositors with `wlr-layer-shell` (labwc,
  niri, sway, KWin, COSMIC), Windows 11 first ([ADR 0012](docs/adr/0012-wayland-layer-shell-desk-shell.md)):
  a 48 px taskbar with the Start button, the Search pill, pinned applications chosen from the
  installed desktop entries, quick actions (settings hub, portal screenshot into the editor,
  clipboard history from deskd) and a clock with the date; and a Start/Search flyout with a 4 × 3
  pinned grid, a user/Settings/Lock/Power footer and instant `desk-search` results (calculator,
  Ukrainian ↔ QWERTY layout tolerance, applications, `>` system actions) driven by
  `Up`/`Down`/`Return`/`Esc`. `libgtk4-layer-shell.so.0` is loaded at run time; without it, or with
  `--windowed`, the surfaces open as normal windows. `desk-shell --toggle-start` toggles the flyout
  of the running instance from a key binding. deskd trusts the `desk-shell` binary next to it.
- `desk-shell` Windows 11 flyouts at the right end of the taskbar: quick settings
  (`desk-shell --toggle-quick-settings`, Win+A on labwc, or the new tray pill) with dark mode, do
  not disturb, private clipboard, screenshot and settings hub tiles over the default output volume,
  mute and a per-application volume mixer from deskd's `Mixer`; and clipboard history
  (`desk-shell --toggle-clipboard`, Win+V on labwc, or the taskbar button) with pin/unpin, delete,
  "Clear all" (pinned items stay) and a private-mode banner. Both close on `Esc` or a click outside
  and stay usable when deskd is not running. The settings hub and screenshot buttons moved from the
  taskbar into quick settings. `data/install.sh` now installs `desk-shell` (when built;
  `--desk-shell PATH`), `desk-win11-session` and the labwc config, and `scripts/uninstall.sh`
  removes them.
- Optional Windows 11 Wayland session on labwc: `data/labwc/windows11/` (labwc `rc.xml` with
  Windows 11 key bindings, cascade placement and edge snapping; the Fluent dark `desk-win11`
  window theme; `autostart` for the wallpaper, `deskd` and `desk-shell`), the launcher
  `data/labwc/desk-win11-session`, the session entry `data/labwc/desk-win11.desktop` and the
  guide [docs/wayland-session.md](docs/wayland-session.md). GNOME stays installed.
- macOS skin: the focused application's name, in bold, right after the logo menu, with a small
  menu (New Window, Hide, Hide Others, Quit); it follows the focus and is removed with the logo menu.
- macOS skin: dock icon magnification on dash-to-dock (1.45× under the pointer, cosine falloff over
  2.5 icons); off when animations are off or reduced motion is set, and silent without dash-to-dock.
- Incremental update script `scripts/update.sh`: pulls latest `main` (`--ff-only`), reuses the
  existing build toolbox (refreshing dnf packages, third-party assets and extensions automatically
  when `scripts/versions.sh` changes or when `--with-assets` / `--with-extensions` / `--all` is
  passed), rebuilds Rust release binaries and `desk-bridge`, syncs bundled themes, runs
  `data/install.sh` and checks `themes doctor`. `data/install.sh` now also detects stale
  `bridge/build/` artifacts when `bridge/src` or `bridge/static` changed.
- Search engine core (stage 4): the pure-Rust crate `desk-search` (no desktop dependency) with the
  query language (`=` calculator, `>` system actions, `.`/`file:` files, `#` clipboard, `:` emoji,
  plain text for all; cut at 256 characters), a safe arithmetic evaluator (`= 12*(3+4)` → `84`; no
  shell or script input), tiered ranking (exact, prefix, word prefix, substring, fuzzy with typos)
  with Unicode folding and Ukrainian ↔ QWERTY layout tolerance (`сдшзищфкв` finds «clipboard»),
  an XDG desktop entry parser, seven system actions, and decayed usage counts in an atomic 0600
  `search-usage.json` that is never written with `remember-usage=false`. `deskd` serves it as
  `io.github.OlehDzhumyk.Desk.Search` (`Query`, `RecordLaunch`, `ClearUsage`) to trusted callers
  only (ADR 0008); applications are cached in memory and dropped when an `applications/` folder
  changes (no polling). Settings `io.github.OlehDzhumyk.Desk.Search` (`enabled`, `search-shortcut`,
  `web-search` off by default, `max-results`, `remember-usage`) and a «Пошук» page; the shortcut
  is empty until the bridge launcher replaces `spotlight@nin`.
- Search box (stage 4, partial): the bridge draws the own search launcher per skin (`macos`
  Spotlight-like pill at 22% of the monitor height, `windows11` Fluent panel, `default` Adwaita
  popover), with grouped results («Top hit», then by kind), icons, `↑`/`↓`, `Tab`/`Shift+Tab`
  to the next or previous group, `Enter`, `Super+1`..`Super+9`, `Esc` that clears first and then
  closes, a second press of the search shortcut and a click outside to close. It
  follows the light scheme, high contrast and reduced motion. Applications launch, calculations
  are copied, the seven system actions run (power actions through the session manager, which
  asks first), files open, clipboard entries are put back, emoji are inserted and a web result
  opens the browser; each launch is counted with `RecordLaunch`. Queries go only to the verified
  deskd (ADR 0008), and a slow reply never replaces newer results. `search-shortcut` is bound in
  normal mode only and shown in the Shortcut Guide; it stays empty in every theme for now.
- Quick Look (stage 4, partial): `themes quick-look <paths>…` opens a short-lived GTK 4 /
  libadwaita previewer styled for the skin (`macos` dark title strip, `windows11` Peek-style
  header with `<` `>`, `default` plain window). It shows images with zoom, text and code (only the
  first `max-text-kb` KiB are read, with «Показано початок файлу»), folders with sizes and a
  details card for other files; `Space`/`Esc` close, the arrows move through the files («3 з 3»),
  `Return` opens in the default application, `I` toggles the details. Files are never run, FIFOs
  and devices are never opened, and an unreadable file shows an error card. Settings
  `io.github.OlehDzhumyk.Desk.QuickLook` (`enabled`, `quick-look-shortcut`, `max-text-kb`,
  `autoplay-media`, `follow-selection`) with per-theme shortcuts (Windows 11: Ctrl+Space; macOS:
  Super+Y; Fedora: none) and a «Швидкий перегляд» page. The bridge binds `quick-look-shortcut`
  in normal mode only (never plain Space) and opens the previewer on the local files copied to the
  clipboard (Files offers no selection API); a second press closes it, and with no copied file a
  toast explains what to do. The Shortcut Guide lists the shortcut. The Files hand-over (Space on
  the selection), media and PDF follow separately.
- Window layouts in the bridge (stage 4): `Super+Z` (macOS: `Ctrl+Super+Up`) opens a skin-aware
  layout picker for the focused window with miniature layouts and a full keyboard path; hovering
  or focusing a zone shows a click-through zone preview (no fade under reduced motion); choosing a
  zone snaps the window into the zone that deskd resolves and saves the monitor's active layout.
  Snap assist offers the other windows of the workspace for the empty zones (`1`..`9`, `Esc`).
  `Ctrl+Super+T` pins the focused window above others or unpins it, with a toast. Dialogs, tool
  windows and override-redirect surfaces are never snapped. Not yet: hover picker on the maximise
  button and drag zones.
- Window layouts core (stage 4): the pure-Rust crate `desk-layouts` (no desktop dependency) with
  six built-in layouts (halves, thirds, two-thirds, quarters, focus, stack), portrait adaptation,
  even-pixel zone geometry with exact outer and inner gaps inside the work area, minimum-size
  fitting and an atomic 0600 `layouts.json` for custom layouts and the active layout per monitor.
  `deskd` serves it as `io.github.OlehDzhumyk.Desk.Layouts` (`ListLayouts`, `ActiveLayout`,
  `SetActiveLayout`, `ResolveZones`, signal `Changed`) to trusted callers only (ADR 0008).
  Settings `io.github.OlehDzhumyk.Desk.Layouts` (`enabled`, `layout-picker-shortcut`,
  `always-on-top-shortcut`, `gap-outer`, `gap-inner`, `snap-assist`, `hover-picker`) with
  per-theme defaults and a «Розкладки вікон» page.
- Shortcut guide (stage 4): hold `Super` for `hold-delay` (default 900 ms) to show a skin-aware
  overlay of the keyboard shortcuts currently active in the session, read live from GNOME
  (`wm.keybindings`, `shell.keybindings`, `media-keys` including custom keybindings) and
  `io.github.OlehDzhumyk.Desk.*` schemas plus `Desk.Shortcuts` overrides (labelled «theme» vs
  «your setting»). Releasing `Super` closes the overlay without opening the overview; pressing any
  other key cancels or lets the real shortcut run. Accessible shortcut `Ctrl+Shift+Super+/`
  (`shortcut-guide-shortcut`, override `shortcut-guide-toggle`) and `sticky` mode open a modal
  searchable view (`Up`/`Down` scroll, `Esc` closes). `windows11` draws a dimmed full-screen
  multi-column grid with rounded square keycaps and a neutral `Super` label; `macos` draws a
  centred translucent panel with `⌘ ⌥ ⇧ ⌃` symbols; `default` uses plain-text keycaps and is off
  by default (`enabled=false` in the schema, `enabled=true` in `windows11` and `macos` themes).
  Settings `io.github.OlehDzhumyk.Desk.ShortcutGuide` (`enabled`, `hold-delay`, `sticky`,
  `show-owner`, `shortcut-guide-shortcut`) and a «Довідник скорочень» page in the window.
- Screenshot annotation editor (stage 3): `themes capture-edit <path.png>` opens a short-lived
  GTK 4 / libadwaita window with pen, highlighter, arrow, line, rectangle, ellipse, redaction
  (solid or pixelate) and crop, an accent-first colour palette, line widths, undo/redo
  (`Ctrl+Z`, `Ctrl+Shift+Z`), zoom, Copy (`Ctrl+C`, PNG), Save (`Ctrl+S`) and Save as. Saving
  renders every annotation through `desk-capture` and replaces the file atomically with one plain
  RGBA PNG: redacted pixels are destroyed, no metadata, layer or undo history is kept
  (AC-SHOT-05). `Esc` closes, with a «Зберегти зміни?» question only when edited.
- Skin-aware on-screen displays (stage 3): with the `windows11` and `macos` skins the bridge draws
  the volume, mute, microphone, brightness and keyboard backlight OSD itself (Windows 11: acrylic
  bar at the bottom centre; macOS: translucent centred square or a pill), click-through, with a
  distinct segment, marker and text above 100 %, high-contrast outline and number, reduced-motion
  support and a polite screen-reader announcement. Settings `io.github.OlehDzhumyk.Desk.Osd`
  (`enabled`, `show-percentage`, `timeout-ms`, `position`) and an «Екранні індикатори» page in the
  window. With the `default` skin, `enabled` off or a manager another extension patched, GNOME's
  own OSD stays untouched; skin switch and `disable()` restore it exactly.
- OCR text extractor (stage 3): select a region of the screen and its text is recognised locally
  with Tesseract and copied to the clipboard. `deskd` hosts `io.github.OlehDzhumyk.Desk.Ocr` at
  `/io/github/OlehDzhumyk/Desk/Ocr` (`Recognize`, `AvailableLanguages`, `IsAvailable`): one request
  at a time, regions above `max-megapixels` refused, missing language data named with English as the
  fallback; the child runs with `RLIMIT_AS` of 1 GiB, without network where user namespaces are
  allowed, and its temporary image is always deleted. The bridge binds `ocr-shortcut`
  (Shift+Super+T; Ctrl+Shift+Super+T in the macOS theme; override `ocr-toggle`), draws a dimmed
  selection overlay (pointer or keyboard, `Esc` cancels) per skin and shows a status toast.
  Settings `io.github.OlehDzhumyk.Desk.Ocr` (`enabled`, `languages`, `record-in-history`,
  `keep-line-breaks`, `max-megapixels`, `ocr-shortcut`) and a «Розпізнавання тексту» page.
- Screenshot toolbar and capture in the bridge (stage 3): the four `capture-*-shortcut` keys (normal
  mode only), a skin-aware toolbar (`windows11` top bar, `macos` capsule, `default` bar) with Region,
  Screen and Record modes, a 0 / 3 / 10 s delay with a countdown that `Esc` cancels, full keyboard
  operation, region and full-screen capture saved through deskd's `SaveScreenshot` and copied to the
  clipboard as `image/png` (`copy-to-clipboard`), a macOS floating thumbnail (6 s, click opens
  `themes capture-edit`) or a toast in the other skins, and screen recording through GNOME's
  screencast service with an elapsed-time item in the top bar.
- Screenshot core (stage 3): the new pure-Rust crate `desk-capture` (no GTK, GLib or D-Bus, so any
  Wayland desktop or a CLI can use it) checks PNG captures and refuses images over 100 megapixels
  before decoding, removes `tEXt`, `zTXt`, `iTXt`, `tIME` and `eXIf` chunks (no user name, host
  name or path in a saved file), saves atomically as `Screenshot YYYY-MM-DD HH-MM-SS.png` with
  `-2`, `-3` for captures in the same second, and renders the editor's annotations (redact as solid
  fill or pixelate, crop, rectangle, ellipse, line, arrow, highlighter, pen) from JSON. Redaction is
  destructive: the saved file holds only the result.
- Screenshot settings (stage 3): schema `io.github.OlehDzhumyk.Desk.Capture` (`enabled`,
  `save-folder`, `copy-to-clipboard`, `show-preview`, `delay`, `include-pointer`, `record-format`,
  `record-audio`, `annotate-after-capture`, and `capture-region-shortcut`,
  `capture-screen-shortcut`, `capture-toolbar-shortcut`, `capture-record-shortcut`), a «Знімки
  екрана» page, and four shortcuts on the «Keyboard shortcuts» page (overrides `capture-region`,
  `capture-screen`, `capture-toolbar`, `capture-record`). Windows 11 theme: Shift+Super+S,
  Super+Print, Print, Shift+Super+R; macOS theme: Shift+Super+4, Shift+Super+3, Shift+Super+5;
  Fedora keeps GNOME's own screenshot keys.
- Screenshot engine (stage 3): `deskd` hosts `io.github.OlehDzhumyk.Desk.Capture` at
  `/io/github/OlehDzhumyk/Desk/Capture` (`SaveScreenshot`, `ApplyAnnotations`) for trusted
  callers only (ADR 0008), built on `desk-capture`. It follows `enabled` and `save-folder` live and
  holds no image between calls.
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

- `data/install.sh` replaces a hand-made `~/.local/bin/desktop-themes-update` launcher (and its
  `desktop-themes-update.desktop`) with «Update Desktop Themes»; it removes them only when the
  script starts with the comment "Update desktop-themes-rs from GitHub".
- CI: npm downloads are cached in the bridge, E2E build and install smoke jobs; the GNOME Shell
  E2E workflow runs on pull requests only when they touch code or data under test (not docs or
  labwc only); every job has a timeout; `cargo test` also runs `cargo xtask check`, and the labwc
  `rc.xml` is checked by `xmllint`.
- Theme `macos`: the dock shows when the pointer touches the screen edge, without pressure, after
  0.1 s, and hides 0.2 s after the pointer leaves.
- The bridge's shared UI parts live in `bridge/src/ui/`: primitives (orientations, motion setting,
  system style classes, icon button, label), one skin-aware toast (`.desk-toast`, used by the
  colour picker and the text extractor; it now follows `.desk-light` and the macOS skin) and the
  region selector with its physical-pixel capture (`.desk-region-*`), ready for screenshots.
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

- desk-shell: the taskbar is exactly 48 px high again, with a 48 px exclusive zone (ADR 0012); the
  top line and the Search pill outline are inset shadows, not borders (labwc 0.20 measured 49 px).
- Windows 11 labwc session: `rc.xml` and the `desk-win11` theme no longer use options that labwc
  0.20 rejects (`windowSwitcher` `show` and `allWorkspaces`, `snapping` `range`, `titlebar.height`);
  the defaults give the same behaviour.
- desk-shell: the clipboard flyout drops the answer of an older refresh (no stale or doubled
  rows); `LD_PRELOAD` keeps entries that are not UTF-8 and finds gtk4-layer-shell only as a whole
  entry or path segment; `dlsym` pointers are size-checked at compile time.
- `data/install.sh` passes the bridge path to `sh -c` as an argument, not inside the script text.
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
- `setlocale` runs before the language setting is read, while the program still has one thread
  (reading GSettings starts GLib's worker threads, and `setlocale` is not thread-safe).
- deskd's Capture interface decodes, annotates and encodes PNG files on a worker thread and holds
  no lock meanwhile; other D-Bus calls are no longer held up while a screenshot is saved.
- The caller check (ADR 0008) asks the bus for the owner of `org.gnome.Shell` only for callers that
  are not our own binaries, and builds its bus proxy once: `desk-shell` on compositors without
  GNOME Shell no longer pays for a failing lookup on every call.

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
