#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Stop what start.sh started: the PIDs it recorded, processes whose environment carries THIS
# environment's private bus or config dir, and our dbus-daemon by PID. The toolbox shares the host
# PID namespace, so nothing is ever killed by name.
H=$(cd "$(dirname "$0")" && pwd); . "$H/env.sh"
for f in notifications.pid shell.pid; do
  p="${HS_STATE:?}/$f"
  [ -f "$p" ] && kill "$(cat "$p")" 2>/dev/null && echo "stopped $(cat "$p") ($f)"
  rm -f -- "$p"
done
sleep 1
for p in $(pgrep -u "$(id -u)" -f .); do
  [ "$p" = "$$" ] || [ "$p" = "$PPID" ] && continue
  if { tr '\0' '\n' < "/proc/$p/environ"; } 2>/dev/null | grep -qx -e "DBUS_SESSION_BUS_ADDRESS=$HS_BUS" -e "XDG_CONFIG_HOME=$HS_CONF"; then
    kill "$p" 2>/dev/null && echo "stopped $p ($(tr '\0' ' ' < "/proc/$p/cmdline" | cut -c1-60))"
  fi
done
pidf="${HS_STATE:?}/bus.pid"
if [ -f "$pidf" ]; then
  kill "$(cat "$pidf")" 2>/dev/null && echo "stopped bus $(cat "$pidf")"
  rm -f -- "$pidf"
fi
rm -f -- "${HS_BUS_SOCK:?}" "${HS_WAYLAND:?}" "${HS_WAYLAND:?}.lock"
