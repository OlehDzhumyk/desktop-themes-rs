# Automatic light/dark by sunset

## Summary

The desktop switches between light and dark appearance by itself: at sunset and sunrise computed **locally** for the
user's place, or at fixed times. The switch is smooth for the user (GTK apps, the shell, wallpaper variants) and never fights a manual
choice.

## Status

**Implemented** (stage 2). Origin badge: *From macOS, adapted for Windows 11 style* (macOS has "Auto" appearance by sunrise/sunset;
Windows 11 has a manual light/dark switch only, with third-party schedulers).

*Gaps:* optional GNOME location service (GeoClue) lookup and skin-specific quick-settings
indicators are deferred to a follow-up PR; offline city/coordinate lookup, timezone fallback,
NOAA solar scheduling, fixed-time scheduling, manual override until next transition, and D-Bus
control (`io.github.OlehDzhumyk.Desk.Appearance`) are implemented in `deskd`.

## Reference behaviour

* **macOS:** Appearance: Light, Dark, **Auto** (dark from sunset to sunrise for the current location; also follows Night Shift
  timing); wallpapers with dynamic variants follow it.
* **Windows 11:** a manual "Choose your mode" (Light, Dark, Custom for system and apps); no schedule built in.
* **GNOME 51:** `org.gnome.desktop.interface color-scheme` (`default`, `prefer-dark`, `prefer-light`); Night Light has its own sunset
  schedule for colour temperature (separate feature; we may reuse its location).

## Our behaviour

### Modes (hub: «Вигляд»)

`light`, `dark`, **`auto-sun`** (sunset/sunrise), **`auto-time`** (fixed times, default 20:00 to 07:00), and `manual` is the
same as light/dark. The mode is a user setting: themes **do not** set it, only the allowed variants (a theme with both light
and dark parts follows the resulting scheme; `fedora` sets `prefer-dark` as its default through `settings.dconf`, which a
mode other than `manual` overrides).

### Scheduling

* deskd computes sunrise and sunset with the standard NOAA solar algorithm from latitude/longitude, for the current date;
  twilight offset setting (default 0 min, civil twilight -6 degrees optional).
* Location: (1) manual coordinates or city picked from a built-in list of ~500 cities (data only, no network); (2) the
  system location service (`org.gnome.system.location` / GeoClue) **only if the user enabled location services in GNOME**; we
  never request network lookup ourselves; (3) fallback: time zone's central latitude/longitude, with a note «Орієнтовно».
* One-shot timer for the next transition; re-armed after suspend/resume, time-zone change, clock jump, location change,
  midnight. If the transition time has passed at login or resume, the correct scheme is applied immediately.
* Applying = writing `color-scheme`; the shell, libadwaita and GTK apps follow live. Themes with explicit variants
  (`PREVIEW_DARK`, `picture-uri-dark`, GTK `gtk-dark.css`) follow automatically; the shell theme of `macos` is always dark.
  Tokens-based themes get their light or dark palette (design-tokens (`docs/design-tokens.md`, lands with its stage 1.5 pull request)); `themes apply` is **not**
  re-run, only the scheme key changes (fast, <= 100 ms).

### Manual override

Toggling light/dark by hand (quick settings, GNOME Settings, the hub) while in an auto mode applies at once and **keeps
the override until the next scheduled transition**, then auto resumes (the macOS behaviour); a setting may switch auto off instead.

### Skin appearance

* `macos`: the hub row "Appearance" with Light / Dark / Auto as three thumbnails; a thin sun/moon indicator in quick settings.
* `windows11`: a "Mode" drop-down with «Світлий», «Темний», «Автоматично (за заходом сонця)», «Автоматично (за часом)» as a Windows
  settings row.
* `default`: a plain row and a quick-settings dark-style toggle that shows "auto" in its subtitle.

### Animation

Colour change is the toolkit's instant swap (no cross-fade of ours unless the shell provides one); with reduced motion nothing extra.

### Empty and error states

Unknown location with location services off: uses the fallback and shows a hint in the hub. Polar day or night: the sun
never sets/rises, so the scheme stays; the hub says «Сонце не заходить/не сходить сьогодні».

## Settings (proposed, `io.github.OlehDzhumyk.Desk.Appearance`)

| Key | Type | Default |
|---|---|---|
| `mode` | choice `light,dark,auto-sun,auto-time` | `dark` initially = what the user had |
| `dark-from`, `light-from` | string `HH:MM` | `20:00`, `07:00` |
| `location` | (dd) lat, lon | `(0,0)` meaning unset |
| `location-source` | choice `manual,system,timezone` | `timezone` |
| `twilight-offset` | int minutes -90..90 | 0 |
| `override-until-next` | bool | true |

## Privacy & security

Location is used only to compute sunrise and sunset; stored in GSettings, never transmitted. The system location service is
used only if the user already enabled it. No network by us.

## Edge cases

* DST change, travel across time zones: recompute at the zone change signal.
* System suspended through a transition: applied on resume.
* User changes `color-scheme` by `gsettings` directly: treated as a manual override.
* `prefer-light` vs `default`: light is written as `default` (GNOME's convention) unless the user chose `prefer-light`.
* Apps that ignore the scheme: out of our control.
* Night Light (colour temperature) is independent; optional link "match Night Light schedule" is proposed, not default.

## Accessibility

Text labels (not only icons) show the next change time («Темний режим о 20:12»); contrast of both palettes is verified by the tokens
rules; no flashing: the change happens once.

## Performance

No polling; one timer; computing sunrise < 1 ms; no idle wakeups beyond the armed timer; deskd <= 20 MB.

## Acceptance criteria

* AC-DARK-01: Given `auto-sun` at latitude 50.45, longitude 30.52 on 2026-06-21, Then the computed sunset is within 2 minutes
  of 21:08 local time and `color-scheme` is `prefer-dark` after it.
* AC-DARK-02: Given the clock is advanced past sunset (test clock), Then `color-scheme` changes within 1 s without any other key change.
* AC-DARK-03: Given the user toggles light during the dark phase, Then it stays light until the next sunrise, then auto resumes.
* AC-DARK-04: Given suspend across sunset, Then the scheme is correct within 2 s of resume.
* AC-DARK-05: Given `mode=light`, Then no timer is armed (idle wakeups ~0).
* AC-DARK-06: Given the location is unset and location services off, Then the fallback is used and the hub shows the note.
* AC-DARK-07: Given a theme switch while in `auto-sun`, Then the scheme still follows the schedule.
* AC-DARK-08: Given no network, Then everything works.

## Open questions

1. Should `fedora`'s forced `prefer-dark` be dropped from `settings.dconf` now, since modes own the key?
2. Smooth transition (gradual) or instant? GNOME offers none; instant for now.
3. Reuse GNOME Night Light's computed times when available, to avoid two sunsets?
