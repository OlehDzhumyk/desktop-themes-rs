# Volume mixer and audio device switching

## Summary

One place to see and change what is playing and where: the master volume, the **output and input
devices** (switch the default with one click, remember per-device volume), and a **per-application
volume mixer** with mute. It lives inside the quick-settings panel the user already opens for volume.
The mixer is Windows-native; the `macos` skin gets the same capabilities arranged as Apple would
have done it, inside the Control Center "Sound" tile.

## Status

**Implemented** (stage 2). Origin badge: *From Windows, adapted for macOS* (device
switching alone is in both systems). The engine's node/route/metadata model and the PipeWire parameter
encoding are ported from [wiremix](https://github.com/tsowell/wiremix) (MIT OR Apache-2.0); pwvucontrol
(GPL-3.0-or-later) was read for its node model but not copied (its WirePlumber binding is a git fork);
COSMIC's audio applet (GPL-3.0-only) was not used. See CREDITS.md.

### Decisions taken while building it

* The D-Bus interface is `io.github.OlehDzhumyk.Desk.Mixer` (object `/io/github/OlehDzhumyk/Desk/Mixer`,
  `data/dbus/`), not `...Audio` as first proposed, so it matches the feature and the settings schema.
  It is versioned by its `Version` property (1); additions keep it.
* Volumes on the bus are on the cubic slider scale (1.0 = 100 %, as wpctl and GNOME show them); the server
  stores the cube. Devices with an ALSA route are changed through the route (hardware mixer, saved per
  port, like pavucontrol); other nodes through their `Props`.
* Choosing an output makes it the configured default (`default.configured.audio.sink`); streams that were
  moved by hand are released to follow it, so "all playing streams move".
* "Recently playing (<= 30 s)" means a stream that still exists but stopped running; streams that are gone
  are gone. One one-shot deadline is armed only while such a stream exists.
* Per-application **volume** memory (`remember-app-volume`, off by default) is ours, in
  `~/.local/share/deskd/mixer/apps.json`; turning it off deletes it. Per-application **routing** memory
  (AC-MIX-10, `remember-app-routing`) is left to WirePlumber's own stream restore, which already keeps the
  target of a moved stream (open question 2: yes).
* The bridge extends the menu of GNOME's own output slider in quick settings instead of drawing a new
  panel, in every skin: the rows are shell popup-menu items, so theme, light/dark, accent, keyboard
  navigation and high contrast come from the shell. `windows11` hides GNOME's device list and shows Output,
  Volume mixer and Input; `macos` shows Output and Input and a collapsed "Apps" disclosure (open question 3:
  shown, collapsed, only while something plays); `default` keeps GNOME's chooser and adds "Applications".
* Shortcut: `Ctrl+Super+V` in the Windows 11 theme (as Windows opens its sound output panel); none in macOS
  and Fedora, which have no such key. Right-click on the system area opens the mixer directly
  (`entry-point`). The key is `mixer-shortcut`, not `toggle-shortcut`: shell keybinding names are global.
* Not done in v1: the input level meter, per-device volume rows in the `windows11` list (the master slider
  is GNOME's), a failure toast for a move the server refused (the row shows the server's state).

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

* **deskd** hosts the mixer engine (`io.github.OlehDzhumyk.Desk.Mixer`, `crates/deskd/src/mixer/`) that talks
  to **PipeWire natively through `pipewire-rs`** on its own thread (no libpulse) and exposes `ListDevices`,
  `ListStreams`, `ListApps`, `SetVolume`, `SetMute`, `SetAppVolume`, `SetAppMute`, `SetDefault(kind,id)`,
  `MoveStream`, `MoveApp`, properties `Version`, `Available`, `DefaultOutput`, `DefaultInput` and a
  coalesced signal `Changed(as)`. It subscribes to server events only
  (no polling, no peak monitoring in v1), and keeps no audio data.
* The bridge draws the sub-panel and registers it in quick settings; it asks deskd and never opens an audio
  connection itself.
* The bridge only renders; it never opens an audio connection and does not use the shell's `Gvc` for the mixer.

## Settings (schema `io.github.OlehDzhumyk.Desk.Mixer`, see docs/settings.md)

| Key | Type | Default | Range |
|---|---|---|---|
| `enabled` | bool | true | |
| `entry-point` | choice | `menu-and-icon` | `menu`, `menu-and-icon` (also right-click on the system area) |
| `show-silent-apps` | bool | false | list applications whose stream is paused |
| `remember-app-volume` | bool | false | restore an application's volume when it plays again |
| `volume-step` | uint | 5 | 1..20 (%) |
| `hide-apps` | as | `[]` | app keys never listed |
| `mixer-shortcut` | as | `[]` | set by themes; your override is `io.github.OlehDzhumyk.Desk.Shortcuts mixer-open` |

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

1. *(Decided)* the engine lives in deskd (PipeWire via `pipewire-rs`); the bridge renders only.
2. *(Decided)* per-app routing memory is WirePlumber's; we keep only the optional volume memory.
3. *(Decided)* the macOS skin shows the per-app disclosure, collapsed, only while something plays; the
   owner may still prefer to hide it (stricter "as Apple would").
4. *(Decided)* no extension was ported; the engine follows wiremix (see Status).
