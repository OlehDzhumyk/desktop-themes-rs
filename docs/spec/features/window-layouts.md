# Snap layouts and always-on-top

## Summary

Arrange windows in ready-made layouts (halves, thirds, quarters, a big window beside two small ones) with a
shortcut, by dragging to a screen edge, or by choosing a layout from a picker; a window can be pinned above others
("always on top"). A layout can be filled quickly by suggesting the remaining windows (snap assist).

## Status

**Planned** (stage 4) as our own implementation. Today only the stock edge tiling of GNOME is on (`windows11`
behaviour sets `edge-tiling=true`) and the `windows11` theme enables the third-party extension **Tiling Shell**
(`tilingshell@ferrarodomenico.com`; GPL-3.0-or-later) for snap layouts: status of that
path **Partial**. Origin badge: *In Windows 11 and macOS* (tiling); always-on-top *from Windows (PowerToys)*.

## Reference behaviour

* **Windows 11:** snap layouts: hover the maximise button or press `Win+Z` for a layout picker (2/3 columns, grids,
  focus layouts), click a zone to place the window; snap assist then shows thumbnails of other windows to fill the
  remaining zones; `Win+Arrows` snap halves/quarters; dragging to an edge or corner snaps; layouts are remembered
  per monitor arrangement and groups restore together. PowerToys Always on Top: `Win+Ctrl+T` pins a window (a coloured
  border shows it).
* **macOS (15+):** window tiling: hover the green button for Move & Resize / Fill & Arrange menus, `Fn+Ctrl+Arrows`
  tile halves, `Option`-drag shows tiling zones, edge-drag tiles, tiled windows resize together; no always-on-top.

## Our behaviour

### Layout engine

* **deskd** keeps layout definitions (zones as fractions of the monitor work area), the user's custom layouts and
  per-monitor selection (`layouts.json`, 0600); the **bridge** applies them: it moves and resizes `Meta.Window`s and draws the
  picker and zone previews. Windows never leave the work area; gaps (`gap`, default 8 px outer/6 px inner in
  `windows11`, 0 in `default`) are applied by the bridge.
* Built-in layouts: Halves, Thirds, Two-thirds + third, Quarters, Focus (large centre with two side columns), Stack
  (main + two). Layouts adapt to portrait monitors (rows instead of columns).

### Interactions

| Action | `windows11` | `macos` | `default` |
|---|---|---|---|
| Open the layout picker | `Super+Z`, or hover the maximise button for 600 ms | hover the maximise button (a Move & Resize-style menu) or `Ctrl+Super+Up` | `Super+Z` |
| Snap left / right / top | `Super+Left/Right/Up` (GNOME defaults) | `Ctrl+Super+Left/Right/Up` | GNOME `Super+Arrows` |
| Quarters | `Super+Left` then `Up`/`Down` | `Ctrl+Super+Shift+Arrows` | GNOME tiling |
| Drag zones | drag to an edge/corner, or hold `Shift` while dragging to show zones | hold `Alt` while dragging | edge tiling |
| Maximise | `Super+Up` | `Ctrl+Super+Up` (Fill) | GNOME |

Picker: a popover with miniature layouts; `Left/Right/Up/Down` choose a layout, `Enter` picks a zone with the arrows
(`Tab` between layouts and zones), `Esc` closes; the chosen zone previews as a translucent rectangle on the
screen before the click. After placing a window, **snap assist** (setting, default on in `windows11`, off in `macos`)
shows thumbnails of other windows for each empty zone; `1..9` or click picks; `Esc` skips.

### Always on top

* Toggle: `Ctrl+Super+T` or the title-bar menu «Завжди зверху». The window stays above others on its workspace
  (Mutter `make_above`); a small pin badge appears in the window's title bar area for the skin (`windows11`:
  accent-coloured thin border for 1 s then a pin in the header; `macos`: pin glyph in the title only on hover).
* State is per window and per session (not remembered after the window closes). Fullscreen windows still cover it.
* A window pinned above the lock screen: never.

### Animation

Resize/move animations use the shell's; zone preview fade <= 100 ms; all instant under reduced motion.

### Empty and error states

A window that cannot be resized (fixed size): it is placed in the zone at its own size, centred, and no error is
shown. Layout with no free monitor space: ignored.

## Settings (proposed, `io.github.OlehDzhumyk.Desk.Layouts`)

`enabled` (true), `layout-picker-shortcut` and `always-on-top-shortcut` (per theme, overridable), `gap-outer`
(uint 0..64, per skin), `gap-inner`, `snap-assist` (bool), `hover-picker` (bool, true in windows11), `custom-layouts`
(a editor in the hub; stored as JSON in the data directory, not in dconf).

## Privacy & security

Window titles and app ids are used in memory for snap assist thumbnails only; nothing is stored; layouts hold
geometry only.

## Edge cases

* Monitors hot-plugged: layouts re-resolve against the new work area; windows keep their zone if it still exists.
* Mixed-DPI monitors; fractional scaling: geometry computed in logical pixels, rounded to even pixels.
* Windows with minimum sizes larger than a zone: extend into the neighbouring free zone, never overlap a tiled neighbour.
* Dialogs, tool windows and `override-redirect` surfaces are never snapped.
* Keyboard shortcut collision with GNOME's `Super+Arrows`: the registry in [behavior](behavior.md) is authoritative.
* Wayland-native client that ignores configure: fall back to the size it reports.

## Accessibility

Every action has a keyboard path; the picker announces "layout <name>, zone <n> of <m>"; zone previews have a
3:1 outline; snap assist thumbnails have window titles as names; reduced motion honoured.

## Performance

Picker visible <= 16 ms; snapping a window <= 1 frame of layout work; no timers or polling (hover uses
motion events with one 600 ms one-shot timer only while hovering); deskd stays <= 20 MB.

## Acceptance criteria

* AC-WIN-01: Given `windows11` and a focused window, When `Super+Z` is pressed and the left half zone chosen with the
  keyboard, Then the window occupies exactly the left half of the work area minus the gaps.
* AC-WIN-02: Given a window snapped left and snap assist on, Then thumbnails of other windows are offered for the right
  half; picking one places it there.
* AC-WIN-03: Given `Ctrl+Super+T`, Then the window stays above an overlapping non-pinned window after the latter is
  focused; pressing it again removes the pin.
* AC-WIN-04: Given a second monitor is unplugged, Then no window is left off-screen.
* AC-WIN-05: Given a dialog is focused, When `Super+Left` is pressed, Then the dialog is not resized.
* AC-WIN-06: Given reduced motion, Then no preview fade runs.
* AC-WIN-07: Given a window with a minimum width larger than the zone, Then it does not overlap its tiled neighbour.
* AC-WIN-08: Given the `macos` skin, Then snap assist is off by default and `Ctrl+Super+Left` snaps left.

## Open questions

1. Replace Tiling Shell with our own engine, or keep integrating it and only skin it (principle 5)? Its custom-layout editor is
   substantial.
2. Should tiled windows resize together (macOS) in all skins?
3. Groups restore (Windows) when a monitor is reconnected: in scope?
