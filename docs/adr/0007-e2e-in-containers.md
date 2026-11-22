# 0007. End-to-end tests run GNOME Shell in a throw-away container, driven over Eval

* Status: accepted
* Date: 2026-11-22

## Context

`tools/headless/` (ADR 0005) isolates a headless shell inside the developer's own account: a
private bus, runtime dir and dconf database, but the host's processes, the user's real
`~/.local/share` (linked in) and the toolbox around it. It has one shell-script test, timing by
`sleep`, no assertions on the shell's log and no way to look inside the shell: GNOME Shell 51 only
answers `org.gnome.Shell.Eval` in `--unsafe-mode`, which we never want near the real session.

ddterm's test suite (GPL-3.0-or-later, CI files CC0) solves the same problem completely: pytest
starts `gnome-shell --headless --unsafe-mode` with a virtual monitor inside a podman container whose
PID 1 is systemd, from images built by ddterm/gnome-shell-image (MIT); a helper module is loaded
into the shell with Eval; the shell's output is parsed for errors. Its harness is about 7,000 lines
of fixtures for ddterm's own windows, Xorg/Wayland matrices and app hooks that we do not need.

## Decision

We port ddterm's approach, not its whole fixture tree: `tests/e2e/` is a small pytest harness
(`harness/shell.py`, `harness/hook.js`) that runs inside a container built FROM the pinned
`ghcr.io/ddterm/gnome-shell-image/fedora-45` image (GNOME Shell 51.1, the version we target).
ddterm's log parser and GitHub annotations plugin are copied unchanged; the rest is rewritten for
our bridge, deskd and the `themes` CLI. `tools/e2e.sh` (`just e2e`) builds, starts the container
and runs pytest in it.

* The container is the isolation boundary: the repository is mounted read-only, nothing of the
  host session (bus, runtime dir, home) is mounted, and each test gets its own HOME, XDG dirs,
  dconf database and private session bus inside it. `--unsafe-mode` and Eval exist only there.
* The bridge gets **no** test-only D-Bus interface or debug flag: the tests read and drive it
  through Eval and the hook module (keyboard via a Clutter virtual device, screenshots via
  `Shell.Screenshot`, leak tracking by wrapping `connect`/`timeout_add` while it is enabled).
* No sleeps: tests wait on D-Bus signals (ExtensionStateChanged, deskd's Changed), on promises
  the hook resolves from shell signals, or on a child's output line; timeouts only bound a hang.
* Every test fails on a JS error or critical from the bridge in the shell log.
* CI runs the same script on a GitHub runner with rootless podman (`e2e.yml`), on every pull
  request, because the suite takes about a minute plus the build.

## Consequences

* Good: the tests see what a user sees (enable, shortcut, paste, live theme apply, logout) and what
  the shell sees (actors, signal handlers, log); a test cannot touch the developer's desktop.
* Good: the first runs found bridge bugs the shell-script test could not see: quick settings menus
  left on the stage after disable, destroyed actors touched by late D-Bus replies at logout.
* Bad: the image is about 1.7 GB; the first run pulls it. Bumping it means a new tag and digest.
* Bad: Eval reaches into private bridge fields (`_clipboard`, `_panel`); a refactor of the bridge
  may need a matching change in `harness/hook.js`.
* `tools/headless/` stays for interactive use, screenshots of real themes with the user's installed
  assets, and `tools/perf/`; new end-to-end tests go to `tests/e2e/`.

## Alternatives considered

* Copy ddterm's harness wholesale: most of it serves ddterm's terminal window and its matrix of
  distributions; adapting it would cost more than the 600 lines we need.
* Extend `tools/headless/`: no Eval outside unsafe mode, and unsafe mode next to the real session
  breaks ADR 0005.
* A test-only D-Bus interface in the bridge behind a setting: production code that must never be
  switched on, for something Eval already gives inside the container.
