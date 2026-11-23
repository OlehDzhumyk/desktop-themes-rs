# Bundled themes

Five themes ship in `themes/`. Third-party assets they refer to are installed by
`scripts/install-assets.sh` and `scripts/install-extensions.sh`; versions are pinned in
`scripts/versions.sh`, licences are in [../CREDITS.md](../CREDITS.md).

## fedora (built-in)

The stock look. It cannot be deleted. Selecting it returns every module to the baseline (plus the
dark colour scheme key in `settings.dconf`).

## macos

WhiteSur GTK/shell theme, WhiteSur icons and cursors, WhiteSur wallpapers, Inter UI font,
JetBrains Mono, a bottom dock (Dash to Dock), blurred panel (Blur my Shell), Spotlight-style
launcher, a macOS-style logo menu on the left of the top bar and clipboard history (desk-bridge,
skin `macos`: a Spotlight-like panel on Shift+Super+V), and a lock-screen layout.

* Names used: GTK `WhiteSur-Light`, shell `WhiteSur-Dark`, icons `WhiteSur`, cursor `WhiteSur-cursors`,
  fonts `Inter`, `JetBrains Mono`.
* `WhiteSur-Light` ships `gtk-dark.css`, so GTK apps follow the GNOME dark-style toggle. The shell
  theme is always dark (GNOME has a single shell theme and the panel sits on arbitrary wallpapers).
* **Behavior**: Alt/Super+Tab switches applications, Alt/Super+\` windows of the app, Super+Space opens
  the launcher, Super+Q closes, Super+H minimizes, natural scrolling, hot corner on, edge tiling off,
  grayscale font rendering.
* **Sounds**: `macos-like`, synthesized (see below).
* **Apps**: Ptyxis palette `Pro` (referenced by name) and JetBrains Mono 12 in the default profile.
* **Tokens** (`tokens.toml`): accent `blue`, fonts (Inter, JetBrains Mono, bold titles), the sound
  theme and the terminal; WhiteSur's `gtk4/` and shell theme stay hand-made and win.
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

## macos-glass (experimental)

> **Experimental.** On GNOME Shell 51.1 on real hardware the shell crashed (SIGSEGV in
> `st_theme_node_reduce_border_radius` ← `st_theme_node_paint_borders`) about 20 s after applying
> this theme, and again at the next start; GNOME then sets `org.gnome.shell disable-user-extensions`
> and no extension runs. `themes doctor` and the window detect this state and name the likely
> culprit (here `liquid-glass@...`); see docs/architecture.md, "Crash protection".

The macOS theme plus the **Liquid Glass** extension (refraction, highlights and glass edges on the
panel, dock, menus, quick settings, notifications, OSD). It inherits everything from `macos`
(`INHERITS=macos` in `theme.conf`) and owns only `theme.conf`, `glass.dconf` and an
`extensions.txt` with one line, `+liquid-glass@thinkingcoding1231.gmail.com`.

The refraction shader redraws the background under each glass element and costs noticeably more GPU
time than plain blur; prefer `macos` on battery. Liquid Glass is not on extensions.gnome.org and is
installed from its repository at a pinned commit.

## windows11

Fluent GTK/shell theme (`round` tweak), Fluent icons and cursors, Noto Sans, a centred taskbar
(Dash to Panel) with ArcMenu as the start menu, Tiling Shell for snap layouts, Desktop Icons NG, and
Win+V clipboard history (desk-bridge, skin `windows11`: a flyout next to the text cursor).

* Names used: GTK `Fluent-round`, shell `Fluent-round-Dark`, icons `Fluent`, cursor `Fluent-cursors`,
  font `Noto Sans 10`.
* Wallpapers `bloom-light.png` / `bloom-dark.png` (3840x2160) are generated procedurally by
  `tools/generate-bloom-wallpaper.py`.
* **Behavior**: Alt+Tab switches windows, Super+Tab opens the overview (Task View), Super+D show
  desktop, Super+E Files, Super+Shift+S screenshot UI, Ctrl+Shift+Esc system monitor (custom
  keybinding, which replaces the `custom-keybindings` list), traditional scrolling, edge tiling on,
  hot corner off, rgba subpixel rendering.
* **Sounds**: `windows11-like`, synthesized.
* **Apps**: Ptyxis palette `campbell` (referenced by name), Cascadia Mono 12, I-beam cursor.
* **Tokens** (`tokens.toml`): accent `blue`, Noto Sans, the sound theme and the terminal; Fluent's
  `gtk4/` and shell theme stay hand-made and win.
* Fluent has no GNOME Shell 49-51 stylesheet; the newest one is used, so expect small overview/panel
  glitches. `dash-to-panel` conflicts with `dash-to-dock` (macOS): never enable both. `gtk4-ding` is
  the least proven extension; drop it from `extensions.txt` if it misbehaves.
* Segoe UI and Microsoft logos/wallpapers are not used.

## lavender (tokens only)

A showcase of [design tokens](design-tokens.md): the theme is `theme.conf` + `tokens.toml`. Lavender
colours for libadwaita in light and dark, soft radii (window 14, buttons 8, cards and popovers 12),
Adwaita Sans/Mono, a dark lavender GNOME Shell layer over the stock stylesheet (translucent panel,
rounded quick settings), its own Ptyxis palette and a generated gradient wallpaper. Stock icons,
cursor and behaviour; no extensions besides User Themes (added automatically for the shell layer).

## Sound themes (generated, CC0)

`tools/synth-sounds.py <dir>` writes `macos-like` and `windows11-like` (27 freedesktop events each,
48 kHz stereo Vorbis, about -16 LUFS, peaks below -1 dBFS) using only numpy/scipy synthesis: no
samples, no Apple or Microsoft audio. GNOME 51 plays only some events from a theme (window bell,
notification sound hints, GTK/libcanberra apps); volume feedback, screenshot, drive and break sounds
are fixed system files and cannot be themed. Choosing an alert sound in Settings writes
`theme-name='__custom'`; re-applying the theme resets it.

## Own extensions

* `desk-bridge@desktop-themes-rs` (TypeScript sources in `bridge/`): the one shell-side bridge to
  deskd and the theme service: the "Theme" toggle in quick settings (formerly `themes-quick`), the
  clipboard history panels, the volume mixer and the macOS logo menu. Enabled in every theme (a
  theme cannot switch it off). See docs/features.md.
* `extensions/lockscreen-layout@desktop-themes-rs`: patches the GNOME Shell 51 unlock dialog (clock
  position, size and font, blur, tint, dedicated background). No network, no subprocesses; everything
  is restored in `disable()`. Settings schema `org.gnome.shell.extensions.lockscreen-layout`, driven
  per theme through `lockscreen.dconf`.
