# Sounds

## Summary

Each theme can bring a **sound theme**: a freedesktop sound theme (`~/.local/share/sounds/<name>`) of
short event sounds (notifications, errors, login, power, ...), and the `sounds` module switches the
desktop to it. The bundled `macos-like` and `windows11-like` themes are synthesised by this project
(original works, CC0): no Apple or Microsoft audio is used.

## Status

**Partial** (stage 0). Switching the sound theme works; there is no sound page in the hub, no
volume/per-event controls, and some GNOME 51 events cannot be themed at all (listed below).
Origin badge: *Original* (generated, CC0).

## Reference behaviour

* **Windows 11:** a "sound scheme" (Settings > Sound > More sound settings) assigns a file to each
  system event (notification, critical stop, device connect, ...); sounds are played for most system
  events, with a separate startup sound.
* **macOS:** one alert sound plus UI feedback sounds (screenshot, volume change, empty trash), with
  a setting to turn UI sounds off.

## Our behaviour

### What the module sets

`themes/<id>/sounds.dconf` writes `org/gnome/desktop/sound`:

| Key | `macos` | `windows11` | `fedora` |
|---|---|---|---|
| `theme-name` | `macos-like` | `windows11-like` | baseline (stock `freedesktop`) |
| `event-sounds` | `true` | `true` | baseline |
| `input-feedback-sounds` | `false` | `false` | baseline |

The sound theme directories are created by `scripts/generate-assets.sh` (via
`tools/synth-sounds.py`) in `~/.local/share/sounds/`; the generated files are not committed. Applying
the module needs no restart: GNOME reads `theme-name` live for events requested afterwards.

### The generated themes

`tools/synth-sounds.py <dir>` writes `macos-like` and `windows11-like`: 27 freedesktop event names, 48 kHz
stereo Vorbis (`.oga`), loudness about -16 LUFS, true peak <= -1 dBFS, produced only with numpy/scipy
(no samples). Events covered: `alarm-clock-elapsed`, `audio-volume-change`, `battery-caution`,
`battery-low`, `bell`, `bell-terminal`, `bell-window-system`, `camera-shutter`, `complete`,
`desktop-login`, `desktop-logout`, `device-added`, `device-removed`, `dialog-error`,
`dialog-information`, `dialog-warning`, `message`, `message-new-instant`,
`network-connectivity-established`, `network-connectivity-lost`, `power-plug`, `power-unplug`,
`screen-capture`, `service-login`, `service-logout`, `trash-empty`, `window-attention`.

Character: `macos-like` is soft, glassy, bell-like (short tones, warm low tones for errors);
`windows11-like` is clean, short and rounded, with a gentle rising arpeggio for login. They must be
distinguishable at a glance, equally loud, never harsh, and each under 3 s.

### Which GNOME 51 events cannot be themed

Per the project's notes (re-verify on each GNOME release): **volume-change feedback, the screenshot
sound, drive (device added/removed) sounds and the break-reminder sounds are fixed system files** that
GNOME plays itself and ignores the sound theme. Therefore the generated `audio-volume-change`,
`screen-capture`, `camera-shutter`, `device-added` and `device-removed` are provided for apps and future
GNOME versions that do use the theme, but today they have no audible effect in the shell. The events that
do follow the theme are those requested through libcanberra by GTK apps and by the shell (window bell,
notification sound hints, dialogs, power/battery, login/logout where implemented). Which exact event
names GNOME 51 requests is **not** established by this spec (open question 1).

### Interaction with the user's own choice

Choosing an alert sound in GNOME Settings writes `theme-name='__custom'`; re-applying the theme resets it
to the theme's sound theme. Muting (`event-sounds=false`) set by the user is overwritten by the theme's
value on apply; the hub must therefore show "event sounds" next to the theme (planned).

### Settings (planned hub page «Звуки»)

*Proposed* controls on the sounds page: sound theme (from installed themes), event sounds on/off,
input feedback on/off, a **preview** button per generated event, and "Reset to the theme's sounds". No new
GSettings keys: these are the GNOME keys above. Volume stays with the system/[mixer](mixer.md).

### Empty and error states

* Sound theme directory missing (assets not generated): `themes doctor` warns "не знайдено звукову тему
  «macos-like»" and GNOME falls back to its default theme; nothing fails.
* No audio device: nothing plays; no error.

### Animation

Not applicable. A preview button shows a transient "playing" state that respects reduced motion (no
animation, just a label change).

## Privacy & security

Local files only; no network; no microphone. Synthesis is deterministic and offline.

## Edge cases

* Sound theme named in dconf but only installed in the system dir: found (doctor searches user, XDG data
  dirs and `/usr/share`).
* A third-party theme's `sounds.dconf` pointing at a theme with an `index.theme` that inherits another:
  fine, resolved by libcanberra.
* Applying with sounds disabled globally (`event-sounds=false` in a theme): events are silent; the preview
  must still play (explicit user action).
* Sound at login: GNOME 51 does not play a login sound from the theme in user sessions the way Windows does;
  `desktop-login` is provided for completeness.

## Accessibility

Sounds supplement visual cues, never replace them (all events have visual equivalents); the preview
buttons have accessible names; loudness is normalised so alerts are not painful; GNOME's "visual alerts"
accessibility option is unaffected.

## Performance

Playback is libcanberra's; generation takes seconds at install time, not at runtime. Applying the module is
one dconf write.

## Gaps found

* **G-SND-1** The theme's `sounds.dconf` can silently restore `event-sounds=true` for a user who muted
  sounds (applied with the module); `input-feedback-sounds=false` likewise. The apply plan has no "keep
  the user's mute" rule.
* **G-SND-2** `docs/themes.md` states which events cannot be themed but this was never covered by a test;
  the generated `audio-volume-change`, `screen-capture`, `camera-shutter`, `device-added` and
  `device-removed` therefore may never play.
* **G-SND-3** There is no sound page, no preview and no per-event override in the window; users must use
  GNOME Settings and then lose the choice on the next apply (`__custom`).
* **G-SND-4** No test checks the loudness/peak targets of the generated files.

## Acceptance criteria

* AC-SND-01: Given `themes apply macos --only sounds`, When `org.gnome.desktop.sound theme-name` is read,
  Then it is `macos-like`; for `windows11` it is `windows11-like`.
* AC-SND-02: Given `scripts/generate-assets.sh` ran, When `~/.local/share/sounds/macos-like` is listed,
  Then it has an `index.theme` and an `.oga` for each of the 27 events.
* AC-SND-03: Given every generated file, When measured, Then integrated loudness is within -16 +/- 1 LUFS
  and the true peak is <= -1 dBFS, and the duration is < 3 s.
* AC-SND-04: Given `themes reset`, Then `theme-name` returns to its baseline value.
* AC-SND-05: Given the sound theme is missing, When `themes doctor` runs, Then there is a `missing-asset`
  warning naming it.
* AC-SND-06: Given the user selected a custom alert in GNOME Settings (`theme-name='__custom'`), When the
  sounds module is applied, Then `theme-name` is the theme's.
* AC-SND-07: Given the user muted event sounds, When any theme's sounds module is applied, Then the user is
  not unmuted without being told (target, G-SND-1).
* AC-SND-08: Given the sounds page (planned), When the preview of `dialog-error` is pressed, Then the file
  plays once and the button is operable by keyboard.

## Open questions

1. Which event names does GNOME 51 actually request from the theme (shell, gsd-media-keys, gsd-power, GTK)?
   A short audit script should settle the table above.
2. Should the theme be allowed to leave `event-sounds` and `input-feedback-sounds` to the user (a theme says
   only `theme-name`)?
3. Should we offer an optional startup/login sound through a systemd user unit (opt-in)?
