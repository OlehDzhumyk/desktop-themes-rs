# Focus modes and do-not-disturb schedules

## Summary

Silence distractions on demand or on a schedule: a **Focus** state hides notification banners and optionally
mutes sounds, with named modes (Work, Sleep, Personal, custom), exceptions (apps allowed to notify), schedules
(days and hours), and a quick toggle with a status icon. Always reversible and never "stuck on".

## Status

**Planned** (stage 2). Origin badge: *In Windows 11 and macOS*. GNOME has a single "Do Not Disturb" switch
(`org.gnome.desktop.notifications show-banners`); everything beyond it is ours.

## Reference behaviour

* **Windows 11:** Do not disturb (notification centre toggle, automatic rules: duplicating display, full-screen
  apps, first hour after a feature update, scheduled hours), priority notifications list, **Focus sessions** (a timer with
  breaks started from the Clock app, hides taskbar badges and flashing).
* **macOS:** Focus modes (Do Not Disturb, Work, Sleep, Personal, custom) in Control Center and System Settings:
  allowed people/apps, schedules by time, location or app, a Focus filter, status shown to others, icon in the menu bar.

## Our behaviour

### Concepts

A **mode** has: name, icon, `block-banners` (default true), `mute-sounds` (default false), `allowed-apps` (app ids whose
notifications still appear), `hide-badges` (visual), optional **schedule** (weekdays + start/end, midnight wrap
allowed), and an optional **duration** for a manual start ("for 1 hour", "until tomorrow", "until I turn it off").
Built-in modes: **Do not disturb** (all banners blocked), **Work**, **Sleep** (also mutes sounds); the user can add custom ones.
At most one mode is active; a manual start overrides schedules until it ends; leaving a scheduled window ends the mode.

### Controls

| Control | `windows11` | `macos` | `default` |
|---|---|---|---|
| Quick toggle | in the notification flyout (`Super+N`): a «Не турбувати» button plus «Фокус» session timer | quick settings tile «Фокус» with a mode list (like Control Center) | a «Фокус» tile in GNOME quick settings with the mode list |
| Status icon | moon/bell-slash icon in the panel while active | crescent icon in the panel while active | symbolic moon icon |
| Shortcut | none by default (overridable) | none by default | none by default |

**Focus sessions** (Windows 11 style, `windows11` only; elsewhere hidden): start 15/25/45/60 min with an optional 5 min
break, a countdown in the flyout; ends with a quiet notification (allowed through).

### Enforcement

* deskd owns the state machine and schedules; the bridge enforces: it sets/clears `show-banners` and applies the allowed-apps
  filter to the message tray, and shows the status icon. deskd stores the user's **previous** `show-banners` value in its
  state so that ending a mode (or deskd/shell restart, or crash) restores it; on start-up deskd reconciles: if no mode is active
  but `show-banners` is false **because we set it**, restore it.
* Blocked notifications are not discarded: they remain in the notification list (the tray), so nothing is lost; a
  summary «Пропущено n сповіщень» can be shown when the mode ends (setting).
* **Critical** notifications (urgency critical, e.g. low battery) always get through.
* Allowed apps: exception implemented in the bridge by letting those sources show banners although `show-banners` is false
  (feasibility to be proven: open question 1; fallback: allowed apps only count while the mode is "all banners
  blocked = off").

### Schedules

Armed with **one-shot timers** for the next transition (no polling); re-armed after suspend/resume, time-zone and clock
changes. A schedule that is active at login starts the mode immediately. Overlapping schedules: the one that started last wins.

### Animation

The tile and icon change without animation beyond GNOME's; countdown digits do not animate; the end-of-session toast follows
`enable-animations`.

### Empty and error states

No modes configured: only the built-in ones. State file corrupt: modes reset to built-ins; never leave banners
blocked (reconciliation above). deskd unreachable: the tile shows «Недоступно»; the GNOME switch still works.

## Settings (proposed, `io.github.OlehDzhumyk.Desk.Focus`; modes in `~/.local/share/deskd/focus.json`, 0600)

`enabled` (true), `active-mode` (string, `''`), `summary-on-end` (bool, true), `session-length` (uint minutes, 25),
`session-break` (uint minutes 0..30, 5). Modes are edited in the hub.

## Privacy & security

Allowed-apps and schedules are local. Nothing is shared with other devices or contacts ("status shown to others" is not
implemented). The state file lists app ids only.

## Edge cases

* Mode ends while the screen is locked: banners restored; nothing shown until unlock.
* Full-screen video/presentation: optional rule "block banners while an app is full screen" (default off; open question 2).
* The user toggles GNOME's own switch during a mode: our state follows `show-banners` (user wins; the mode ends).
* Clock change backwards: schedules recomputed; no double start.
* Calendar-based focus (during meetings): out of scope.
* Notification sounds: `mute-sounds` toggles `org.gnome.desktop.sound event-sounds` and restores the previous value the same way.

## Accessibility

The state is communicated by text («Фокус: Робота до 17:00»), not only by icon; the tile and timer are keyboard operable and
announced; the summary toast is announced politely.

## Performance

No idle wakeups except the single armed timer; no polling; deskd <= 20 MB; tile paints within one frame.

## Acceptance criteria

* AC-FOCUS-01: Given «Не турбувати» is started, Then `show-banners` is false and a notification sent by an unlisted app produces
  no banner but appears in the notification list.
* AC-FOCUS-02: Given the mode is ended, Then `show-banners` equals its value before the start.
* AC-FOCUS-03: Given a schedule 22:00–07:00 and the clock at 22:00, Then the mode starts without user action and ends at 07:00.
* AC-FOCUS-04: Given a critical notification during a mode, Then its banner is shown.
* AC-FOCUS-05: Given deskd is killed during a mode, When it restarts, Then the state is reconciled: the mode continues or
  `show-banners` is restored, never left silently blocked without an active mode.
* AC-FOCUS-06: Given a system suspend across the schedule end, When resumed, Then the mode has ended.
* AC-FOCUS-07: Given `windows11`, When a 25-minute session is started, Then the flyout shows a countdown and a quiet
  notification at the end.
* AC-FOCUS-08: Given the user turns GNOME's own do-not-disturb off during a mode, Then our mode ends.
* AC-FOCUS-09: Given idle for 60 s with a schedule armed, Then deskd made <= 2 wakeups.

## Open questions

1. Can the bridge let selected sources through while `show-banners=false` without private API? If not, exceptions
   are implemented by suppressing banners per source instead of globally.
2. Automatic rules (full screen, screen sharing) in the first release?
3. Focus filters (change theme or wallpaper per mode) via [auto-dark](auto-dark.md)-like hooks?
