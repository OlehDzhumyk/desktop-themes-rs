# 0009. deskd writes the do-not-disturb keys; the bridge only adds exceptions

* Status: accepted
* Date: 2026-11-23

## Context

Focus modes ([spec](../spec/features/focus.md)) hide notification banners by setting
`org.gnome.desktop.notifications show-banners` to false, and the Sleep mode also turns
`org.gnome.desktop.sound event-sounds` off. The spec first proposed that the shell bridge sets and
clears these keys while deskd keeps the state. But the value to restore must survive a deskd crash,
a shell restart and a logout, and banners must never stay blocked without an active mode. The
bridge is not running when the shell is down, and deskd already owns the state file and the
schedules. Allowed apps (exceptions) need a hook inside GNOME Shell, which only the bridge has.

## Decision

deskd writes `show-banners` and `event-sounds` itself, like the appearance engine writes
`color-scheme`. The bridge does not write them; it shows the tile and the status icon and lets the
allowed apps of the active mode through.

* deskd stores the value it overrides in `~/.local/share/deskd/focus.json` (0600) **before** it
  writes, and restores it when the mode ends, but only while the key still holds what deskd wrote.
* At start-up deskd reconciles: a mode that should still run continues; an expired one is ended
  and the value restored. If the state file is lost while `active-mode` is set, banners are turned
  back on.
* A change of `show-banners` to true by anyone else during a mode ends the mode (the user wins).
* Allowed apps: the bridge wraps the `showBanners` getter of GNOME's
  `NotificationApplicationPolicy` while it is enabled, so a listed app answers with its own per-app
  setting instead of the global switch. Critical notifications pass in GNOME already. If the getter
  is missing on a future shell, nothing is wrapped and all banners stay hidden (the safe side).

## Consequences

* Good: one writer and one source of truth; the revert works with no shell running; the logic is
  tested in Rust without a shell (`crates/deskd/src/focus/`).
* Good: GNOME's own switch and its date-menu indicator stay in step with our modes.
* Bad / cost: the allowed-apps exception relies on a shell-internal getter (a private API);
  it must be checked on each GNOME release.

## Alternatives considered

* The bridge writes the keys (the first spec text): no revert while the shell is down, and two
  processes would need to agree on the saved value.
* Per-source banner suppression in the bridge without touching `show-banners`: GNOME's own
  do-not-disturb switch and indicator would not reflect a mode, and it needs a deeper private hook
  into the message tray.
