#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Зупинити все, що запустив start.sh (шел, приватну шину й активовані нею служби).
H=$(cd "$(dirname "$0")" && pwd); . "$H/env.sh"
for f in notifications.pid shell.pid; do
  [ -f "$HS_STATE/$f" ] && kill "$(cat "$HS_STATE/$f")" 2>/dev/null && echo "зупинено gnome-shell $(cat "$HS_STATE/$f")"
  rm -f "$HS_STATE/$f"
done
sleep 1
# процеси хоста, підключені до приватної шини (themes --dbus-service, gui тощо)
for p in $(pgrep -u "$(id -u)" -f . ); do
  [ "$p" = "$$" ] || [ "$p" = "$PPID" ] && continue
  if { tr '\0' '\n' < /proc/$p/environ; } 2>/dev/null | grep -qx "DBUS_SESSION_BUS_ADDRESS=$HS_BUS"; then
    kill "$p" 2>/dev/null && echo "зупинено $p ($(tr '\0' ' ' < /proc/$p/cmdline | cut -c1-60))"
  fi
done
# у toolbox працює лише наша шина та процеси, які вона активувала (dconf-service, themes --dbus-service)
if [ -f "$HS_STATE/bus.pid" ]; then
  toolbox run -c "$HS_TOOLBOX" sh -c "pkill -x dbus-daemon; pkill -x dconf-service; pkill -x themes; true" 2>/dev/null
  echo "зупинено шину $(cat "$HS_STATE/bus.pid") і активовані нею служби"; rm -f "$HS_STATE/bus.pid"
fi
rm -f "$HS_BUS_SOCK" "$HS_DIR/$HS_WAYLAND" "$HS_DIR/$HS_WAYLAND.lock"
