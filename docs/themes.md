# Bundled themes

Four themes ship in `themes/`. Third-party assets they refer to are installed by
`scripts/install-assets.sh` and `scripts/install-extensions.sh`; versions are pinned in
`scripts/versions.sh`, licences are in [../CREDITS.md](../CREDITS.md).

## fedora (built-in)

The stock look. It cannot be deleted. Selecting it returns every module to the baseline (plus the
dark colour scheme key in `settings.dconf`).

## macos

WhiteSur GTK/shell theme, WhiteSur icons and cursors, WhiteSur wallpapers, Inter UI font,
JetBrains Mono, a bottom dock (Dash to Dock), blurred panel (Blur my Shell), Spotlight-style
launcher, clipboard history, and a lock-screen layout.

* Names used: GTK `WhiteSur-Light`, shell `WhiteSur-Dark`, icons `WhiteSur`, cursor `WhiteSur-cursors`,
  fonts `Inter`, `JetBrains Mono`.
* `WhiteSur-Light` ships `gtk-dark.css`, so GTK apps follow the GNOME dark-style toggle. The shell
  theme is always dark (GNOME has a single shell theme and the panel sits on arbitrary wallpapers).
* **Behavior**: Alt/Super+Tab switches applications, Alt/Super+\` windows of the app, Super+Space opens
  the launcher, Super+Q closes, Super+H minimizes, natural scrolling, hot corner on, edge tiling off,
  grayscale font rendering.
* **Sounds**: `macos-like`, synthesized (see below).
* **Apps**: Ptyxis palette `Pro` and JetBrains Mono 12 in the default profile.
* **Spotlight** (`spotlight@nin`) replaces the overview search; Super+Space must not stay bound to
  `toggle-overview` (both would fire and the shell may crash), so `spotlight.dconf` and
  `behavior.dconf` both clear it. Two small patches are applied, see ../patches/README.md.
* **Lock screen**: `lockscreen-layout` (this repository's extension): large thin clock at the top,
  blurred wallpaper, password field below centre.
* No Apple SF fonts, logos or wallpapers are used. The Apple panel icon variant of WhiteSur is not
  used.

Compatibility notes: the WhiteSur installer knows GNOME up to 48; on Shell 51 / libadwaita 1.10 expect
small cosmetic differences. Upstream WhiteSur issue 1440 reports stutter on GNOME 51 Wayland with any
user shell theme; if the shell lags after applying, clear `user-theme` `name`.

## macos-glass

The macOS theme plus the **Liquid Glass** extension (refraction, highlights and glass edges on the
panel, dock, menus, quick settings, notifications, OSD). Everything except `theme.conf`,
`glass.dconf`, `extensions.txt` is a relative symlink to `../macos/*`; run
`themes/macos-glass/sync-glass.sh` after changing `themes/macos`.

The refraction shader redraws the background under each glass element and costs noticeably more GPU
time than plain blur; prefer `macos` on battery. Liquid Glass is not on extensions.gnome.org and is
installed from its repository at a pinned commit.

## windows11

Fluent GTK/shell theme (`round` tweak), Fluent icons and cursors, Noto Sans, a centred taskbar
(Dash to Panel) with ArcMenu as the start menu, Tiling Shell for snap layouts, Desktop Icons NG, and
Win+V clipboard history.

* Names used: GTK `Fluent-round`, shell `Fluent-round-Dark`, icons `Fluent`, cursor `Fluent-cursors`,
  font `Noto Sans 10`.
* Wallpapers `bloom-light.png` / `bloom-dark.png` (3840x2160) are generated procedurally by
  `tools/generate-bloom-wallpaper.py`.
* **Behavior**: Alt+Tab switches windows, Super+Tab opens the overview (Task View), Super+D show
  desktop, Super+E Files, Super+Shift+S screenshot UI, Ctrl+Shift+Esc system monitor (custom
  keybinding, which replaces the `custom-keybindings` list), traditional scrolling, edge tiling on,
  hot corner off, rgba subpixel rendering.
* **Sounds**: `windows11-like`, synthesized.
* **Apps**: Ptyxis palette `campbell`, Cascadia Mono 12, I-beam cursor.
* Fluent has no GNOME Shell 49-51 stylesheet; the newest one is used, so expect small overview/panel
  glitches. `dash-to-panel` conflicts with `dash-to-dock` (macOS): never enable both. `gtk4-ding` is
  the least proven extension; drop it from `extensions.txt` if it misbehaves.
* Segoe UI and Microsoft logos/wallpapers are not used.

## Sound themes (generated, CC0)

`tools/synth-sounds.py <dir>` writes `macos-like` and `windows11-like` (27 freedesktop events each,
48 kHz stereo Vorbis, about -16 LUFS, peaks below -1 dBFS) using only numpy/scipy synthesis: no
samples, no Apple or Microsoft audio. GNOME 51 plays only some events from a theme (window bell,
notification sound hints, GTK/libcanberra apps); volume feedback, screenshot, drive and break sounds
are fixed system files and cannot be themed. Choosing an alert sound in Settings writes
`theme-name='__custom'`; re-applying the theme resets it.

## Own extensions

* `extensions/themes-quick@desktop-themes-rs`: the "Theme" toggle in quick settings, talks to the
  D-Bus service. It is enabled in every theme (it cannot be switched off by a theme).
* `extensions/lockscreen-layout@desktop-themes-rs`: patches the GNOME Shell 51 unlock dialog (clock
  position, size and font, blur, tint, dedicated background). No network, no subprocesses; everything
  is restored in `disable()`. Settings schema `org.gnome.shell.extensions.lockscreen-layout`, driven
  per theme through `lockscreen.dconf`.
