# Per-theme behaviour and keybindings

## Summary

The **behavior** module of a theme sets how the desktop *acts*: shortcuts, window and application
switching, touchpad and mouse, hot corners, font rendering, window placement, title-bar actions. It is
independent of the look, so the macOS look can have Windows 11 behaviour. This spec is the **registry
of key owners**: one table per theme, and the rules every new shortcut must follow.

## Status

**Implemented** for `fedora` (returns to the baseline), `macos` and `windows11` (`macos-glass` inherits
the `macos` module). Origin badge: *In Windows 11 and macOS* (each theme follows its system's
conventions). The planned shortcuts at the end are **Planned**.

## Reference behaviour

* **Windows 11:** `Alt+Tab` switches windows; `Win+Tab` opens Task View; `Win+D` shows the desktop;
  `Win+E` Files; `Win+A` quick settings; `Win+S` search; `Win+N` notification centre; `Win+V` clipboard;
  `Win+Shift+S` snipping; `Ctrl+Shift+Esc` task manager; `Win+Space` switches keyboard layout; edge snapping on;
  traditional (non-natural) scrolling; title-bar double-click maximises; ClearType (RGB sub-pixel) text.
* **macOS:** `Cmd+Tab` switches applications, ``Cmd+` `` windows of the application; `Cmd+Q` quits, `Cmd+H` hides, `Cmd+Space`
  Spotlight; `Ctrl+Space` switches input source; natural scrolling; hot corners available; title-bar
  double-click minimises (a user setting); grayscale anti-aliasing; no edge snapping by default.

## Our behaviour

### Key registry (what each theme sets)

`Super` is the Windows key / Command. Keys below are what `themes/<id>/behavior.dconf` writes; a cell
"(system)" means the theme sets nothing and the user's baseline (or the GNOME default on a fresh
Fedora Silverblue / GNOME 51.1) applies. "-" means unbound.

| Action (GSettings key) | `fedora` (baseline / GNOME default) | `macos` | `windows11` |
|---|---|---|---|
| Switch applications (`wm.keybindings switch-applications`) | `Super+Tab`, `Alt+Tab` | `Super+Tab`, `Alt+Tab` | - |
| Switch applications backward | `Shift+Super+Tab`, `Shift+Alt+Tab` | `Shift+Super+Tab`, `Shift+Alt+Tab` | - |
| Switch windows of the app (`switch-group`) | `Super+Above_Tab`, `Alt+Above_Tab` | `Super+Above_Tab`, `Alt+Above_Tab` | - |
| Switch group backward | `Shift+Super+Above_Tab`, `Shift+Alt+Above_Tab` | same | - |
| Switch windows (`switch-windows`) | - | - | `Alt+Tab` |
| Switch windows backward | - | - | `Shift+Alt+Tab` |
| Switch input source | `Super+Space`, `XF86Keyboard` | `Super+Alt+Space`, `XF86Keyboard` | `Super+Space`, `XF86Keyboard` |
| Switch input source backward | `Shift+Super+Space`, `Shift+XF86Keyboard` | `Shift+Super+Alt+Space`, `Shift+XF86Keyboard` | `Shift+Super+Space`, `Shift+XF86Keyboard` |
| Close window (`close`) | `Alt+F4` | `Super+Q`, `Alt+F4` | `Alt+F4` |
| Minimise (`minimize`) | `Super+H` | `Super+H` | `Super+H` |
| Show desktop (`show-desktop`) | - | - | `Super+D` |
| Overview (`shell.keybindings toggle-overview`) | - (the lone `Super` key opens it) | - (cleared so `Super+Space` is only the launcher) | `Super+Tab` (Task View) |
| Application grid (`toggle-application-view`) | `Super+A` | `Super+A` | `Super+S` |
| Quick settings (`toggle-quick-settings`) | `Super+S` | `Super+S` | `Super+A` |
| Notification list (`toggle-message-tray`) | `Super+V`, `Super+M` | `Super+V`, `Super+M` | `Super+N`, `Super+M` |
| Focus active notification | `Super+N` | `Super+N` | - |
| Screenshot UI (`show-screenshot-ui`) | `Print` | `Print` | `Print`, `Shift+Super+S` |
| Home folder (`media-keys home`) | - | - | `Super+E` |
| Custom shortcuts (`media-keys custom-keybindings`) | none | none (cleared) | one: `Ctrl+Shift+Esc` starts the system monitor (`win11-system-monitor`) |
| Dash to Dock shortcut (`extensions.dash-to-dock shortcut`) | n/a | cleared | n/a |
| Launcher (look module) | overview search | `Super+Space` (Spotlight launcher, [search](search.md)) | `Super` tap (ArcMenu start menu; `arcmenu-hotkey=Super_L`) |
| Clipboard (look module `desk.dconf`) | `Shift+Super+V` | `Shift+Super+V` | `Super+V` |

Rules: `windows11` frees `Super+V` (notification list moves to `Super+N`) and `Super+Space` stays the
input switch; `macos` clears `toggle-overview` so that `Super+Space` belongs to the launcher alone (both
firing at once could crash GNOME Shell), and moves the input switch to `Super+Alt+Space`.

### Pointer, scrolling, window and text settings

| Setting | `fedora` | `macos` | `windows11` |
|---|---|---|---|
| Touchpad natural scroll | on | on | **off** |
| Touchpad tap to click | on | on | on |
| Touchpad click method | fingers | fingers | **areas** |
| Disable while typing | on | on | on |
| Mouse natural scroll | off | off | off |
| Hot corner (`enable-hot-corners`) | on | on | **off** |
| Font anti-aliasing / hinting | grayscale / slight | grayscale / slight | **rgba / full** |
| Animations (`enable-animations`) | on | on | on |
| Locate pointer (`Ctrl`) | off | off | off |
| Battery percentage in panel | off | **on** | off |
| Clock shows weekday | off | **on** | off |
| Edge tiling (`mutter edge-tiling`) | off on the reference system | off | **on** |
| Attach modal dialogs | off | **on** | off |
| Centre new windows | on | on | on |
| Title-bar double-click | toggle-maximize | **minimize** | toggle-maximize |
| Title-bar middle-click | none | none | none |

Bold cells are what the theme changes from the reference system's defaults. Window buttons are part
of the look module (`macos`: close, minimise, maximise on the left; `windows11`: minimise, maximise, close
on the right).

### Ownership rules (binding)

1. **One owner per key per theme.** A shortcut appears once in a theme's registry; two features on the
   same key are a bug. Before a feature takes a shortcut it must be checked against this table, the GNOME
   defaults (`gsettings range`/schemas), and the planned registry below.
2. **Module precedence.** If look and behaviour set the same key, behaviour wins; sounds and apps come
   last ([themes](themes.md)). Therefore a shortcut a *look* feature needs (launcher, clipboard) is written in
   the look module and must not be also written by behaviour, except to **clear** a competing owner.
3. **Clearing is explicit** (`@as []`), never by omission: a key that must be unbound in a theme is written as
   empty so that it does not fall back to the baseline.
4. **Mixing:** look from theme X with behaviour from theme Y keeps X's look-owned shortcuts (launcher,
   clipboard) and Y's behaviour shortcuts. The combination must not leave two owners on one key; the
   `macos` look + `windows11` behaviour case is therefore tested: `Super+Space` is the launcher *and* the
   Windows 11 input switch, a known collision that the spec resolves by **the look wins for look-owned
   keys**: the launcher is bound and the input source keeps only its second binding (`XF86Keyboard`). **Implemented:** `themes apply` finds
   such clashes in the resulting keys (`themes_core::shortcuts::resolve_conflicts`), removes the accelerator from the
   system key, and warns. The same rule frees `Super+V` for a Windows 11 look on a macOS behaviour.
5. Modifier convention: `macos` maps Command to `Super`, Option to `Alt`; `windows11` uses the Windows key as
   `Super`.
6. Shortcuts are stored as GSettings keys (`<Super>v`, `Above_Tab` etc.), never as scancodes, so layouts work.

### Keyboard navigation inside system UI

Behaviour does not change focus rules: Tab order, `Esc`, arrows in menus and the overview are GNOME's.

### Planned shortcut registry (proposed, binds the Planned specs)

| Feature | `windows11` | `macos` | `default` |
|---|---|---|---|
| Emoji and symbols ([emoji](emoji.md)) | `Super+.` (also `Super+;`) | `Ctrl+Super+Space` | `Super+.` |
| Screenshot, region ([screenshots](screenshots.md)) | `Shift+Super+S`, `Print` | `Shift+Super+4` | `Print` |
| Screenshot, full screen | `Super+Print` | `Shift+Super+3` | `Shift+Print` (GNOME) |
| Screenshot and recording toolbar | `Shift+Super+R` (record) | `Shift+Super+5` | `Print` (GNOME UI) |
| Text from screen ([ocr](ocr.md)) | `Shift+Super+T` | `Ctrl+Shift+Super+T` | `Shift+Super+T` |
| Colour picker ([color-picker](color-picker.md)) | `Shift+Super+C` | `Ctrl+Shift+Super+C` | `Shift+Super+C` |
| Quick Look ([quick-look](quick-look.md)) | `Ctrl+Space` in Files | `Space` in Files | `Space` in Files |
| Snap layouts ([window-layouts](window-layouts.md)) | `Super+Z` | `Ctrl+Super+Left/Right/Up` | GNOME `Super+Arrows` |
| Always on top | `Ctrl+Super+T` | `Ctrl+Super+T` | `Ctrl+Super+T` |
| Shortcut guide ([shortcut-guide](shortcut-guide.md)) | hold `Super` | hold `Super` | hold `Super` (off by default) |
| Focus mode toggle ([focus](focus.md)) | none by default | none by default | none by default |
| Settings hub ([settings-hub](settings-hub.md)) | `Super+I` | none (launcher) | none (launcher) |
| Search ([search](search.md)) | `Super` tap / `Super+S` | `Super+Space` | `Super` tap |

The specs of the individual features give the final key and any exception; collisions with the tables
above are errors and must be resolved in this file first.

## Settings

Behaviour keys are plain GNOME GSettings keys listed above, managed by the theme manager; users who
change one by hand have it overwritten by the next apply of the module (the baseline restores it on reset).
**Decided (owner):** the shortcut of every feature is a **registry entry** (`crates/desk-settings`,
`SHORTCUTS`) edited in **one place**: the window page «Keyboard shortcuts» (`themes gui --page shortcuts`) and
`themes shortcut list|set|reset`. A theme's `desk.dconf`/`behavior.dconf` only supplies the **default**.

* **Precedence:** the user's override (if set) wins over the active theme's default, which wins over the registry
  default (`desk_settings::shortcuts::resolve`). An empty override means "follow the theme"; `['disabled']`
  means "no shortcut".
* **Storage:** two keys per shortcut. The *key in force* (`io.github.OlehDzhumyk.Desk.Clipboard
  toggle-shortcut`, unchanged, so stored values and the bridge keep working) is what the bridge binds; themes
  write their default into it. The *override* lives in `io.github.OlehDzhumyk.Desk.Shortcuts` (`clipboard-toggle`)
  and is not a managed theme key. Every `themes apply` puts the override on top of the theme's value
  (`shortcuts::apply_overrides`); changing the shortcut writes both keys at once. `themes reset` (a full apply of
  `fedora`) forgets the overrides.
* **Reset to theme default** (per shortcut): clears the override and writes the active theme's value back.
* **Conflicts:** choosing a key that another action already owns (our features, the launcher, GNOME keybindings,
  custom keybindings) is **refused** and the owner is named. The same detection runs in `themes apply`.

Supersedes the earlier idea of tracking "user-set" keys in `state.json`: a separate override key is visible to
`gsettings`, survives a lost `state.json` and needs no skip logic in the planner.
Limit: only keys that are *set* in dconf are checked; GNOME defaults of untouched keys are not (G-BEHAV-3).

## Privacy & security

Behaviour is data only; custom keybindings start commands (`gnome-system-monitor`): a third-party theme's
custom keybindings are an arbitrary-command risk and must be shown to the user before applying (see
[themes](themes.md) on hooks). `custom-keybindings` is a **list** key: a theme that sets it replaces the
user's own custom shortcuts for as long as the theme is active (the baseline restores them).

## Edge cases

* `windows11` sets `custom-keybindings` to a one-element list: any custom shortcuts the user created are
  hidden while it is active and return on reset.
* A non-Latin layout active: keysym-based bindings (`<Super>v`) follow the layout's Latin fallback
  (GNOME resolves them on the base layout); verify with a Ukrainian layout.
* `Above_Tab` is the key above Tab (backtick on US, a different symbol elsewhere); the spec talks about the
  physical key.
* Hot corner off and the overview reachable via the `Super` key or `Super+Tab` only in `windows11`.
* Switching theme while the user holds a modifier: bindings change at the next key event; no stuck keys.

## Accessibility

Every shortcut has a pointer path (panel buttons, menus); Sticky Keys and Slow Keys of GNOME work with our
bindings; the hub lists all shortcuts as text so they are discoverable ([shortcut-guide](shortcut-guide.md)).

## Performance

Applying behaviour is dconf writes only (no restart); the shell re-reads keybindings live.

## Gaps found

* **G-BEHAV-1** *(resolved)* the stale comment in `themes/macos/spotlight.dconf` is corrected
  ([themes](themes.md) G-THEMES-3).
* **G-BEHAV-2** *(resolved)* Mixing `macos` look with `windows11` behaviour: `themes apply` detects the two owners
  of `Super+Space`, the launcher keeps it, and a warning is shown (`shortcuts::resolve_conflicts`, test over all
  look x behaviour pairs of the bundled themes).
* **G-BEHAV-3** There is no automated test that checks a theme's registry against the GNOME default
  collisions (`tools/integration-test.sh` writes real dconf and is not run in CI). A headless
  collision test is needed.
* **G-BEHAV-5** *(resolved)* Feature shortcuts are editable in the window and the CLI; overrides survive theme applies.
* **G-BEHAV-4** The `windows11` theme leaves `Super+Down` (GNOME: unmaximise) and `Super+Up` as GNOME
  defaults; Windows 11 restores/minimises on `Super+Down`. Not specified yet (see
  [window-layouts](window-layouts.md)).

## Acceptance criteria

* AC-BEHAV-01: Given `themes apply windows11 --only behavior`, When `gsettings get
  org.gnome.desktop.wm.keybindings switch-windows` is read, Then it is `['<Alt>Tab']` and
  `switch-applications` is `@as []`.
* AC-BEHAV-02: Given `macos` behaviour is active, When `Super+Q` is pressed with a window focused, Then
  the window is closed.
* AC-BEHAV-03: Given `macos` behaviour is active, When `Super+Space` is pressed, Then the launcher opens and
  the Overview does **not** (`toggle-overview` is `@as []`).
* AC-BEHAV-04: Given `windows11` is active, When `Ctrl+Shift+Esc` is pressed, Then the system monitor starts.
* AC-BEHAV-05: Given `windows11` is active, When `Super+V` is pressed, Then the clipboard panel opens and the
  notification list does not (it is `Super+N`).
* AC-BEHAV-06: Given `macos` is active, When `Super+V` is pressed, Then the notification list toggles and the
  clipboard panel does not (it is `Shift+Super+V`).
* AC-BEHAV-07: Given `windows11`, When the touchpad settings are read, Then natural scroll is off and click
  method is `areas`; for `macos` natural scroll is on and click method is `fingers`.
* AC-BEHAV-08: Given `themes reset`, When behaviour keys are read, Then each equals its baseline value.
* AC-BEHAV-09: Given any theme, When all its binding keys are collected, Then no key combination is bound to two
  different actions (target test, G-BEHAV-3).
* AC-BEHAV-10: Given `macos` look + `windows11` behaviour, When applied, Then `themes` warns about the
  `Super+Space` collision and the launcher wins (G-BEHAV-2; unit tests in `themes-core/src/shortcuts.rs`).
* AC-BEHAV-12: Given the user set `themes shortcut set clipboard-toggle '<Ctrl><Alt>c'`, When any theme is applied,
  Then the key in force is still `['<Control><Alt>c']`; after `themes shortcut reset clipboard-toggle` it is the
  active theme's value.
* AC-BEHAV-13: Given a shortcut already owned by another action, When the user picks it, Then the change is
  refused and the owner is named.
* AC-BEHAV-11: Given `windows11` behaviour is reset, When `custom-keybindings` is read, Then the user's own
  custom shortcuts are present again.

## Open questions

1. *(Decided)* per-feature override in the hub; the theme provides defaults (see Settings).
2. Is `Super+I` right for opening the hub in the Windows skin, given GNOME's own Settings?
3. Windows 11 `Super+Down` / `Super+Up` semantics, and macOS tiling keys, to be fixed with
   [window-layouts](window-layouts.md).
4. Should `Alt+Tab` thumbnails (windows previews) be part of the behaviour module (extension-dependent)?
