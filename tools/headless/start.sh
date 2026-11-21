#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Запуск headless GNOME Shell 1920x1080 на приватній шині.
#   Справжня сесія користувача вже тримає org.gnome.Shell / org.gnome.Mutter.* на /run/user/1000/bus,
#   тому другий шел там конфліктував би (або замінив би справжній). Шина — dbus-daemon із toolbox
#   (на хості немає dbus-run-session/dbus-daemon), сокет у $XDG_RUNTIME_DIR, тож процеси хоста бачать її.
set -eu
H=$(cd "$(dirname "$0")" && pwd); . "$H/env.sh"
"$H/stop.sh" >/dev/null 2>&1 || true
sed "s|@SOCK@|$HS_BUS_SOCK|; s|@HOME@|$HOME|" "$H/bus.conf" > "$HS_STATE/bus.conf"
rm -f "$HS_BUS_SOCK"
toolbox run -c "$HS_TOOLBOX" sh -c "dbus-daemon --config-file='$HS_STATE/bus.conf' --fork --print-pid" > "$HS_STATE/bus.pid"
for _ in $(seq 50); do [ -S "$HS_BUS_SOCK" ] && break; sleep 0.1; done
echo "шина: $HS_BUS (pid $(cat "$HS_STATE/bus.pid"), у toolbox $HS_TOOLBOX)"
export DBUS_SESSION_BUS_ADDRESS="$HS_BUS"
unset WAYLAND_DISPLAY DISPLAY
XDG_SESSION_TYPE=wayland nohup gnome-shell --headless --wayland --no-x11 --wayland-display="$HS_WAYLAND" \
    --virtual-monitor 1920x1080 > "$HS_STATE/shell.log" 2>&1 &
echo $! > "$HS_STATE/shell.pid"
echo "gnome-shell pid $(cat "$HS_STATE/shell.pid"), журнал $HS_STATE/shell.log"
for _ in $(seq 120); do
  if gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell -m org.freedesktop.DBus.Properties.Get org.gnome.Shell ShellVersion >/dev/null 2>&1; then break; fi
  sleep 0.5
done
gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell -m org.freedesktop.DBus.Properties.Get org.gnome.Shell ShellVersion
# служба сповіщень GNOME 51 — окремий gjs-процес, який активує шина; приватна шина (з toolbox)
# не може його активувати, тож запускаємо з хоста вручну
if [ -f /usr/share/gnome-shell/org.gnome.Shell.Notifications ]; then
  nohup /usr/bin/gjs -m /usr/share/gnome-shell/org.gnome.Shell.Notifications > "$HS_STATE/notifications.log" 2>&1 &
  echo $! > "$HS_STATE/notifications.pid"
fi
# оболонка відкриває огляд при старті — закрити через ~10 с
sleep 10
gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell -m org.freedesktop.DBus.Properties.Set org.gnome.Shell OverviewActive "<false>" >/dev/null
echo "огляд закрито; готово"
