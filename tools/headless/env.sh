# SPDX-License-Identifier: GPL-3.0-or-later
# shellcheck shell=bash
# Shared variables of the headless test environment (source env.sh).
#
# The environment is fully isolated from the user's real session:
#   * own private D-Bus (dbus-daemon from the build toolbox) and own Wayland display;
#   * own XDG_RUNTIME_DIR subdirectory: the shell's crash-protection file
#     gnome-shell-disable-extensions, the Wayland socket and dconf's shm never land in the real one
#     (PipeWire is still reached through PIPEWIRE_RUNTIME_DIR, for screenshots);
#   * own XDG_CONFIG_HOME + DCONF_PROFILE => own dconf database (the real ~/.config/dconf/user and
#     ~/.config/gtk-4.0 are never written);
#   * own XDG_DATA_HOME made of symlinks to the real ~/.local/share entries (themes, icons, fonts,
#     backgrounds, installed extensions) plus overlays: extensions under test, GSettings schemas and
#     a private D-Bus service directory (only deskd; never the real `themes` service);
#   * stop.sh kills only PIDs it started or processes carrying this environment's bus address.
# Several environments can run side by side: HS_NAME=foo tools/headless/start.sh
export HS_NAME="${HS_NAME:-dt}"
export HS_REAL_RUNTIME="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
# if this file is sourced from inside the environment, XDG_RUNTIME_DIR is already ours
case "$HS_REAL_RUNTIME" in */headless-*) HS_REAL_RUNTIME="${HS_REAL_RUNTIME%/headless-*}" ;; esac
export HS_RUNTIME="$HS_REAL_RUNTIME/headless-$HS_NAME"
export HS_STATE="$HS_RUNTIME/state"
export HS_BUS_SOCK="$HS_RUNTIME/bus"
export HS_BUS="unix:path=$HS_BUS_SOCK"
export HS_WAYLAND="$HS_RUNTIME/wayland-hs"
export HS_ROOT="${XDG_CACHE_HOME:-$HOME/.cache}/desktop-themes-headless/$HS_NAME"
case "$HS_ROOT" in */desktop-themes-headless/*) ;; *) HS_ROOT="$HOME/.cache/desktop-themes-headless/$HS_NAME" ;; esac
export HS_CONF="$HS_ROOT/config"
export HS_DATA="$HS_ROOT/data"
export HS_TOOLBOX="${HS_TOOLBOX:-desktop-themes-build}"
# HS_TOOLBOX=none: dbus-daemon & co. are installed where the shell runs (CI container): no toolbox
tbx() { if [ "$HS_TOOLBOX" = none ]; then "$@"; else toolbox run -c "$HS_TOOLBOX" "$@"; fi; }
mkdir -p "$HS_RUNTIME" "$HS_STATE" "$HS_CONF/dconf" "$HS_DATA"
chmod 700 "$HS_RUNTIME"
# dconf db names become D-Bus object path elements: letters, digits and _ only
export HS_DCONF_DB="headless_${HS_NAME//[^A-Za-z0-9_]/_}"
printf 'user-db:%s\n' "$HS_DCONF_DB" > "$HS_STATE/dconf-profile"

# environment of every process of the headless session (one VAR=value per line)
hs_env() {
  printf '%s\n' \
    "DBUS_SESSION_BUS_ADDRESS=$HS_BUS" \
    "XDG_RUNTIME_DIR=$HS_RUNTIME" \
    "PIPEWIRE_RUNTIME_DIR=$HS_REAL_RUNTIME" \
    "XDG_CONFIG_HOME=$HS_CONF" \
    "XDG_DATA_HOME=$HS_DATA" \
    "DCONF_PROFILE=$HS_STATE/dconf-profile" \
    "DESKD_RUNTIME_DIR=$HS_RUNTIME/deskd" \
    "XDG_SESSION_TYPE=wayland"
}
