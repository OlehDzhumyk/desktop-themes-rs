# 0002. One Rust daemon (deskd) over zbus, plus a thin shell bridge

* Status: accepted
* Date: 2026-11-21

## Context

Desktop features (first the clipboard history, later more) need logic, storage and privacy rules,
and a UI that lives in the shell. GNOME Shell is a single process whose stall freezes the desktop.
The theme service must not block on theme switches, which can take seconds.
See [docs/architecture.md](../architecture.md).

## Decision

* `deskd` is one session daemon (`io.github.OlehDzhumyk.Desk`, D-Bus activated, written in Rust with
  `zbus`) hosting feature engines. It decides what to keep, stores it and exposes a D-Bus API
  (`desk-proto`).
* The bridge is thin ([ADR 0001](0001-typescript-bridge-not-wasm.md)): capture, draw, forward.
* Shared user settings live in one GSettings schema read by deskd, the bridge and the window; there
  is no second settings dialog.
* The theme D-Bus service stays in the `themes` binary for now (on demand, exits when idle); moving
  it into deskd later changes no client.

## Consequences

* Good: shell code stays small and cannot hold business logic; deskd is testable without a shell.
* Good: crash isolation; a deskd bug does not take down GNOME Shell.
* Bad: one more process and an IPC boundary; the D-Bus interface XML in `data/dbus/` is a contract
  that must be versioned carefully.

## Alternatives considered

* Logic inside the extension: untestable, and a bug can crash the shell.
* One daemon per feature: more processes, duplicated storage and privacy code.
