# SPDX-License-Identifier: GPL-3.0-or-later
# shellcheck shell=bash
# Спільні змінні для headless-тестів (source env.sh)
export HS_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export HS_BUS_SOCK="$HS_DIR/hs-bus"
export HS_BUS="unix:path=$HS_BUS_SOCK"
export HS_WAYLAND=wayland-hs
export HS_STATE="$HS_DIR/themes-headless"
export HS_TOOLBOX="${HS_TOOLBOX:-desktop-themes-build}"
mkdir -p "$HS_STATE"
