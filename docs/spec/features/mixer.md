# Volume mixer and audio device switching

## Summary

One place to see and change what is playing and where: the master volume, the **output and input
devices** (switch the default with one click, remember per-device volume), and a **per-application
volume mixer** with mute. It lives inside the quick-settings panel the user already opens for volume.
The mixer is Windows-native; the `macos` skin gets the same capabilities arranged as Apple would
have done it, inside the Control Center "Sound" tile.

## Status

**Planned** (stage 2). Origin badge: *From Windows, adapted for macOS style* (device switching alone is
in both systems). Candidates to integrate or port with attribution: an existing GNOME Shell quick-settings
audio extension (licence to be checked when chosen; principle 5).

## Reference behaviour

* **Windows 11:** the volume slider in the quick-settings flyout has a chevron that lists output devices
  (click selects the default); right-click on the speaker icon opens "Volume mixer": a master slider and one
  slider per running application with icon, name, mute button, plus per-device volume; "Output" and "Input"
  device pickers; app-to-device routing in Settings > Sound.
* **macOS:** Control Center "Sound" shows an output volume slider and a list of output devices with a check
  mark (and input); there is **no** per-application volume in the system.

## Our behaviour

### Elements

| Element | Behaviour |
|---|---|
| Master volume slider | 0..100% (to 150% only when `org.gnome.desktop.sound allow-volume-above-100-percent`); mute button on the icon; step by arrow keys = `volume-step` (default 5%); `Shift` = 1% |
| Output devices | list of sinks with name, form factor icon (speakers, headphones, HDMI, Bluetooth) and a check on the default; selecting makes it default and **moves all playing streams** to it |
| Input devices | list of sources (microphones) with a check; mic mute; a level indicator only while the list is open |
| Applications | one row per playing (and recently playing, <= 30 s) stream: app icon, name, slider, mute; two streams of one app are one row (volume applies to all); the shell's own event-sound stream is hidden |
| Per-app output | a row's "..." menu "Play on..." moves that app's streams to another device |

### Skins

* **`windows11`:** the quick-settings volume slider gets a chevron; the chevron opens a **sub-panel in the same
  quick-settings flyout** with sections "Output", "Volume mixer" (the app list) and "Input", plus a link
  «Налаштування звуку». `Win+A` opens quick settings (see [behavior](behavior.md)); `Tab` reaches the chevron.
  Right-click on the panel speaker icon opens the sub-panel directly.
* **`macos`:** the Control Center "Sound" tile becomes a sound page: the volume slider on top, **Output**
  (list, check mark), **Input**; below, a collapsed disclosure **"Apps"** (shown only when at least one app is
  playing) with compact sliders. As Apple would: no mixer chrome by default, device names without technical
  suffixes ("Built-in Speakers"), AirPods-style Bluetooth devices at the top. Opened from the quick-settings
  Sound tile or the panel icon.
* **`default`:** GNOME's own volume slider and device chooser, extended by one "Applications" group under the
  slider; same data.

### Keyboard, focus

Sliders: `Left/Right` (or `Up/Down`) adjust by `volume-step`, `Home/End` min/max, `M` mutes the focused row; list rows: `Up/Down`
move, `Enter` selects, `Esc` closes the sub-panel (focus returns to the chevron). Media keys keep working
and show GNOME's OSD. The master volume keys never need the panel.

### Animation

Slider and disclosure transitions <= 150 ms; instant with reduced motion. Level changes are never animated.

### Empty and error states

No audio server: the panel shows «Аудіосервер недоступний» and the master slider is disabled. No application
playing: «Немає програм, що відтворюють звук». A device that disappears: the list updates; if it was the default,
the system's fallback default is shown (no error toast unless a stream was lost).

## Engine and architecture

* **deskd** hosts an `Audio` engine (`io.github.OlehDzhumyk.Desk.Audio`) that talks the PulseAudio protocol
  (served by PipeWire) via libpulse and exposes `ListDevices`, `ListStreams`, `SetVolume(kind,id,v)`,
  `SetMute`, `SetDefault(kind,id)`, `MoveStream(id,sink)`, signal `Changed`. It subscribes to server events only
  (no polling, no peak monitoring in v1), and keeps no audio data.
* The bridge draws the sub-panel and registers it in quick settings; it asks deskd and never opens an audio
  connection itself.
* Alternative to decide (open question 1): the bridge uses the shell's own `Gvc.MixerControl` (already running in
  the shell process) and deskd holds only preferences.

## Settings (proposed, schema `io.github.OlehDzhumyk.Desk.Audio`)

| Key | Type | Default | Range |
|---|---|---|---|
| `enabled` | bool | true | |
| `show-apps` | bool | true | |
| `volume-step` | uint | 5 | 1..20 (%) |
| `remember-app-routing` | bool | false | route an app to the same device next time |
| `hide-apps` | as | `[]` | app ids never listed |

Over-amplification follows `org.gnome.desktop.sound allow-volume-above-100-percent`; no copy.

## Privacy & security

Reads names and icons of applications that are playing audio (local). No audio samples are read in v1. Remembered
routing stores app ids and device names in `~/.local/share/deskd/` (0600). No network.

## Edge cases

* Bluetooth device in the headset profile (low quality) vs A2DP: switching profile is out of scope; show the
  device as the server reports it.
* HDMI/DisplayPort sinks that appear/disappear with monitor power: hidden when unavailable.
* Applications that reset their own volume (flat-volume behaviour of some browsers): our slider reflects the
  server value live.
* Many streams (> 12): the list scrolls; rows of the same app are merged.
* Muted device selected as default: shows muted, selecting does not unmute.
* A stream whose app has no desktop entry: generic icon, the stream's `application.name`.
* Flatpak/sandboxed apps: identified by app id when present.
* Moving a stream to a device that cannot play the format: server decides; show nothing on success, a toast
  «Не вдалося перемкнути» on failure.
* Mute and volume keys while the sub-panel is open: both update the panel live.

## Accessibility

Sliders have accessible names ("<app>: volume") and values in percent; muted state is text, not just an icon; check
marks are also announced ("default"); all functions have keyboard paths; contrast per skin tokens.

## Performance

Panel visible <= 16 ms; list <= 50 ms; deskd idle wakeups unaffected (events only when the audio graph
changes); added resident memory <= 3 MB; no work while no UI is open beyond the event subscription.

## Acceptance criteria

* AC-MIX-01: Given two output devices and `windows11`, When the chevron is activated and the second device chosen,
  Then it is the server default and all playing streams moved to it.
* AC-MIX-02: Given an application stream playing, When its slider is set to 30%, Then the server shows 30% for
  that stream and other streams are unchanged.
* AC-MIX-03: Given a muted app row, When the row is activated by keyboard (`M`), Then the stream is unmuted.
* AC-MIX-04: Given the `macos` skin and no playing app, Then the "Apps" disclosure is not shown.
* AC-MIX-05: Given a device is unplugged while it is the default, Then the list updates within 500 ms and the new
  default is checked.
* AC-MIX-06: Given `allow-volume-above-100-percent=false`, Then the slider cannot exceed 100%.
* AC-MIX-07: Given the audio server is stopped, Then the panel shows the unavailable text and does not freeze the
  shell.
* AC-MIX-08: Given `enable-animations=false`, Then no slider or disclosure transition runs.
* AC-MIX-09: Given idle (no panel open) for 60 s, Then deskd made <= 2 wakeups attributable to the Audio engine.
* AC-MIX-10: Given `remember-app-routing`, When an app is moved to device B and restarted, Then its new stream is
  routed to B.

## Open questions

1. Engine in deskd (consistent with the architecture) or the bridge on `Gvc` (zero extra process memory)?
2. Per-app routing memory: use PipeWire's own policy (WirePlumber) instead of storing our own?
3. Should the macOS skin show the per-app disclosure at all, given macOS has none (stricter "as Apple would")?
4. Which existing extension do we port from, if any?
