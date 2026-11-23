# Colour picker

## Summary

An eyedropper: a magnifying loupe follows the pointer; a click copies the colour under it in the chosen format
and remembers it in a short history. Useful for designers, developers and anyone matching colours.

## Status

**Implemented** (stage 3). Origin badge: *In Windows 11 (PowerToys) and macOS (Digital Color Meter)*. deskd hosts
the engine (`io.github.OlehDzhumyk.Desk.ColorPicker`: formats, history, contrast); the bridge
(`bridge/src/colorPicker/`) binds the shortcut, reads pixels with GNOME Shell's own `Shell.Screenshot.pick_color()`
(the call behind `org.gnome.Shell.Screenshot` `PickColor`) and draws the loupe, the history panel and the toast. No
external code.

Gaps:

* **Gap (loupe):** the loupe magnifies a frame of the stage taken when the picker opens
  (`Shell.Screenshot.screenshot_stage_to_content()`, kept on the GPU, never written to disk); the swatch and the value
  are read live under the pointer. Video or animation under the pointer can differ between the loupe and the value.
  The loupe is a 120 px square with rounded corners, not a circle: St cannot clip to a circle
  (`bridge/src/colorPicker/overlay.ts`).
* **Gap (history panel):** one modal panel for all skins, styled per skin: `windows11` opens it as a flyout above the
  bottom right corner (not inside the Win+V flyout), `macos` and `default` as a popover at the pointer. It opens with
  `H` or a right click while picking; the `macos` «same shortcut with `Shift`» is not bound, and there is no
  quick-settings entry (`bridge/src/colorPicker/panel.ts`).
* **Gap (tooltip):** the note on night light and colour profiles is in this spec and the settings page, not in the
  preview (`bridge/src/colorPicker/overlay.ts`).
* **Gap (tests):** deskd tests cover the formatting of AC-COLOR-01 and 02, and AC-COLOR-03 and 05. The shell side
  (the pixel read of 01 and 02, and 04, 06, 07, 08) has no GNOME Shell end-to-end test yet (`just e2e`).

## Reference behaviour

* **Windows 11 (PowerToys Color Picker, `Win+Shift+C`):** a loupe near the cursor shows the colour; click picks and
  copies in the default format (HEX, RGB, HSL, ...); scroll adjusts the zoom; a history editor lists picks and offers
  formats, a colour mixer and variations.
* **macOS:** the Digital Color Meter app shows values under the pointer in a chosen colour space and copies on
  `Cmd+Shift+C`; the system colour panel has a magnifier eyedropper.

## Our behaviour

### Flow

1. Shortcut (`windows11` `Shift+Super+C`; `macos` `Ctrl+Shift+Super+C`; `default` `Shift+Super+C`; overridable).
2. The pointer becomes a crosshair; a **loupe** (a 120 px circle, 10x magnification of an 12x12 px area, with the
   centre pixel outlined) follows it with the value in the chosen format below it. The loupe flips side near screen
   edges. Scroll wheel or `+`/`-` changes the zoom 4x..16x.
3. `Enter` or click picks: the value is copied to the clipboard (a toast: «Скопійовано #A1B2C3»), the picker closes,
   and the colour is added to the history (last 20, newest first). `Esc` cancels. Arrow keys move the pointer by 1 px
   (`Shift` = 10 px) so a pick is possible without a mouse.
4. **History panel** (the skin's panel: `windows11` a flyout, `macos` a popover from the same shortcut with
   `Shift`; `default` a popover): swatches with value, click copies, `Delete` removes, a format drop-down, «Очистити».

### Formats

HEX (`#RRGGBB`, default), `rgb(r, g, b)`, `hsl(h, s%, l%)`, `hsv`, CSS name when exact, GTK/libadwaita `@define-color`
line, and decimal `r,g,b`. The format is a setting and can be cycled with `F` while picking.

### States

Picking a pixel on a transparent or off-screen region: refused (no copy). Compositor error: «Не вдалося визначити
колір». Colours are sRGB values of the framebuffer, **before** night-light or colour-profile adjustments only if the
compositor reports them so; this is stated in the preview tooltip.

### Animation

The loupe follows the pointer without easing. Fade of the overlay <= 80 ms, none with reduced motion.

## Settings (proposed, `io.github.OlehDzhumyk.Desk.ColorPicker`)

`enabled` (true), `format` (choice `hex,rgb,hsl,hsv,name,gtk,decimal`, `hex`), `zoom` (uint 4..16, 10), `history-size` (uint
0..50, 20; 0 = remember nothing), `copy-on-pick` (true), `color-picker-shortcut` (per theme; your override is
`io.github.OlehDzhumyk.Desk.Shortcuts color-picker-toggle`). The key is not called `toggle-shortcut`: shell keybinding
names are global, and the clipboard already binds that name.

## Privacy & security

Picked colours are stored locally in `~/.local/share/deskd/colors.json` (0600) only when `history-size > 0`; clearable.
The loupe image is never written to disk. Reading pixels of other windows is what the compositor allows to its own
shell; sandboxed apps cannot use this.

## Edge cases

* HiDPI/fractional scaling: coordinates are logical, pixels physical; the loupe samples physical pixels.
* Multiple monitors: works across them; the loupe follows the monitor under the pointer.
* Video overlays and HDR: values are what the compositor exposes (may be tone-mapped); documented.
* A colour copied while private mode is on the clipboard is still recorded in clipboard history unless private mode
  is set; picker history itself is separate.
* Pointer grabbed by a game or a menu: the shortcut still opens; menus close first.

## Accessibility

The value is shown as text and announced; each swatch has an accessible name with the value and the nearest colour
name; contrast ratio between the last two picks is shown in the history panel (WCAG ratio, helps accessibility
work); fully keyboard operable.

## Performance

Overlay visible <= 16 ms; loupe updates at the pointer rate with < 4 ms of work per frame (one small area read);
no polling after close; deskd resident memory unchanged (<= 20 MB).

## Acceptance criteria

* AC-COLOR-01: Given a window with the pixel `#336699` under the pointer, When the picker is invoked and `Enter` is
  pressed, Then the clipboard holds `#336699`.
* AC-COLOR-02: Given `format=rgb`, Then the clipboard holds `rgb(51, 102, 153)`.
* AC-COLOR-03: Given 25 picks, Then the history keeps the 20 newest.
* AC-COLOR-04: Given `Esc`, Then the clipboard is unchanged.
* AC-COLOR-05: Given `history-size=0`, Then no `colors.json` is written.
* AC-COLOR-06: Given the keyboard only, When arrows move the pointer and `Enter` picks, Then the value of the pixel at
  the new position is copied.
* AC-COLOR-07: Given the lock screen, Then the shortcut does nothing.
* AC-COLOR-08: Given reduced motion, Then no fade runs.

## Open questions

1. Offer a "contrast checker" mode (pick foreground and background) as in some design tools?
2. Colour-managed output (display P3) later?
3. Should the macOS skin copy on click or on `Cmd+Shift+C`, as Digital Color Meter does?
