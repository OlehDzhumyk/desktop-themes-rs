#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prepare a plain Fedora container (root, no systemd, no logind, no GPU) for tools/headless:
# used by CI (.github/workflows/headless.yml) and tools/perf in containers. Run once, as root,
# before start.sh. Together with HS_TOOLBOX=none (dbus-daemon etc. are installed in the same
# container) and HS_NO_SHOTS=1 (no PipeWire: key events only, no screenshots).
#
# What it does and why:
#   * removes /run/systemd/seats, which the dnf transaction creates: with it present gnome-shell
#     picks its logind login manager and dies on the missing system bus; without it it uses the
#     dummy one;
#   * starts a system D-Bus (gnome-shell asks it for a few optional services);
#   * creates $XDG_RUNTIME_DIR (mutter renders in software, "surfaceless renderer without GPU").
set -eu
rmdir /run/systemd/seats 2>/dev/null || true
if [ ! -S /run/dbus/system_bus_socket ]; then
  mkdir -p /run/dbus
  dbus-daemon --system --fork
fi
RT=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
mkdir -p "$RT"
chmod 700 "$RT"
echo "ci-env: ready (XDG_RUNTIME_DIR=$RT)"
