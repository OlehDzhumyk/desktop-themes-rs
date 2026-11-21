#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Виконати команду в оточенні headless-шела: ./run.sh themes apply macos
H=$(cd "$(dirname "$0")" && pwd); . "$H/env.sh"
export DBUS_SESSION_BUS_ADDRESS="$HS_BUS" WAYLAND_DISPLAY="$HS_WAYLAND" XDG_SESSION_TYPE=wayland
unset DISPLAY
exec "$@"
