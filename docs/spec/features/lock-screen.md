# Lock screen layouts

## Summary

The lock screen of GNOME Shell 51 is re-laid out per theme by the extension
`lockscreen-layout@desktop-themes-rs`: a macOS arrangement (large thin clock at the top centre, blurred
wallpaper, password field below the centre) or a Windows 11 arrangement (large time and date at the bottom
left, sharp wallpaper with a slight tint, password field after a key press). The stock `fedora` theme
does not enable the extension, so its lock screen is GNOME's own.

## Status

**Implemented** (stage 0; per-theme settings in `lockscreen.dconf`). Origin badge: *In Windows 11 and
macOS* (layouts only; code is original). Gaps below.

## Reference behaviour

* **Windows 11:** a full-bleed wallpaper, big time and date at the lower left, a first "curtain" screen;
  any key or click reveals the sign-in field (centre) with the user's avatar.
* **macOS:** a blurred wallpaper, a large thin clock and date at the top centre, the user's avatar and
  password field below the centre.

GNOME has a two-step lock screen like Windows (clock, then prompt on a key press), centred clock, blurred
wallpaper.

## Our behaviour

### What the extension changes

It patches `UnlockDialog` of GNOME Shell 51 (via `InjectionManager`; the stock methods are restored in
`disable()`), working in the session modes `user` and `unlock-dialog` (so it keeps running while locked).
No network, no subprocess.

| Key (`org.gnome.shell.extensions.lockscreen-layout`) | Type | Default | Meaning |
|---|---|---|---|
| `clock-position` | enum | `default` | `default` (stock, centred), `center-top`, `bottom-left` |
| `margin-x` | int 0..2000 | 64 | `bottom-left`: distance from the left edge |
| `margin-y` | int 0..2000 | 96 | `center-top`: distance from the top; `bottom-left`: from the bottom |
| `clock-font-size` | int 0..400 | 0 | time size in px (0 = stock) |
| `date-font-size` | int 0..200 | 0 | date size in px (0 = stock) |
| `clock-font-family` | string | `''` | family for time and date (empty = stock) |
| `clock-font-weight` | string | `''` | CSS weight for the time |
| `clock-glow` | string | `''` | CSS colour of a soft glow behind the time |
| `clock-color`, `date-color` | string | `''` | CSS colours |
| `hide-hint` | bool | `false` | hide "Click or press a key" |
| `prompt-offset` | int -500..500 | 0 | shifts the password prompt down (px) |
| `blur` | bool | `true` | blur the background |
| `blur-radius` | int 0..200 | 90 | blur radius |
| `brightness` | double 0..1 | 0.65 | brightness when blurred |
| `tint` | string | `''` | CSS colour laid over the background |
| `background-uri` | string | `''` | `file://` image; empty = desktop wallpaper; the word `screensaver` = `org.gnome.desktop.screensaver picture-uri` |

### The two layouts (values the themes set)

| | `macos` | `windows11` |
|---|---|---|
| Clock position | `center-top`, `margin-y` 110 | `bottom-left`, `margin-x` 80, `margin-y` 90 |
| Time / date size | 128 / 26 px | 120 / 30 px |
| Font | Inter, weight 200 | Inter, weight 300 |
| Colours | time 92% white, date 85% white | time white, date 95% white |
| Hint | shown | hidden |
| Prompt offset | +120 px | 0 |
| Background | blur on, radius 60, brightness 0.78 | blur off, tint `rgba(0,0,0,0.22)`, brightness 1.0 |
| Background image | `screensaver` key (the theme's `screensaver picture-uri`) | same |
| Blur my Shell for the lock screen | off (the extension does the blur) | off |

(The text is white on a darkened image in both; the contrast must hold on any wallpaper, see
accessibility.)

### Behaviour

* Changing any key while the screen is locked or unlocked takes effect immediately (the dialog is re-laid
  out on `changed`).
* The stock layout is restored exactly when the extension is disabled or `clock-position=default` with
  zeros/empties (the defaults).
* The password prompt is the stock one; the extension never reads, stores or alters the password or the
  authentication flow.
* Per-monitor: the clock layer is bound to the primary monitor; other monitors show the background only
  (stock behaviour).
* Animation: the stock curtain/unlock animations are untouched and follow `enable-animations`; the
  extension adds none.

### Empty and error states

* `background-uri` unreadable or missing: falls back to the desktop wallpaper.
* Unknown font family: Pango substitutes; text never disappears.
* The extension fails to enable: GNOME's lock screen stays stock; `themes doctor` reports it
  (broken-extension rules) and rolls it back.

## Settings

The keys above are declared in the settings registry (`crates/desk-settings/src/lockscreen.rs`); the schema
file in the extension directory is generated from it (`cargo xtask gen`, see [settings.md](../../settings.md)).
They are written per theme by `lockscreen.dconf` (look module) and edited on the «Lock screen» page of the
window (`themes gui --page lockscreen`). The page needs the extension installed: its schema is compiled next
to the extension (`data/install.sh`, `themes doctor --fix`).

Accepted values (the registry checks them in the window, in `themes apply` for theme files, in `themes
doctor` for dconf, and the extension repeats the check before it builds a style string): `clock-position`
one of `default`, `center-top`, `bottom-left`; `clock-font-weight` empty, `100`..`900`, `normal` or `bold`;
the colours (`clock-color`, `date-color`, `clock-glow`, `tint`) empty or `#rgb`, `#rgba`, `#rrggbb`,
`#rrggbbaa`, `rgb()`, `rgba()` with numbers (no colour names); integers and the brightness within their ranges.
A refused value is ignored and the standard look is used.

## Privacy & security

Sensitive surface: it draws over the unlock dialog. Requirements: no logging of key events; no screenshots; no
subprocess; no network; restore everything in `disable()`. Style strings built from settings (colours,
weights) are written into St `style`; the family is stripped of quotes and backslashes (see G-LOCK-2).

## Edge cases

* GNOME updates change `UnlockDialog` internals (`_clock`, `_stack`, `_backgroundGroup`, `_bgManagers` are
  private fields): the extension then fails to enable and the stock screen remains; `shell-version` is
  pinned to 51.
* Lock while a theme is being applied: layout reads settings at dialog creation and on change.
* HiDPI: sizes are in logical px; background effects use the stage scale factor.
* Multiple users / GDM login screen: not affected (it is the session lock only).
* Light wallpapers with the `windows11` layout: the tint (22% black) keeps white text legible; a brighter
  wallpaper needs a higher tint (see accessibility gap).

## Accessibility

Clock text must have >= 4.5:1 contrast against the background under the chosen blur/brightness/tint
for any wallpaper (a spec requirement not enforced today, G-LOCK-3). Text sizes are fixed px (not scaled by
`text-scaling-factor`; the stock sizes scale): large defaults compensate. The prompt remains reachable
by keyboard (first key press reveals it, as stock); the "hint" can be hidden only because the screen reacts
to any key.

## Performance

Blur is GPU work on the lock screen only; opening the lock screen must take <= 200 ms longer than stock;
no work is done while unlocked other than holding the injections.

## Gaps found

* **G-LOCK-1** Both lock-screen files set `background-uri='screensaver'`, a sentinel only this extension
  understands (it then reads `org.gnome.desktop.screensaver picture-uri`, a key GNOME 51 itself ignores,
  per the comment in `extension.js`). With the extension off, that key has no effect; the dependency is
  implicit and undocumented for theme authors (add it to the theme contract).
* **G-LOCK-2** (fixed) `clock-color`, `date-color`, `clock-glow`, `clock-font-weight` and `tint` were
  interpolated into St style strings without validation. They are now checked against the registry grammar (a
  colour, a weight from a list) by the extension, `themes apply` and `themes doctor`; only the font family is
  still sanitised by stripping quotes and backslashes.
* **G-LOCK-3** No contrast guard: a light wallpaper with the `windows11` values can make the clock illegible.
* **G-LOCK-4** Sizes are px, not scaled with `text-scaling-factor`.
* **G-LOCK-5** The extension has no unit/headless test; the headless tools cover only the clipboard.

## Acceptance criteria

* AC-LOCK-01: Given `macos` look is active, When the screen is locked, Then the clock is horizontally centred
  with its top about 110 px from the top, the background is blurred, and the hint is visible.
* AC-LOCK-02: Given `windows11` look, When locked, Then the time is at the bottom left (80 px from the left,
  90 px from the bottom), the hint is hidden, the wallpaper is not blurred and a 22% black tint is present.
* AC-LOCK-03: Given `fedora` is active, Then `lockscreen-layout` is not enabled and the lock screen is stock.
* AC-LOCK-04: Given the extension is disabled while locked, Then the stock layout is restored with no leftover
  actors (`d._lx` is null, no extra layers).
* AC-LOCK-05: Given `lockscreen-layout blur` is toggled while locked, Then the background changes without
  re-locking.
* AC-LOCK-06: Given `background-uri` points to a missing file, Then the desktop wallpaper is shown.
* AC-LOCK-07: Given `clock-color='red; margin:0'` (hostile), Then the value is refused and the standard colour
  is used (`desk_settings::validate` tests; `extension.js` repeats the check).
* AC-LOCK-08: Given a white wallpaper and either layout, When the clock/background contrast is measured,
  Then it is >= 4.5:1 (target, G-LOCK-3).
* AC-LOCK-09: Given the password is typed on the lock screen, Then it is never present in any log of the shell
  or the extension.

## Open questions

1. Show the user avatar and name as macOS and Windows do (not available from the stock dialog without more
   patches)?
2. A "clock style" preset list in the hub (macOS, Windows 11, stock) or only per theme?
3. Should the layout also apply to the GDM login screen (needs system-level changes; out of scope today)?
