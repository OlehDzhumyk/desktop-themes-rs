# 0005. Headless testing must be fully isolated from the live session

* Status: accepted
* Date: 2026-11-21

## Context

Applying a theme writes dconf, toggles shell extensions and has crashed GNOME Shell on real
hardware; GNOME then disables all extensions (crash protection). Tests that touch the developer's
live session can lose their desktop state, and agents run many commands unattended.

## Decision

* End-to-end tests run only in `tools/headless/`: own private D-Bus bus, Wayland display,
  `XDG_RUNTIME_DIR` subdirectory, dconf database (`XDG_CONFIG_HOME` + `DCONF_PROFILE`) and data
  directory; `tools/perf/` refuses to run against any other bus.
* Everything goes through `tools/headless/run.sh`; the real bus address is never exported.
* `stop.sh` kills only PIDs it started or processes carrying its own bus address; never by pattern.
* Unit tests never call `dconf write`, `gsettings set` or `gio::Settings::set_*` against the default
  backend (ast-grep rule `tools/ast-grep/rules`); the Claude hook blocks the same commands outside
  an isolated environment.
* `tools/integration-test.sh` writes the real dconf and is VM-only.

## Consequences

* Good: a failing or crashing test cannot damage the developer's session.
* Bad: the harness needs `dbus-daemon` and gnome-shell, so it is not part of required CI
  (`headless.yml` runs on demand and weekly).

End-to-end tests moved into a container: [ADR 0007](0007-e2e-in-containers.md).

## Alternatives considered

* Run tests on the live session with a backup/restore: a crash mid-test still loses state.
* A full VM for every run: too slow for iteration; kept for the integration test.
