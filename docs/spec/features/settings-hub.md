# Settings hub «Робочий стіл» (Desktop)

## Summary

One window for everything desktop-themes-rs does: theme switching and mixing, every feature's settings, appearance, language, credits and the
doctor. Today it is the window «Теми» with pages for themes, mixing, «Функції» (feature settings) and credits. The hub is the **only** place that
holds settings (principle 1); its pages are built from the **settings registry**, so a new setting appears in the schema, the window, the docs and
`gsettings` at once.

## Status

**Partial.** Implemented on `main`: the window with «Теми», «Змішати», «Функції» (clipboard page, credits). Stage 1.5 (in review: the settings
registry in `crates/desk-settings`, English source strings, Ukrainian `po/uk.po`, language override, `--page general`, generated
`docs/settings.md`). **Planned** (stage 2 and later): renaming to «Робочий стіл», the sidebar of all feature pages, search, per-page reset, shortcut
overrides, the quick-settings and launcher entries. Origin badge: *Original*.

## Reference behaviour

* **Windows 11 Settings:** a left navigation list of categories, a search box on top that finds any setting and jumps to it, a page of cards/rows with
  switches and drop-downs; changes apply at once; "Reset" buttons on some pages.
* **macOS System Settings:** a sidebar with a search field, grouped panes, rows with toggles and pop-up buttons, changes apply at once; "Reset"
  per pane rare.
* **GNOME Settings:** libadwaita sidebar and preferences pages: our baseline toolkit.

## Our behaviour

### Structure (registry-driven)

* The registry declares **Features** (id, title, origin badges), **Pages** (id, title, subtitle, icon, section `Features` | `App` | `About`, kind
  `Settings` | `Credits`, groups of rows) and **Keys** (schema, name, type, default, range, label, description, widget `Edit` | `Display` |
  `Hidden`). The window builds a page from its groups; rows follow the key's kind: switch for bool, spin button for ranged numbers, drop-down for choices,
  entry for strings, a shortcut row for shortcut lists, a list editor for string lists.
* Pages (target order): **Themes** (cards), **Mix** (modules), **Appearance** (light/dark mode, accent, from-wallpaper: see [auto-dark](auto-dark.md),
  [theme-from-wallpaper](theme-from-wallpaper.md)); **Features** section, one page per feature in the feature map order: Clipboard, Emoji, Screenshots,
  OCR, Colour picker, Quick Look, Window layouts, Shortcut guide, Focus, Mixer (sound), Search, Phone, Lock screen, Sounds; **App**: General (language), System
  (doctor, reset, baseline info); **About**: Credits (from CREDITS.md), About. A page of a feature that is not installed or **Planned** is hidden.
* Every feature page starts with its **origin badge** (the registry's `origins`: «Є в Windows 11 і macOS», «Засновано на ... (ліцензія)», ...),
  a one-sentence description, and the groups: the enable switch first, then settings, then **Privacy** (what is stored, "Clear" actions with
  confirmation), then **Shortcuts**.
* The hub is reachable: from quick settings («Відкрити «Робочий стіл»…»), from the launcher (`.desktop` entry), from `themes gui [--page ID]`, and with
  `Super+I` in the `windows11` skin ([behavior](behavior.md) registry). Single instance: a second launch raises the window.

### Behaviour of rows

* **Live apply:** changing a row writes the GSettings key immediately; deskd, the bridge and the window react through `changed` signals; no «Застосувати»
  button (except theme apply, which is an action). A write failure shows a toast and restores the displayed value.
* **Display rows** show a value that the theme sets (skin, today also shortcut) with «Задається темою» and a link to the Themes page.
* **Shortcuts are overridable per feature** (decision): a shortcut row shows the current key; activating it records a new one (`Esc` cancels,
  `Backspace` clears to «вимкнено», «Скинути» restores the theme default); the picked key is checked against the [behavior](behavior.md) registry
  and GNOME's keys: on conflict the row shows «Збіг із <action>» and refuses unless the user confirms the swap. A user-set shortcut is stored in the
  feature's own key and survives theme switches (the theme manager skips keys the user set explicitly; `themes reset` clears them).
* **Reset:** each page has «Скинути сторінку» (to schema defaults for its keys, after confirmation) and the App > System page has «Скинути все»
  (themes `reset` plus all feature keys).
* **Search:** a search field in the header (`Ctrl+F`) matches setting labels, descriptions and page titles (Unicode case-folded, Ukrainian and English
  both), shows grouped hits, and `Enter` jumps to the row and highlights it for 1.5 s (no highlight animation under reduced motion: a static outline).
* **Language** (`io.github.OlehDzhumyk.Desk language`: `system` | `uk` | `en`): the change takes effect after the window restarts (today) and live for the shell
  bridge after re-enable; the target is live for all ([principles](../principles.md#internationalisation)).
* **Doctor and crash banner** stay at the top of every page (banner «GNOME вимкнув розширення після збою оболонки»); see [doctor](doctor.md).
* **Experimental themes** show a badge and need confirmation before apply ([liquid-glass](liquid-glass.md)).

### Skins

The hub is a GTK 4/libadwaita window and follows the system (dark/light, accent, text scale); it is **not** skinned by theme except through the
token-generated libadwaita stylesheet the active theme installs. In the `windows11` skin, row spacing and the header may use Windows-like proportions only
through that stylesheet.

### Keyboard, focus, animation

Standard libadwaita: `Tab`/arrows, `Ctrl+F` search, `Ctrl+W` close, `F10` main menu, `Ctrl+,`-style preferences not needed. The sidebar is a single tab stop with arrow
navigation; page changes keep the focus on the page title; dialogs trap focus and return it. Page transitions are libadwaita's and follow
`enable-animations`.

### Empty and error states

Schema missing: page «Налаштування недоступні» («Схему GSettings ... не встановлено») with the install hint (today's behaviour on the clipboard page);
deskd unreachable: the row «Служба deskd» shows «недоступна: <reason>» and the page still edits GSettings; no themes: «Тем не знайдено».

## Settings

The hub itself: `io.github.OlehDzhumyk.Desk language` (default `system`) and the registry-generated `docs/settings.md`, the authoritative key list. Window size is not
persisted by us (GTK/libadwaita state only).

## Privacy & security

The hub shows and edits local settings only; it sends nothing. «Clear» actions are real deletion with a confirmation naming what is lost. No setting is hidden
from the user that affects privacy (storage, sync, network features); network-using features are **off by default** and say so on their page.

## Edge cases

* GSettings keys changed from outside (`gsettings set`): rows update live.
* Two windows: prevented (single instance via the `...Themes.Window` D-Bus interface).
* A key with a range violation set from outside: GSettings coerces it to the default; the row shows the effective value.
* A feature whose engine is installed but disabled: the page is visible with the enable switch off and the other rows insensitive.
* RTL languages: not supported yet (non-goal); layout uses logical start/end.
* Very long translations: rows wrap, never truncate labels.

## Accessibility

All rows are libadwaita rows with accessible names = labels and descriptions; shortcut recording announces the captured key; error states are text;
contrast and text scaling follow the system; search results are announced; no information by colour alone; every action reachable by keyboard.

## Performance

Window shows within 200 ms on the reference laptop (heavy work such as `doctor` or listing deskd status runs off the main thread); pages are built lazily;
resident cost zero when closed (the window is a short-lived process; only the D-Bus single-instance claim exists while open).

## Gaps found

* **G-HUB-1** The window is still named «Теми» and has no sidebar of feature pages; «Функції» lists only the clipboard and credits.
* **G-HUB-2** Shortcut rows are read-only («Задається темою») and `excluded-apps` is not exposed ([clipboard](clipboard.md) G-CLIP-9, [behavior](behavior.md) G-BEHAV-5).
* **G-HUB-3** No settings search, no per-page reset.
* **G-HUB-4** The language setting needs a window restart (documented in the page text) instead of applying live.
* **G-HUB-5** No `--page` for the pages of planned features (they do not exist yet), and no test that every registry page opens (a headless test should open each).

## Acceptance criteria

* AC-HUB-01: Given the registry, When `themes gui --page clipboard` opens, Then every key with widget `Edit` of the clipboard schema has a row and none with `Hidden`.
* AC-HUB-02: Given the clipboard page, When `history-size` is changed to 100, Then `gsettings get ... history-size` is 100 within 200 ms and deskd uses it.
* AC-HUB-03: Given `gsettings set ... private-mode true` from a terminal, Then the open page's switch turns on within 1 s.
* AC-HUB-04: Given the `windows11` theme, Then the clipboard shortcut row shows `Super+V` with «Задається темою» (stage 1.5), and after the target feature, a user override
  survives a theme switch.
* AC-HUB-05: Given the shortcut recording picks `Super+Tab` already owned by the behaviour registry, Then the row shows the conflict and refuses without confirmation.
* AC-HUB-06: Given a search for «буфер», Then the clipboard page and its rows are listed and `Enter` jumps to the first.
* AC-HUB-07: Given «Скинути сторінку» confirmed, Then every key of the page equals its schema default.
* AC-HUB-08: Given `DESKTOP_THEMES_LANGUAGE=en`, Then all labels are English; with `uk`, Ukrainian.
* AC-HUB-09: Given the schema is not installed, Then the page shows the unavailable state and the window does not crash.
* AC-HUB-10: Given two launches, Then one window exists.
* AC-HUB-11: Given `enable-animations=false`, Then no row highlight animation runs.
* AC-HUB-12: Given any page, Then all controls are reachable by `Tab` and have accessible names (automated AT-SPI check).
* AC-HUB-13: Given `docs/settings.md` is regenerated, Then it equals the committed file (the registry test).

## Open questions

1. Final order and grouping of the sidebar; whether Themes and Mix are one page.
2. Does the hub also host a "Shortcuts" overview page (all shortcuts in one table, with overrides), or only per-feature rows plus the [shortcut-guide](shortcut-guide.md)?
3. Name in English: "Desktop" vs "Desktop Features"; the `.desktop` file and window title follow.
4. Live language switch for the window (rebuild widgets) vs restart.
