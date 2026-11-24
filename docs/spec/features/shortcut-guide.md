# Shortcut guide (hold Super)

## Summary

Hold the `Super` key for about a second and an overlay lists the keyboard shortcuts that are **actually active** in the
current theme, grouped, searchable; releasing the key closes it. It makes the registry in [behavior](behavior.md)
discoverable.

## Status

**Implemented** (stage 4). Origin badge: *From Windows, adapted for macOS style* (Windows: PowerToys Shortcut Guide;
macOS has no system-wide guide, apps show their own, and the Cmd-hold cheat sheet exists only in third-party tools).

How the hold is detected (mutter gives key events to the focused client before the stage, so a stage event handler
never sees them): the bridge grabs `Super_L` / `Super_R` as accelerators, which fire on press because mutter does not
consume the overlay key's press; another shortcut is seen through the shell's `filter-keybinding` signal before it runs;
the release of `Super` alone asks `filter-keybinding` about the overlay combo just before `overlay-key` is emitted, and
the bridge answers «filtered» only when the guide was shown. No shell function is patched.

Remaining gaps:

* **Gap (focus while held):** once shown by the hold, the overlay holds a grab in `Shell.ActionMode.NORMAL` so it sees
  the release of `Super` (mutter shortcuts still run first); the focused window loses the keyboard while the guide is
  visible, unlike the «non-modal» wording below.
* **Gap (unbound keys before the delay):** a key with no shortcut pressed together with `Super` goes to the focused
  window and is not reported to the bridge. When the delay ends the bridge checks the modifier state once: the guide
  opens only while `Super` alone is still held. Holding on after `Super`+<unbound key> or a `Super`+click still opens it.
* **Gap (non-US keycap names):** `bridge/src/shortcutGuide/reader.ts` maps key symbols such as `Above_Tab` through a
  static table rather than querying the active XKB keymap for the current layout's glyph.
* **Gap (owner of GNOME keys):** only Desk feature shortcuts know their source (`Desk.Shortcuts` override); rows read from
  GNOME schemas are always labelled «theme».
* **Gap (headless E2E tests):** automated tests in `tests/e2e/` for AC-GUIDE-01..08 (in particular that the overview
  stays closed on the `Super` release across Shell updates) are not written yet; the trigger was checked against the
  mutter and GNOME Shell sources only.
* Per-application shortcuts (reading menus of the focused app) are out of scope.

## Reference behaviour

* **Windows (PowerToys):** holding the Windows key (about 900 ms) shows a full-screen overlay with the shortcuts for
  Windows, the active app (where it declares them) and the taskbar; it closes on release.
* **macOS:** no system overlay; the Help menu search finds menu commands and their shortcuts per app.

## Our behaviour

### Trigger

`Super` held alone for `hold-delay` (default 900 ms). Any other key pressed before the delay cancels the guide (it is a
modifier of a shortcut). A tap (< delay) keeps its normal effect (overview, ArcMenu, launcher). The guide never
steals a real shortcut: while it is shown, pressing a shortcut key still runs it and closes the guide.

### Content (live, from the system)

Read at open time, not from files in the theme: the current values of GSettings keys listed in the registry
(`wm.keybindings`, `shell.keybindings`, media-keys incl. custom bindings, our features' `toggle-shortcut`
keys), so user overrides appear correctly. Groups: **Windows and apps**, **Desktop** (overview, launcher, quick
settings, notifications), **Features** (clipboard, emoji, screenshots, OCR, colour picker, layouts, mixer), **Input**
(layout switching), **Custom**. Each row: action name (translated), the key caps, the owner (theme or "your setting").
Unbound entries are hidden.

### Skins

* **`windows11`:** dimmed full-screen overlay, columns by group, keycaps as small rounded squares, a Windows-key glyph
  drawn by us (not Microsoft's artwork: a plain "Super" label or a neutral key icon).
* **`macos`:** centred translucent panel (as Apple would present a cheat sheet): a search field on top, list grouped with
  headings, symbols `⌘ ⌥ ⇧ ⌃` for Super/Alt/Shift/Ctrl.
* **`default`:** same panel as `macos` with plain text keys; **off by default** (opt-in).

### Interaction

Overlay appears while the key is held; releasing closes it (default). `sticky` mode (setting) keeps it open until
`Esc`; in sticky mode typing filters the list and `Up/Down` scroll. `Esc` always closes. Focus is not taken from the
active window while held (the overlay is non-modal); sticky mode grabs the keyboard.

### Animation

Fade in <= 120 ms after the delay; instant with reduced motion.

### Empty and error states

Registry unreadable: the overlay shows the keys it could read plus «Деякі скорочення недоступні». No custom shortcuts:
the group is hidden.

## Settings (`io.github.OlehDzhumyk.Desk.ShortcutGuide`)

`enabled` (bool, default `false`; set to `true` by `themes/windows11/desk.dconf` and `themes/macos/desk.dconf`),
`hold-delay` (uint 400..2000 ms, step 50, default 900), `sticky` (bool, default `false`), `show-owner` (bool, default
`true`), `shortcut-guide-shortcut` (accel, default `['<Control><Shift><Super>slash']`; user override
`shortcut-guide-toggle` in `io.github.OlehDzhumyk.Desk.Shortcuts`).

## Privacy & security

Only key bindings (no typed text) are read; nothing is stored; the overlay never logs key events.

## Edge cases

* **Overview conflict:** GNOME opens the overview when `Super` is released without another key. If the guide was
  shown, the release must **not** open it. The bridge must suppress that release (the implementation point most likely to
  break on a Shell update; headless-test it).
* ArcMenu or Spotlight bound to `Super`/`Super+Space`: held `Super` shows the guide first; release closes it without
  triggering them.
* Sticky Keys: a latched `Super` counts as held only with a real press.
* Non-US layouts: key caps show the layout's names for keys like `Above_Tab` ("`" or "'").
* Many custom shortcuts: the list scrolls.
* Lock screen: never.

## Accessibility

Content is plain text rows readable by screen readers on demand (sticky mode, or `Super+/`-style open: *proposed* second
trigger `Ctrl+Shift+Super+/` for users who cannot hold keys); the hold delay is adjustable up to 2 s; contrast per
tokens; reduced motion honoured.

## Performance

Overlay visible <= 16 ms after the delay; the key listener is event-driven (no polling); building the list reads ~100
GSettings keys (< 5 ms); no resident cost besides one key handler.

## Acceptance criteria

* AC-GUIDE-01: Given `windows11`, When `Super` is held 1 s, Then the overlay lists `Super+V` under Features and `Alt+Tab`
  under Windows and apps; on release it closes and the overview does not open.
* AC-GUIDE-02: Given a tap of `Super` (100 ms), Then the usual action runs and no overlay appears.
* AC-GUIDE-03: Given `Super` is held and `E` pressed within 300 ms, Then Files opens and no overlay appears.
* AC-GUIDE-04: Given the user changed the clipboard shortcut to `Ctrl+Alt+V`, Then the guide shows that key.
* AC-GUIDE-05: Given sticky mode and the query "clip", Then only clipboard-related rows remain.
* AC-GUIDE-06: Given reduced motion, Then no fade runs.
* AC-GUIDE-07: Given `default` skin on a fresh install, Then holding `Super` shows nothing.
* AC-GUIDE-08: Given the lock screen, Then nothing appears.

## Open questions

1. Per-application shortcuts (reading menus of the focused app) are out of scope: confirm.
2. Is suppressing the overview on release safe across Shell versions, or should the guide use a different default
   trigger (`Super+/`)?
