# Desktop features

Features that both Windows 11 and macOS have, built for GNOME. The logic and state live in **deskd**;
the shell-side UI is drawn by the bridge extension **desk-bridge@desktop-themes-rs** in the skin of
the current theme; settings are on the «Функції» page of the window (`themes gui --page clipboard`).
Every feature page names its origin («Є в Windows 11 і macOS», «Засновано на …»).

## Clipboard history

| | |
|---|---|
| Open | Windows 11 theme: **Super+V**; macOS theme: **Shift+Super+V**; Fedora: Shift+Super+V or quick settings «Буфер обміну» (`toggle-shortcut` in the theme's `desk.dconf` is the default; change it in the window, page «Keyboard shortcuts», or with `themes shortcut set clipboard-toggle ...`) |
| Keys | ↑/↓ choose, Enter paste, Esc close; Ctrl+P pin; Delete (Windows 11) or Ctrl+Backspace (macOS) delete; typing searches (macOS, default) |
| Paste | the item goes onto the clipboard and Shift+Insert (Ctrl+Shift+Insert in terminals) is sent to the window that had the focus; turn off with «Вставляти вибране» |
| Items | text up to 256 KiB, images (PNG/JPEG/GIF/WebP/SVG) up to `image-max-kb`, all images together up to `image-budget-mb` (oldest unpinned images are dropped first; nothing is written when `$XDG_RUNTIME_DIR` is nearly full); duplicates move to the top; pinned items survive trimming and «Очистити все» |
| Privacy | private mode (nothing recorded); content offered with `x-kde-passwordManagerHint` or `application/x-nspasteboard-concealed-type` is never read; copies made while a password manager has focus (`excluded-apps`: KeePassXC, Bitwarden, 1Password, Secrets, Seahorse, Proton Pass) are skipped |
| Storage | `~/.local/share/deskd/clipboard/history.json` (0600, text only by default); images only for the session in `$XDG_RUNTIME_DIR/deskd/clipboard/images` unless «Зберігати зображення» is on |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Clipboard`: `enabled`, `history-size`, `images`, `image-max-kb`, `image-budget-mb`, `persist`, `persist-images`, `private-mode`, `paste-on-select`, `toggle-shortcut`, `excluded-apps`; your shortcut: `io.github.OlehDzhumyk.Desk.Shortcuts` `clipboard-toggle` |

Skins:

* **windows11**: flyout next to the text cursor (the caret rectangle the focused app reports to the
  input method; the pointer otherwise). Tabs «Буфер обміну» / «Емодзі» / «Символи» (all three live in
  the same unified flyout), «Очистити все», cards with a pin button and «…», which reveals
  delete and pin like Windows 11.
* **macos**: a Spotlight-like centred panel (macOS 26 style): large search field, list with
  relative times, preview of the selected item, keyboard hints.
* **default**: a compact list popup at the pointer and a «Буфер обміну» toggle in quick settings
  (recent items, private mode, clear, settings).

### Based on Clipboard Indicator

Capture, paste-on-select and the image handling are adapted from
[Clipboard Indicator](https://github.com/Tudmotu/gnome-shell-extension-clipboard-indicator)
(`clipboard-indicator@tudmotu.com`, Copyright (c) 2014 Yotam Bar-On and contributors, **MIT** — the
licence file of the extension; compatible with GPL-3.0-or-later):

| From | To | What |
|---|---|---|
| `extension.js` `_setupSelectionTracking`, `_refreshIndicator`, `#getClipboardContent` | `bridge/src/clipboard/watcher.ts` | `Meta.Selection::owner-changed`, the mime-type order, the UTF8_STRING workaround (gnome-shell#8233), serialised reads |
| `keyboard.js`, `extension.js` `#pasteItem`, `#updateClipboard` | `bridge/src/clipboard/paste.ts` | virtual keyboard, Shift+Insert / Ctrl+Shift+Insert for terminals (input purpose), setting text or bytes on `St.Clipboard` |

Storage, history rules, search and the UI are new (deskd and the skins). The installed Clipboard
Indicator is no longer enabled by any bundled theme.

## Volume mixer

| | |
|---|---|
| Open | the chevron next to the volume slider in quick settings; right-click on the system area of the top bar (`entry-point`); Windows 11 theme: **Ctrl+Super+V** (`mixer-shortcut` in the theme's `desk.dconf`; change it on the «Keyboard shortcuts» page or with `themes shortcut set mixer-open ...`) |
| Shows | one row per application that plays sound (icon, name, what plays, slider, mute, «Play on…»), output and input devices with the default checked |
| Keys | Left/Right: `volume-step` (5 %), Shift: 1 %, Home/End, M mutes the focused row; Up/Down move between rows; Esc closes |
| Limits | 100 %, or 150 % when `org.gnome.desktop.sound allow-volume-above-100-percent` is on |
| Engine | deskd follows PipeWire (events only, no audio data) and exposes `io.github.OlehDzhumyk.Desk.Mixer`; the bridge never opens an audio connection |
| Storage | only with «Запам'ятовувати гучність програм»: `~/.local/share/deskd/mixer/apps.json` (0600: app ids, volume, mute); turning it off deletes it |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Mixer`: `enabled`, `entry-point`, `show-silent-apps`, `remember-app-volume`, `volume-step`, `hide-apps`, `mixer-shortcut`; your shortcut: `io.github.OlehDzhumyk.Desk.Shortcuts` `mixer-open` |

Skins (all inside the quick settings sound menu, in the shell theme's colours, light/dark and accent):

* **windows11**: «Мікшер гучності» sub-panel with «Виведення» (devices), the application sliders and
  «Введення», like the Windows 11 volume mixer flyout.
* **macos**: «Звук» page as in Control Center: «Виведення» and «Введення» with check marks and plain
  device names (Bluetooth first), and a collapsed «Програми» disclosure, shown only while something
  plays. macOS has no per-application volume; the badge says «Запозичено з Windows, адаптовано під
  macOS».
* **default**: GNOME's own device chooser, with a «Програми» group of sliders under it.

```sh
gdbus call --session -d io.github.OlehDzhumyk.Desk -o /io/github/OlehDzhumyk/Desk/Mixer \
  -m io.github.OlehDzhumyk.Desk.Mixer.ListApps false
```

### Based on wiremix

The PipeWire side of the engine (which objects to bind, how `Props`, `Route` and metadata events are
read and written, the cubic volume scale) is adapted from
[wiremix](https://github.com/tsowell/wiremix) (Copyright Thomas Sowell, **MIT OR Apache-2.0**; used under
MIT, compatible with GPL-3.0-or-later), `src/wirehose/` → `crates/deskd/src/mixer/pw.rs` and `model.rs`
(attribution in the file headers). The application model, defaults, memory and the UI are new.

## Logo menu

| | |
|---|---|
| Open | left side of the top bar (shown only in the `macos` skin) |
| Items | About This System, System Settings…, Applications, Force Quit… (asks for confirmation before killing the focused window), Sleep, Restart…, Shut Down…, Lock Screen, Log Out… |
| Activities | hides the default Activities indicator while shown (`hide-activities`) and restores it when disabled, hidden or on any other skin |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.LogoMenu`: `enabled`, `hide-activities` |

Skins:

* **macos**: shown on the left of the top bar (`start-here-symbolic`), replacing the third-party `logomenu@aryan_k` extension.
* **windows11** and **default**: not shown; the standard Activities indicator is left untouched.

## Emoji and symbol picker

| | |
|---|---|
| Open | Windows 11 theme: **Super+.** or **Super+;**; macOS theme: **Ctrl+Super+Space**; Fedora: **Super+.** (`emoji-shortcut` in the theme's `desk.dconf`; override on «Keyboard shortcuts» or `themes shortcut set emoji-toggle ...`) |
| Search | English and Ukrainian names and keywords, case-folded and apostrophe/diacritics-tolerant; ranked exact → prefix → word/keyword → substring with recents tie-break |
| Keys | Arrows move across the grid, `Page Up/Down` jump 3 rows, `Ctrl+1..9` switch category, `Enter` inserts, `Shift+Enter` inserts and keeps open, `Esc` closes |
| Insertion | Sets the glyph with `application/x-desk-internal` so clipboard history ignores it, sends `Shift+Insert` (`Ctrl+Shift+Insert` in terminals), then restores the previous clipboard text |
| Skin tones | Fitzpatrick modifier (`none`, `light`, `medium-light`, `medium`, `medium-dark`, `dark`) applied to person/hand emoji (including ZWJ sequences) |
| Storage | Last 36 picks in `~/.local/share/deskd/emoji.json` (0600); turning off `remember-recents` clears and removes the file |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Emoji`: `enabled`, `emoji-shortcut`, `skin-tone`, `remember-recents`, `close-after-insert`; your shortcut: `io.github.OlehDzhumyk.Desk.Shortcuts` `emoji-toggle` |

Skins:

* **windows11**: live «Емодзі» and «Символи» tabs inside the unified Win+V / Win+. flyout; stays open after insert by default.
* **macos** and **default**: floating keyboard-first picker anchored near the text caret (pointer fallback); closes after insert by default.
## Automatic light/dark

| | |
|---|---|
| Modes | `manual` (default, does not touch `color-scheme`), `light`, `dark`, `auto-sun` (local sunrise/sunset via NOAA solar algorithm), `auto-time` (fixed 24-hour times, default `20:00` to `07:00`) |
| Location | city name from the built-in offline table, decimal coordinates `lat,lon`, or empty (falls back to the system timezone; zero network requests) |
| Manual override | switching light/dark manually (GNOME Quick Settings or D-Bus `SetScheme`) while in an auto mode holds the manual choice until the next scheduled transition when `override-until-next` is enabled |
| Scheduling | one-shot wake timer in `deskd` (zero idle wakeups in `manual`/`light`/`dark`), re-armed automatically on resume from suspend (`org.freedesktop.login1.Manager.PrepareForSleep`) and timezone changes (`org.freedesktop.timedate1`) |
| D-Bus | `io.github.OlehDzhumyk.Desk.Appearance` at `/io/github/OlehDzhumyk/Desk/Appearance` (`Status`, `SetScheme`, `Recompute`, `WallpaperAccent`, signal `Changed`) |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Appearance`: `mode`, `dark-from`, `light-from`, `location`, `twilight-offset`, `override-until-next`, `accent-from-wallpaper`, `accent-source` |

## Theme from wallpaper

| | |
|---|---|
| Create a theme | `themes tokens from-image PATH --name ID` (CLI), or «Create theme from wallpaper…» on the «Appearance» page: file chooser, seed colour, light/dark preview with contrast numbers, name, then «Apply» in the toast. `--force` moves the old theme to `trash/` first |
| Inspect an image | `themes tokens seed PATH [--json]`: seed colour, nearest GNOME accent, «almost no colour» note, light/dark core colours and their contrast; writes nothing |
| Accent from wallpaper | setting `accent-from-wallpaper` (off by default): `deskd` follows `org.gnome.desktop.background picture-uri`/`picture-uri-dark` (GSettings signals, 500 ms debounce, no polling) and sets `org.gnome.desktop.interface accent-color` to the named accent nearest the seed. `accent-source` picks the variant (`active` follows `color-scheme`). The image is decoded in a `themes tokens seed` child process. Turning it off restores the accent in force before (unless a theme or the user changed it since); `themes doctor` shows the state |
| Images | every gdk-pixbuf format; animated images use the first frame; slideshow XML uses its first image; images under 16×16 or fully transparent are refused |
| Entry points | CLI (`themes tokens from-image`, `themes tokens seed`), window («Appearance» page: switch, source, current accent, create button), quick settings («Theme» menu: «Accent from wallpaper» switch, all skins), D-Bus `WallpaperAccent()` |

Skins: the quick settings switch is the same in `macos`, `windows11` and `default`; the skin-specific
pickers of the spec (macOS dot grid, Windows 11 «Automatic» drop-down) are not built yet. Token
themes and hand-made themes (`macos`, `windows11`) both get only the GNOME `accent-color` change in
the live mode; a full token-palette overlay is not built yet.

## Focus modes

| | |
|---|---|
| Modes | built-in «Не турбувати» (`dnd`), «Робота» (`work`), «Режим сну» (`sleep`, also mutes event sounds) and custom ones; each has `block_banners`, `mute_sounds`, `allowed_apps` and an optional schedule (weekdays, start, end; an end before the start ends the next morning) |
| Start | by hand until turned off, for a time or until a time of day; by schedule; or a focus session (do not disturb for N minutes, then a break). A manual start overrides schedules until it ends; the schedule that started last wins; each schedule window starts its mode once |
| Effect | `deskd` writes `org.gnome.desktop.notifications show-banners` (and `org.gnome.desktop.sound event-sounds`); notifications stay in the notification list, critical ones always show, allowed apps keep their banners |
| Revert | the overridden value is stored in `~/.local/share/deskd/focus.json` (0600) before the write and restored when the mode ends, after a crash or restart; turning GNOME's own do-not-disturb off ends the mode ([ADR 0009](adr/0009-deskd-writes-do-not-disturb-keys.md)) |
| Scheduling | the shared one-shot timer of `deskd` (no polling), re-armed on resume and timezone changes |
| Entry points | quick settings tile «Фокус», `themes focus status\|list\|start\|session\|stop\|set\|delete`, the «Фокус» page of the window, D-Bus `io.github.OlehDzhumyk.Desk.Focus`; `themes doctor` reports an active mode or banners left off without one |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Focus`: `enabled`, `active-mode` (deskd's mirror), `summary-on-end`, `session-length`, `session-break` |

Skins:

* **default**: a «Фокус» tile in quick settings with the mode list; a symbolic moon icon in the panel while a mode is on.
* **macos**: the same tile (like Control Center); a crescent icon in the panel.
* **windows11**: the tile also starts focus sessions (15/25/45/60 min) and shows the countdown; a bell-slash icon in the panel. GNOME Shell has no Windows-style notification flyout, so these controls live in the tile.
