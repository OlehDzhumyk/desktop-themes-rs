# On-screen displays (volume, brightness, mute, keyboard backlight)

## Summary

When the user changes volume, brightness, mute, microphone mute or keyboard backlight with a hardware
key, a short **on-screen display** (OSD) confirms the new level. It looks like the Windows 11 or macOS
one, depending on the skin. It never takes keyboard focus and it hides on its own.

## Status

**Implemented** (stage 3, visual foundation). Origin badge: *In Windows 11 and macOS*.
Code: `bridge/src/osd/` (`feature.ts` patch and restore, `popup.ts` drawing), settings
`crates/desk-settings/src/osd.rs`, end-to-end tests `tests/e2e/test_osd.py`.

*Known gaps of the first release:*

* The bridge draws the values that GNOME Shell passes to `Main.osdWindowManager` (they are the
  applied values the shell read back). `deskd` is not involved yet, so the rule «if `deskd` is
  unreachable, keep the stock OSD» does not apply: the skinned OSD works without `deskd`.
* `showAll` (the shell's own volume and the `ShowOSD` D-Bus call of gsd-media-keys) draws one OSD on
  the pointer monitor, not one per monitor as the stock OSD does.
* The shell's text label (it can name an audio device) is shown only when there is no level, to
  keep device names off the screen.
* The bar width scales with the display scale but not with `text-scaling-factor`; the number and
  icon do.
* Lock screen: the bridge declares no `session-modes`, so GNOME Shell disables it while the screen
  is locked and the stock OSD is used there; no lock-screen test exists yet.
* KDE and wlroots (`wlr-layer-shell`) surfaces are not implemented.

*Where it draws:* on GNOME the bridge (`desk-bridge`) draws it. It replaces and styles
`Main.osdWindowManager` for the `windows11` and `macos` skins. `disable()` restores the stock object
exactly (same methods, same signal connections, nothing left behind). On KDE and wlroots compositors a
later native `wlr-layer-shell` surface may draw the same OSD
([ADR 0011](../../adr/0011-wayland-agnostic-rust-core.md)). `deskd` owns the logic (what to show,
level values, settings). The bridge only paints.

## Reference behaviour

* **Windows 11:** a compact floating bar at the bottom centre, above the taskbar. It has an icon, a
  horizontal progress slider, and a numeric percentage. The surface is Fluent acrylic with a subtle
  border. It hides after about two seconds.
* **macOS:** a compact translucent pill or square, centred or at the top right (it depends on the macOS
  release; see open question 2). It has a crisp icon and a level bar, segmented or smooth. It hides after
  about two seconds.

No vendor assets are used. Icons are our own symbolic set.

## Our behaviour

### Skins

| Aspect | `windows11` | `macos` | `default` |
|---|---|---|---|
| Surface | acrylic bar with Fluent border and corner radius | translucent pill, vibrancy-like blur | GNOME stock OSD, untouched |
| Default position | bottom centre, above the panel | centre of the active monitor | stock position |
| Level | horizontal slider and number | smooth level bar; number optional | stock |

With `default` the bridge does not touch `Main.osdWindowManager` at all.

### Triggers

| Trigger | Shows | Notes |
|---|---|---|
| Output volume | speaker icon, level 0..100 % | follows the default sink |
| Output mute | muted speaker icon, bar empty | un-mute shows the level again |
| Over-amplification (> 100 %) | level above 100 % | the part above 100 % uses a distinct bar colour; the number stays visible |
| Microphone mute | microphone icon, muted or live | no bar |
| Display brightness | sun icon, level 0..100 % | built-in panel and supported external displays |
| Keyboard backlight | keyboard icon, level step | shown as a percentage of the steps |

The OSD shows the value that was **applied**, read back from the system, not the value that was
requested.

### Timing

The OSD hides 1.5 s after the last change (`timeout-ms`). A repeated key press resets this timer and
updates the same OSD. It does not open a second one.

### Focus and input

The OSD is click-through. It never takes keyboard focus, never takes pointer input, and does not appear
in the window list or the Alt+Tab list.

### Animation

The OSD fades in and moves a few pixels to its place in 120 ms, and fades out in 150 ms. When
`enable-animations` is off, or the toolkit reports reduced motion, it shows and hides instantly. The
level bar then also jumps without easing. No information is carried by motion alone.

### Empty and error states

If `deskd` is unreachable, the bridge does not draw its own OSD and the stock GNOME OSD stays in use. If
a level cannot be read, the OSD shows the icon only, without a number.

## Settings (`io.github.OlehDzhumyk.Desk.Osd`)

| Key | Type | Default | Range / values |
|---|---|---|---|
| `enabled` | bool | `true` | |
| `show-percentage` | bool | `true` | |
| `timeout-ms` | uint | `1500` | 500..4000 |
| `position` | choice | `skin-default` | `skin-default`, `bottom-center`, `center`, `top-right` |

Settings go through the settings registry (`crates/desk-settings`), then `just gen`.

## Privacy & security

The OSD stores nothing and uses no network. It shows only levels and states of the user's own
hardware. It shows no media titles, device names, or app names. It does not appear over the lock screen
beyond what the GNOME lock screen policy already allows.

## Edge cases

* **Multi-monitor:** the OSD appears on the monitor with the pointer. If there is no pointer, it appears on the primary monitor.
* **Full-screen video or game:** the OSD still appears, small and click-through. It does not pause, resize, or unredirect the full-screen window.
* **Lock screen:** the OSD follows the GNOME lock screen policy. Hardware keys still work; nothing extra is drawn.
* **Rapid key repeat:** updates coalesce to at most one repaint per frame. There is one OSD, never a stack.
* **High contrast:** with `.desk-hc` the OSD has a solid outline and always shows the number. Blur and transparency are removed.
* **Extension disable or shell restart:** the stock OSD is restored; no signal handler, timer, or actor stays behind.
* **Skin switch while the OSD is visible:** the OSD hides at once, and the next one uses the new skin.
* **Volume key when output is already at maximum or minimum:** the OSD still appears, so the user knows the key worked.
* **Another extension replaces `osdWindowManager`:** the bridge detects it, keeps the stock one, and writes one warning to the log.

## Accessibility

* Each change is announced through a polite live region, for example «Гучність 40 %» / "Volume 40 percent". Muted and live states are announced as words.
* The percentage is never conveyed by colour alone. The number and the bar length always agree. Over-amplification is also marked with text ("above 100 %") and a bar marker.
* High contrast: outline and number (see edge cases). Contrast of text and bar is at least 4.5:1 and 3:1 (WCAG 2.2 AA) in every skin and scheme.
* Text scaling (`text-scaling-factor`) enlarges the number and the OSD.
* Reduced motion: see Animation.

## Performance

* The OSD is visible within 16 ms (one 60 Hz frame) of the key event, p95.
* After hide, zero timers and zero idle wakeups remain. The hide timer is one one-shot timer.
* The bridge adds no memory beyond the budget in [principles](../principles.md#performance-budgets).

## Acceptance criteria

* AC-OSD-01: Given `windows11`, When the output volume changes to 40 %, Then one OSD appears at the bottom centre with a speaker icon, a bar at 40 %, and the number "40".
* AC-OSD-02: Given `macos`, When the display brightness changes, Then a centred pill appears with a sun icon and a level bar, and it hides after `timeout-ms`.
* AC-OSD-03: Given an OSD is visible, When the volume key repeats ten times in one second, Then there is still one OSD, the hide timer restarts at each press, and the OSD hides 1.5 s after the last press.
* AC-OSD-04: Given the output volume is above 100 %, When the OSD shows, Then the part above 100 % has a distinct colour and the number is shown.
* AC-OSD-05: Given the skin is `default` or `enabled` is false, When any trigger fires, Then the stock GNOME OSD appears and `Main.osdWindowManager` is not modified.
* AC-OSD-06: Given an OSD is visible, When a pointer click or a key press happens, Then the input goes to the window below and the OSD does not take focus.
* AC-OSD-07: Given the bridge is disabled, When `disable()` returns, Then the stock OSD manager is restored, no timer is pending, and no signal connection of the bridge OSD remains.

## Open questions

1. Should Caps Lock and Num Lock indicators be an opt-in OSD (as macOS does not and Windows can)? Default: off.
2. macOS style: the centred square (Sequoia) or the compact top-right pill (Tahoe)? Decided: centred square as the default, with `position = top-right` (or `bottom-center`) for the pill.
3. Should the OSD also confirm other keys (media play/pause, airplane mode, display switch)? Proposal: not in the first release.
