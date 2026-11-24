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
## Colour picker

An eyedropper with a magnifying loupe: deskd formats and remembers the colours, the bridge reads the pixels
and draws the loupe, the history panel and the toast.

| | |
|---|---|
| Open | Windows 11 theme: **Shift+Super+C**; macOS theme: **Ctrl+Shift+Super+C**; Fedora: **Shift+Super+C** (`color-picker-shortcut` in the theme's `desk.dconf`; override on «Keyboard shortcuts» or `themes shortcut set color-picker-toggle ...`) |
| Picking | Crosshair cursor and a preview next to the pointer (loupe of a frame taken at open, live swatch and value, format and zoom); it flips near the monitor edges. Click, `Enter` or `Space` picks and copies (`copy-on-pick`) with a «Copied #A1B2C3» toast; `Esc` cancels and leaves the clipboard as it was; transparent or off-screen pixels are refused |
| Keys | Arrows move the pointer 1 px (`Shift`: 10 px), `F` cycles the format (saved in `format`), `+`/`-` or the scroll wheel change the zoom 4x..16x (saved in `zoom`), `H` or a right click opens the history |
| History panel | Swatches with the value and the nearest colour name (accessible name «value, name»), click or `Enter` copies, `Delete` removes, format buttons, «Clear», WCAG contrast ratio and level of the two newest picks |
| Formats | HEX `#RRGGBB` (default), `rgb(r, g, b)`, `hsl(h, s%, l%)`, `hsv(h, s%, v%)`, CSS name when exact, GTK `@define-color` line, decimal `r,g,b` (`theme_tokens::picker`) |
| History | Newest first, one entry per `#RRGGBB` (a repeat moves to the front), `history-size` picks (20 by default); the nearest CSS colour name for accessible swatch names; WCAG contrast ratio of the two newest picks |
| Storage | `~/.local/share/deskd/colors.json` (0600) only while `history-size` is above 0; setting it to 0 clears the history and removes the file |
| D-Bus | `io.github.OlehDzhumyk.Desk.ColorPicker` at `/io/github/OlehDzhumyk/Desk/ColorPicker`: `Record`, `List`, `Remove`, `Clear`, `Format`, `Contrast`, signal `Changed` |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.ColorPicker`: `enabled`, `format`, `zoom`, `history-size`, `copy-on-pick`, `color-picker-shortcut`; your shortcut: `io.github.OlehDzhumyk.Desk.Shortcuts` `color-picker-toggle` |

Skins:

* **windows11**: dark or light Fluent-style preview and toast; the history panel is a flyout above the bottom right corner.
* **macos**: rounded translucent preview; the history panel is a popover at the pointer.
* **default**: the history panel uses the shell theme's popup colours and accent; a popover at the pointer.

All three follow the light/dark scheme, the GNOME accent colour and high contrast (outlines). The fades last 80 ms and
do not run with reduced motion. The shortcut works only in the normal session mode, never on the lock screen.

## On-screen displays

| | |
|---|---|
| Shows | the volume, mute, microphone, brightness and keyboard backlight changes that GNOME Shell reports to `Main.osdWindowManager` (`show`, `showOne`, `showAll`): symbolic icon, level bar, number (`show-percentage`; always with high contrast) |
| Above 100 % | the part above 100 % has its own colour, a marker at 100 % and the words «понад 100 %»; the number stays visible |
| Where | the monitor with the pointer (the primary one as a fallback); `position`: `skin-default`, `bottom-center`, `center`, `top-right` |
| Timing | hides `timeout-ms` (1500) after the last change; a repeat updates the same OSD and restarts its one timer; fade 120 ms in, 150 ms out, none with reduced motion |
| Input | click-through (`reactive: false`), never takes the focus, not in the window list |
| Accessibility | the surface has the notification role; its name says the kind and the value («Гучність 40 %», «Мікрофон вимкнено») |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Osd`: `enabled`, `show-percentage`, `timeout-ms`, `position` |

Skins:

* **windows11**: compact acrylic bar at the bottom centre above the panel: icon, horizontal bar in the accent colour, number.
* **macos**: translucent square in the centre with a large icon over a smooth bar; a pill at `bottom-center` or `top-right`.
* **default**: GNOME's own OSD; the bridge does not touch `Main.osdWindowManager`. The same with `enabled` off, or when another extension has already replaced its methods (one warning in the log).

## Text extractor (OCR)

| | |
|---|---|
| Open | Windows 11 theme: **Shift+Super+T**; macOS theme: **Ctrl+Shift+Super+T**; Fedora: **Shift+Super+T** (`ocr-shortcut` in the theme's `desk.dconf`; override on «Keyboard shortcuts» or `themes shortcut set ocr-toggle ...`). Normal mode only: never on the lock screen |
| Select | The screen dims with a crosshair cursor; drag over the text, or press an arrow key (a rectangle appears; arrows move it, Shift+arrows resize, Ctrl for 1 px steps) and `Enter`; `Esc` or a right click cancels and nothing runs; regions under 8 px are ignored |
| Recognise | `deskd` runs `tesseract` locally on a 0600 PNG in `$XDG_RUNTIME_DIR/deskd/ocr/`, deleted when the call ends; `RLIMIT_AS` 1 GiB, no network namespace where the kernel allows it, killed after 20 s; one recognition at a time; regions above `max-megapixels` are refused |
| Result | The text (paragraphs kept, hyphenated line ends joined) goes to the clipboard and a toast says «Copied text: …» (first line, 60 characters); «No text found» leaves the clipboard alone; missing language data is named (`tesseract-langpack-<code>`) and recognition runs with the rest (English as the fallback) |
| History | `record-in-history` off: the text is copied but the clipboard history skips it; private mode always wins |
| Errors | Without `tesseract` the toast names the package at once and no overlay opens; timeout: «Recognition was interrupted» |
| D-Bus | `io.github.OlehDzhumyk.Desk.Ocr` at `/io/github/OlehDzhumyk/Desk/Ocr`: `Recognize`, `AvailableLanguages`, `IsAvailable` |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Ocr`: `enabled`, `languages`, `record-in-history`, `keep-line-breaks`, `max-megapixels`, `ocr-shortcut`; your shortcut: `io.github.OlehDzhumyk.Desk.Shortcuts` `ocr-toggle` |

Skins:

* **windows11**: darker dim and a light-blue frame, like PowerToys Text Extractor; square-cornered toast.
* **macos**: lighter dim, thin white frame and a pill toast.
* **default**: frame in the GNOME accent colour. High contrast gives every skin opaque surfaces and a thick frame.

## Screenshots

The bridge takes the pixels and hands deskd a PNG; deskd checks, scrubs metadata and saves it; the bridge draws the
toolbar, countdown, preview thumbnail and recording indicator; `themes capture-edit` is the GTK 4 annotation and
destructive redaction editor.

| | |
|---|---|
| Shortcuts | Windows 11 theme: **Shift+Super+S** region, **Super+Print** screen, **Print** toolbar, **Shift+Super+R** record; macOS theme: **Shift+Super+4**, **Shift+Super+3**, **Shift+Super+5** (record from the toolbar); Fedora: none, GNOME's own keys stay (`capture-*-shortcut` in the theme's `desk.dconf`; overrides `capture-region`, `capture-screen`, `capture-toolbar`, `capture-record`) |
| Saving | `Screenshot YYYY-MM-DD HH-MM-SS.png` in local time in `save-folder` (empty: `Screenshots` in the pictures folder); `-2`, `-3` for the same second; atomic, never replaces a file |
| Checks | PNG structure and CRCs; more than 100 megapixels is refused from the header, before any decode |
| Privacy | `tEXt`, `zTXt`, `iTXt`, `tIME` and `eXIf` chunks are removed (no user name, host name or path in the file); redaction (solid or pixelate) replaces the pixels, and the annotated file has one layer and no metadata |
| D-Bus | `io.github.OlehDzhumyk.Desk.Capture` at `/io/github/OlehDzhumyk/Desk/Capture`: `SaveScreenshot(ay) → s`, `ApplyAnnotations(s path, s json) → s` |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Capture`: `enabled`, `save-folder`, `copy-to-clipboard`, `show-preview`, `delay`, `include-pointer`, `record-format`, `record-audio`, `annotate-after-capture`, `capture-region-shortcut`, `capture-screen-shortcut`, `capture-toolbar-shortcut`, `capture-record-shortcut` |
| Core | crate `desk-capture`: pure Rust, no GTK, GLib or D-Bus, so other desktops and a CLI can reuse it (ADR 0011) |
| Toolbar | Modes Region (`1`), Screen (`2`), Record (`3`), delay off / 3 s / 10 s, Close (`Esc`); `Tab`, `Shift+Tab` and the arrow keys move the focus, `Enter` or `Space` presses; a right click closes |
| Region | the shared selector: drag, or arrow keys (`Shift`: resize, `Ctrl`: 1 px) and `Enter`; `Esc` cancels and nothing is saved |
| Delay | a countdown toast; `Esc` (grabbed only while it runs) or the shortcut again cancels; no modal grab, so a menu can be opened and captured |
| Result | PNG to deskd (`SaveScreenshot`); with `copy-to-clipboard` the same PNG goes to the clipboard as `image/png` first, so a failed save keeps the image there |
| Editor | `themes capture-edit <path.png>` (started by `annotate-after-capture` or clicking the macOS thumbnail): GTK 4 / libadwaita window with Pen (**P**), Highlighter (**H**), Arrow (**A**), Line (**L**), Rectangle (**R**), Ellipse (**E**), Redact solid (**X**) and pixelate (**M**), Crop (**C**); colours (accent first, named for screen readers), line width, zoom (**Ctrl+=**, **Ctrl+−**, **Ctrl+1**, fit **Ctrl+0**); **Ctrl+Z** / **Ctrl+Shift+Z** undo and redo; **Ctrl+C** copies the PNG; **Ctrl+S** saves over the file atomically, **Ctrl+Shift+S** saves as; **Esc** closes and asks «Зберегти зміни?» only when edited. The saved file is one RGBA PNG without metadata: redaction destroys the pixels, no layer or undo history is kept |
| Recording | GNOME's screencast service (`org.gnome.Shell.Screencast`): full screen, pointer as `include-pointer`; file in `~/Videos/Screencasts`, encoder and extension chosen by GNOME; a top-bar item shows the elapsed time, a click or `Enter` stops it |
| Lock screen | the shortcuts are bound in normal mode only; locking closes an open toolbar, selector or countdown |

Skins:

* **windows11**: a compact Fluent bar at the top of the monitor (icons only, accessible names), light-blue focus
  ring; a toast «Screenshot saved» / «Screenshot copied and saved».
* **macos**: a translucent capsule near the bottom (like Shift+Cmd+5); after a capture a floating thumbnail at the
  bottom right for 6 s: it stays while the pointer is on it, a click opens the editor, a right click (or `Esc`
  when it has the focus) dismisses it; it slides in only when animations are enabled.
* **default**: a shell-styled bar near the bottom and the toast. High contrast gives every skin opaque surfaces,
  an outline and a thick focus ring; `show-preview=false` shows nothing after a capture. The editor follows the
  libadwaita style and the accent colour in every skin.

## Quick Look

Status: **partial** (stage 4). `themes quick-look <paths>…` opens the previewer, and the bridge binds the shortcut;
the hand-over from Files (Space on the selection) comes later. Spec: [quick-look.md](spec/features/quick-look.md).

| | |
|---|---|
| Open | `themes quick-look FILE…` (a separate, short-lived process, so a crashing decoder never touches the shell); shortcut `quick-look-shortcut` (Windows 11 theme: **Ctrl+Space**; macOS theme: **Super+Y**, as Cmd+Y; Fedora: none, the Space key of Files and Sushi stay); override `quick-look-toggle`. Plain Space is never bound: shell keybindings are global |
| Which files | the bridge binds the shortcut in normal mode only (not in the overview, not on the lock screen). Files has no API for its selection, so the shortcut previews the local files on the clipboard: copy them in Files (**Ctrl+C**), then press the shortcut. A second press closes the preview it opened. With no copied local file, a toast says what to do and no window opens. The clipboard is read only on the key press; no file name is logged |
| Content | images (fit to the window, **+**/**−** zoom, **0** fit; SVG as an image), text and code (monospace, read-only; only the first `max-text-kb` KiB are read, then «Показано початок файлу»), folders (folders first, with sizes, up to 5000 entries), a details card for other types; FIFOs and devices are never opened |
| Keys | **Space** or **Esc** closes; **Left**/**Right** (**Up**/**Down**) move through the given files with a «3 з 3» counter (`follow-selection`); **Return** opens in the default application and closes; **I** toggles the details (type, size, date, link target, location) |
| Safety | files are only read: scripts and HTML are shown as text, never run or rendered; a missing or unreadable file shows «Не вдається прочитати файл» with the reason |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.QuickLook`: `enabled`, `quick-look-shortcut`, `max-text-kb` (64 to 8192, 1024), `autoplay-media` (reserved), `follow-selection` |
| Gaps | no hand-over from Files yet (`org.gnome.NautilusPreviewer` D-Bus; taking that name over would break Sushi, so the bridge leaves it to Sushi), no video or audio player (GStreamer), no PDF or archive view; no open/close animation at all, so reduced motion needs nothing |

Skins:

* **macos**: a compact dark title strip with the name and «Відкрити в…», no navigation buttons (arrows only).
* **windows11**: a Peek-style header with `<` `>` buttons, the name and counter, and an accented «Відкрити в…».
* **default**: a plain libadwaita window with the name, the counter and «Відкрити в…».

## Window layouts

Status: **implemented** (stage 4). deskd answers the layouts and their geometry; the bridge draws the picker, the
zone preview and snap assist, moves the windows and toggles always-on-top. Not yet: the hover picker on the
maximise button and drag zones (GNOME's edge tiling stays in use). Spec:
[window-layouts.md](spec/features/window-layouts.md).

| | |
|---|---|
| Layouts | built-in `halves`, `thirds`, `two-thirds` (2/3 + 1/3), `quarters`, `focus` (25 % / 50 % / 25 %), `stack` (main 60 % left, two stacked on the right); custom layouts in `~/.local/share/deskd/layouts.json` (0600) |
| Geometry | zones are fractions of the work area; a portrait work area turns columns into rows; `gap-outer` stays free along the edges and `gap-inner` separates neighbours exactly; coordinates and sizes are even logical pixels and never leave the work area; a window larger than its zone is fitted inside the work area (AC-WIN-07) |
| Per monitor | the active layout of each monitor key (default `halves`) is saved in `layouts.json` |
| Shortcuts | `layout-picker-shortcut` (Windows 11 and Fedora: **Super+Z**; macOS: **Ctrl+Super+Up**), `always-on-top-shortcut` (**Ctrl+Super+T**); overrides `layout-picker`, `always-on-top` in `io.github.OlehDzhumyk.Desk.Shortcuts` |
| D-Bus | `io.github.OlehDzhumyk.Desk.Layouts` at `/io/github/OlehDzhumyk/Desk/Layouts`: `ListLayouts() → s`, `ActiveLayout(s) → s`, `SetActiveLayout(s, s)`, `ResolveZones(s, i, i, i, i) → s`, signal `Changed` |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Layouts`: `enabled`, `layout-picker-shortcut`, `always-on-top-shortcut`, `gap-outer`, `gap-inner`, `snap-assist`, `hover-picker` |
| Theme defaults | `windows11`: gaps 8 / 6 px, snap assist and hover picker on; `macos`: snap assist and hover picker off; `fedora`: gaps 0, hover picker off |
| Core | crate `desk-layouts`: pure Rust (only `serde`), no GTK, GLib or D-Bus, so KDE or wlroots adapters can reuse it (ADR 0011) |
| Picker | the shortcut opens a modal popover for the focused window with a miniature of every layout; arrows choose a layout, `Enter`/`Space`/`Tab` go into its zones, arrows move between zones (past the edge into the next layout), `Enter`/`Space` snaps, `Tab` goes back, `Esc` or a click outside closes; names «Layout <name>, zone <n> of <m>»; the choice becomes the monitor's active layout |
| Snapping | only normal windows (never dialogs, tool windows, override-redirect or skip-taskbar surfaces, AC-WIN-05); maximised or fullscreen windows are restored first; a window that cannot be resized is centred in the zone at its own size |
| Zone preview | hovering or focusing a zone shows a click-through translucent rectangle with a light-inside-dark outline (3:1 on any background); fade 80 ms, none under reduced motion (AC-WIN-06) |
| Snap assist | with `snap-assist` on, the other normal windows of the workspace (live miniature and title, up to 9) are offered for each empty zone in turn; `1`..`9`, a click or `Enter` places, `Esc` skips |
| Always on top | the shortcut pins or unpins the focused normal window (Mutter `make_above`) with a toast «Pinned on top» / «Unpinned»; per session only |
| Monitors | a monitor change or the lock screen closes the picker, assist and preview (AC-WIN-04) |

Skins: **windows11** a compact Fluent popover at the top of the monitor, miniatures without names; **macos** a
translucent «Move & Resize» popover at the top; **default** the shell theme's popup surface in the middle. High
contrast gives opaque surfaces, outlines and a thick focus ring in every skin.

## Search

Status: **partial** (stage 4). deskd answers queries and the bridge draws the search box. No theme sets
`search-shortcut` yet, so the macOS look keeps `spotlight@nin` until the box is tested on a live session.
Spec: [search.md](spec/features/search.md).

| | |
|---|---|
| Query language | plain text: all providers; `= expr` (or a bare expression such as `2+2*3`): calculator; `> text`: system actions; `. text` or `file:text`: files; `# text`: clipboard history; `: text`: emoji. Only the first 256 characters are matched |
| Providers now | applications (XDG desktop entries, localized `Name[uk]`, `GenericName`, `Keywords`; hidden, `NoDisplay` and other-desktop entries skipped), system actions (lock screen, suspend, log out, restart, power off, toggle dark mode, toggle clipboard private mode), calculator, emoji (the emoji picker's data). Files and clipboard history answer nothing yet |
| Calculator | `+ - * / % ^`, parentheses, unary signs, decimals (point or comma); `= 12*(3+4)` gives `84`; arithmetic only, never shell or script input (AC-SRCH-03) |
| Ranking | exact > prefix > word prefix > substring > fuzzy (typos, letters in order); Unicode case folding; a query typed in the wrong layout (Ukrainian ЙЦУКЕН ↔ QWERTY, both ways) matches too (AC-SRCH-04); ties broken by usage with a 14-day half-life, then applications before actions |
| Usage | `~/.local/share/deskd/search-usage.json` (0600, atomic); with `remember-usage=false` nothing is written and the file is deleted (AC-SRCH-07); `ClearUsage` forgets everything |
| Web | off by default (`web-search`, AC-SRCH-06); when on, the last result only carries the query for the browser; deskd makes no request |
| Box | the field has the focus; results grouped under «Top hit», then by kind (Calculator, Applications, System actions, Files, Clipboard, Emoji, Web), icon, title and subtitle; `↑`/`↓` (and `PgUp`/`PgDn`) move, `Enter` opens, `Super+1`..`Super+9` open the first nine rows, `Esc` clears the field and closes it when empty, a click outside closes; «Nothing found for “…”» when empty |
| Open a result | application: launched (`Shell.AppSystem`, desktop entry as fallback); calculator: copied, with a «Copied …» toast; system action: lock, suspend, log out, restart and power off through the screen saver, logind and the session manager (which asks first), dark mode and clipboard private mode toggled with a toast; file: opened in its default application (local `file://` only); clipboard entry: put back (pasted with `paste-on-select`); emoji: inserted into the focused window without changing the clipboard; web: the query opens in the default browser on DuckDuckGo. Then `RecordLaunch` (best effort) |
| Shortcut | `search-shortcut` (empty by default, so it does not clash with `spotlight@nin` or the overview); override `search-toggle` in `io.github.OlehDzhumyk.Desk.Shortcuts`; bound in normal mode only (never in the overview or on the lock screen); listed in the Shortcut Guide |
| D-Bus | `io.github.OlehDzhumyk.Desk.Search` at `/io/github/OlehDzhumyk/Desk/Search`: `Query(s, u) → s` (JSON results), `RecordLaunch(s)`, `ClearUsage()` |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.Search`: `enabled`, `search-shortcut`, `web-search`, `max-results` (5 to 50, 12), `remember-usage` |
| Idle cost | none: applications are read once and dropped when an `applications/` folder changes (GIO file monitor, no polling) |
| Core | crate `desk-search`: pure Rust (only `serde`), no GTK, GLib or D-Bus, so KDE or wlroots adapters can reuse it (ADR 0011) |

Skins (bridge `src/search/`):

* **macos**: a Spotlight-like wide translucent pill, centred, its top at 22% of the monitor height; `⌘1`..`⌘9` hints.
* **windows11**: a Fluent panel (8 px corners, accent line under the field) in the middle of the monitor; `Super+1`..`Super+9` hints.
  The start-menu flyout with filters and pinned apps is still to come.
* **default**: a GNOME (Adwaita) popover in the upper third, drawn by the shell theme (`popup-menu-content`).
  The overview search provider is still to come.

All skins follow the light colour scheme, high contrast (opaque surface, outlines, a thick ring on the selected
row) and reduced motion (a fade of 100 ms, none when animations are off). The box opens on the monitor with the
pointer, and each row's accessible name carries its group.

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
| Wallpaper palette | token themes whose libadwaita stylesheet is generated (e.g. `lavender`, themes from `from-image`) also get the seed's full light/dark palette: `themes tokens overlay SEED` (run by `deskd`) renders the theme's tokens with the palette on top into `~/.cache/desktop-themes/wallpaper-overlay/` and writes it into the copied `~/.config/gtk-4.0/gtk.css`. Radii, fonts, the shell layer and the terminal stay the theme's. Turning the mode off, applying another look, `themes reset` and `themes doctor --fix` (with the mode off) put the theme's own stylesheet back and delete the cache; a gtk.css you edited is never touched ([ADR 0010](adr/0010-live-wallpaper-palette-overlay.md)). Hand-made stylesheets (`macos`, `windows11`) keep accent-only |
| Images | every gdk-pixbuf format; animated images use the first frame; slideshow XML uses its first image; images under 16×16 or fully transparent are refused |
| Entry points | CLI (`themes tokens from-image`, `themes tokens seed`, `themes tokens overlay SEED / --remove`), window («Appearance» page: switch, source, current accent and palette, create button), quick settings («Theme» menu, per skin, below), D-Bus `WallpaperAccent()` (with the theme whose palette follows) |

Skins (quick settings «Theme» menu; every control writes `accent-from-wallpaper`, `deskd` does the work):

* `macos`: «Accent colour» row of dots, the nine GNOME accents and a rainbow «From wallpaper» dot
  (like Multicolour). A colour dot sets `accent-color` and turns the live mode off; the rainbow dot
  turns it on. The dots are radio buttons for screen readers; Tab and the arrow keys move between
  them. Without `accent-color` (GNOME before 47) the switch is shown instead.
* `windows11`: «Accent colour: Automatic / Manual» drop-down; «Automatic» turns the live mode on,
  «Manual» turns it off (the previous accent comes back).
* `default`: the «Accent from wallpaper» switch.

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

## Shortcut guide

| | |
|---|---|
| Open | Hold **Super** alone for `hold-delay` (default 900 ms); or press **Ctrl+Shift+Super+/** (`shortcut-guide-shortcut`; override `shortcut-guide-toggle` on «Keyboard shortcuts») to open directly in sticky searchable mode |
| Content | Read live at open time from `org.gnome.desktop.wm.keybindings`, `org.gnome.shell.keybindings`, `org.gnome.settings-daemon.plugins.media-keys` (including custom keybindings) and `io.github.OlehDzhumyk.Desk.*` feature schemas + `Desk.Shortcuts` overrides. Grouped into **Windows and apps**, **Desktop**, **Features**, **Input**, **Custom**; unbound entries are hidden; each row shows action name, keycaps and source («theme» or «your setting» when `show-owner` is on) |
| Interaction | A tap of Super keeps its usual action (overview). Another shortcut pressed before the delay cancels the guide and runs. While shown by the hold: releasing Super closes the guide without opening the overview (the bridge filters mutter's overlay-key binding for that one release); another shortcut closes the guide and still runs; any other key, a click or `Esc` closes it. With `sticky` on or via the accessible shortcut it is modal: type to filter, `Up`/`Down`/`Page Up`/`Page Down` to scroll, `Esc` or click outside to close |
| Lock screen | Never opens while the session is locked; locking closes an open guide |
| Animation | 100 ms fade in; instant with reduced motion or animations disabled |
| Settings | GSettings `io.github.OlehDzhumyk.Desk.ShortcutGuide`: `enabled` (false by default; true in `windows11` and `macos` themes), `hold-delay` (400..2000 ms, 900), `sticky` (false), `show-owner` (true), `shortcut-guide-shortcut`; your shortcut: `io.github.OlehDzhumyk.Desk.Shortcuts` `shortcut-guide-toggle` |

Skins:

* **windows11**: dimmed full-screen multi-column grid by group, rounded square keycaps and a neutral `Super` label.
* **macos**: centred translucent cheat-sheet panel with grouped headings, search field in sticky mode and `⌘ ⌥ ⇧ ⌃` modifier symbols.
* **default**: same centred panel with plain-text keycaps; off by default until enabled in Settings.
