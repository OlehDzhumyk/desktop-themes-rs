# Quick settings theme menu

## Summary

A «Тема» (Theme) tile in GNOME's quick settings (the panel menu with Wi-Fi, volume, dark style). Its
sub-menu lists the installed themes, switches the whole theme with one click, shows which theme is
active (or that modules are mixed), and opens the settings window. It is the fast path; fine control
(modules, mixing, snapshots, doctor) stays in the window.

## Status

**Implemented** (stage 1, moved from the former `themes-quick` extension into `desk-bridge`).
Origin badge: *Original*. The tile is the same in every skin (it is GNOME's own widget class).

## Reference behaviour

Windows 11's quick-settings flyout has Night light, Accessibility and so on, but no theme tile;
macOS's Control Center has a Dark Mode and an Appearance toggle. Our tile sits in the same place a
vendor would put "Appearance" and follows the host's quick-settings conventions (a toggle with a
sub-menu, a header with icon, title and subtitle).

## Our behaviour

### The tile

* Title «Тема»; icon `preferences-desktop-appearance-symbolic`; it is a menu toggle that does **not**
  toggle by itself: a click opens its sub-menu (`toggleMode: false`).
* **Checked** (highlighted) when the theme providing the *look* module is not `fedora`.
* **Subtitle:** the active theme's name; "<name> (змішано)" when any module comes from another theme;
  empty while unknown; "служба недоступна" when the theme service cannot be reached; while an apply
  is running "застосування «<name>»…".
* The tile is added by `DeskIndicator` at `enable()` and removed at `disable()`; it never outlives the
  extension.

### The sub-menu

1. Header: icon, «Тема», the subtitle.
2. One row per installed theme, in the order of `List()`; the row shows the theme name and, with
   `Ornament.CHECK`, a check mark when **all four** modules come from it. A theme providing only part
   of the active modules shows "<name> · n/4" and no check mark.
3. A separator and «Відкрити «Робочий стіл»…», which opens the window and closes quick settings.

Activating a row applies the **whole** theme (`Apply(id, [])`, all modules). While an apply is running
further activations are ignored. When it finishes:

* if a re-login is needed: notification «Тема» with the message (re-login reasons);
* else if there are warnings: notification with the message and the warnings;
* on error: error notification «Не вдалося застосувати «<name>»: <error>»;
* in every case the list is refreshed.

The list refreshes when the menu opens, when the service emits `Changed` (any front end), and after
each apply. If the service call fails the list is replaced by one insensitive row «Помилка: <message>».

### Keyboard, focus, screen reader

Standard quick-settings behaviour: reach the tile with Tab from the panel, Enter or Space opens the
menu, Up/Down move, Enter activates a row, Esc closes. Each row's accessible name is
"<name>: модулі <list>". The menu header announces the current subtitle.

### Animation

Only GNOME's own menu open/close animation, which follows `enable-animations`; the tile adds none.

### Empty and error states

* Service not installed or not activatable: subtitle "служба недоступна" plus the error row; the
  window entry remains available (it launches the binary directly if the `.desktop` file is
  missing).
* No themes: an empty list with only the window entry.

## Settings

None of its own. Visibility follows `desk-bridge` being enabled (a permanent extension).

## Privacy & security

Only theme ids, names and module lists cross D-Bus, on the session bus. The tile never applies a
theme without a click. It must not apply an **Experimental** theme without confirmation (see Gaps).

## Edge cases

* Theme applied from the CLI or the window while the menu is open: `Changed` refreshes it in place.
* Apply takes seconds (the deliberate ~1 s pause between dconf and extensions, plus extension
  loading): the tile stays responsive (async D-Bus; the shell main loop is not blocked); a second
  click is ignored, not queued.
* A theme named with markup characters: names are shown as plain text.
* Very long list: the menu scrolls (GNOME menu behaviour).
* `launchGui()` failure shows an error notification «Робочий стіл».

## Accessibility

Keyboard-only operation as above; the check mark is also conveyed as text (accessible name of the
active row includes its modules; the "n/4" suffix is text); tile contrast is the host's.

## Performance

Opening the menu issues two D-Bus calls (`List`, `Current`) and must paint within one frame using the
last known list, then update; budget "panel visible <= 16 ms". The bridge keeps no timers for this
tile.

## Gaps found

* **G-QS-1** The tile applies `macos-glass` (Experimental, crashed GNOME Shell 51.1) with one click and no
  warning (`bridge/src/quickSettings.ts` `_apply`). Intended: Experimental themes are listed with a
  warning suffix and need a confirmation (see [liquid-glass](liquid-glass.md)).
* **G-QS-2** For the default skin, the clipboard tile is inserted with the private `qs.menu.addItem`
  (cast to `any`) because items added after `addExternalIndicator()` are not placed automatically; this
  can break on a Shell update. Doctor should cover it (journal) and a headless test should pin it.
* **G-QS-3** All strings are hard-coded Ukrainian in `bridge/src/quickSettings.ts`; i18n is stage 1.5
  (settings agent).

## Acceptance criteria

* AC-QS-01: Given `macos` is fully active, When quick settings opens, Then the «Тема» tile is checked,
  its subtitle is "macOS", and the `macos` row has a check mark.
* AC-QS-02: Given look from `macos` and behaviour from `windows11`, When the menu opens, Then the
  subtitle ends with "(змішано)" and neither row has a check mark; both show "n/4".
* AC-QS-03: Given the menu is open, When the row "Windows 11" is activated, Then `themes current`
  eventually reports all modules from `windows11` and the tile subtitle shows "Windows 11".
* AC-QS-04: Given an apply is running, When a second row is activated, Then no second `Apply` call is
  made.
* AC-QS-05: Given the theme service is not reachable, When the menu opens, Then the subtitle is
  "служба недоступна" and one insensitive error row is shown and the window entry still works.
* AC-QS-06: Given `Apply` returns `relogin=true`, Then exactly one notification «Тема» with the
  message is shown.
* AC-QS-07: Given `fedora` is the look, Then the tile is unchecked.
* AC-QS-08: Given an Experimental theme row is activated, Then a confirmation is shown first and
  nothing is applied on «Скасувати» (target behaviour, G-QS-1).
* AC-QS-09: Given `desk-bridge` is disabled, Then the tile disappears and no theme-service client
  remains connected.

## Open questions

1. Should the tile have a second row group for "Light / Dark" so the system toggle and the theme
   state are visible together (see [auto-dark](auto-dark.md))?
2. Show a small badge on rows for Experimental themes, or a separate "Experimental" sub-section?
