#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Start an isolated headless GNOME Shell (1920x1080); env.sh lists what is isolated.
#
#   start.sh [--theme ID] [--ext DIR]... [--deskd BIN] [--set KEY=VALUE]... [--dark|--light]
#     --theme ID   load theme ID's dconf files (with INHERITS) into the PRIVATE dconf db and enable its
#                  extensions; never runs `themes apply`, which would write the real session
#     --ext DIR    overlay an extension directory (e.g. bridge/build/desk-bridge@desktop-themes-rs)
#                  over the installed one; it is enabled too
#     --deskd BIN  deskd binary the private bus activates (default: target/debug/deskd)
#     --set K=V    extra `dconf write K V` after the theme (e.g. /org/gnome/shell/x=true)
set -eu
H=$(cd "$(dirname "$0")" && pwd); . "$H/env.sh"
REPO=$(cd "$H/../.." && pwd)
THEME="" SCHEME=dark DESKD="$REPO/target/debug/deskd"
EXTS=() SETS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --theme) THEME=$2; shift 2 ;;
    --ext) EXTS+=("$(cd "$2" && pwd)"); shift 2 ;;
    --deskd) DESKD=$(realpath "$2"); shift 2 ;;
    --set) SETS+=("$2"); shift 2 ;;
    --dark) SCHEME=dark; shift ;;
    --light) SCHEME=light; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
"$H/stop.sh" >/dev/null 2>&1 || true

# ---- private XDG_DATA_HOME: symlinks to the real entries + overlays
REAL_DATA="$HOME/.local/share"
D="${HS_DATA:?}"
find "$D" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
for e in "$REAL_DATA"/*; do
  n=$(basename "$e")
  case "$n" in gnome-shell|glib-2.0|dbus-1|deskd|desktop-themes|keyrings|recently-used.xbel) continue ;; esac
  ln -s "$e" "$D/$n"
done
mkdir -p "$D/gnome-shell/extensions" "$D/glib-2.0/schemas" "$D/dbus-1/services"
for e in "$REAL_DATA"/gnome-shell/extensions/*; do
  [ -e "$e" ] && ln -s "$e" "$D/gnome-shell/extensions/$(basename "$e")"
done
OVERLAY=()
for d in "${EXTS[@]+"${EXTS[@]}"}"; do
  u=$(basename "$d")
  rm -f -- "$D/gnome-shell/extensions/${u:?}"
  ln -s "$d" "$D/gnome-shell/extensions/$u"
  OVERLAY+=("$u")
done
cp "$REPO/data/io.github.OlehDzhumyk.Desk.gschema.xml" "$D/glib-2.0/schemas/"
tbx glib-compile-schemas "$D/glib-2.0/schemas"
if [ -x "$DESKD" ]; then
  printf '[D-BUS Service]\nName=io.github.OlehDzhumyk.Desk\nExec=%s\n' "$DESKD" \
    > "$D/dbus-1/services/io.github.OlehDzhumyk.Desk.service"
fi

# ---- private bus (dbus-daemon from the toolbox: the host has none). It and everything it activates
# (dconf-service, deskd) carry the private environment.
sed "s|@SOCK@|$HS_BUS_SOCK|; s|@SERVICES@|$D/dbus-1/services|" "$H/bus.conf" > "$HS_STATE/bus.conf"
rm -f -- "${HS_BUS_SOCK:?}"
mapfile -t ENVV < <(hs_env)
tbx env "${ENVV[@]}" \
  dbus-daemon --config-file="$HS_STATE/bus.conf" --fork --print-pid > "$HS_STATE/bus.pid"
for _ in $(seq 50); do [ -S "$HS_BUS_SOCK" ] && break; sleep 0.1; done
echo "bus: $HS_BUS (pid $(cat "$HS_STATE/bus.pid"))"
while IFS= read -r kv; do export "${kv?}"; done < <(hs_env)
unset WAYLAND_DISPLAY DISPLAY

# ---- private dconf db
rm -f -- "${HS_CONF:?}/dconf/${HS_DCONF_DB:?}"
EXT_LIST=("${OVERLAY[@]+"${OVERLAY[@]}"}")
if [ -n "$THEME" ]; then
  "$H/theme-dconf.sh" "$THEME" > "$HS_STATE/theme.dconf"
  dconf load / < "$HS_STATE/theme.dconf"
  while read -r u; do
    case " ${EXT_LIST[*]-} " in *" $u "*) ;; *) [ -n "$u" ] && EXT_LIST+=("$u") ;; esac
  done < "$HS_STATE/theme.dconf.extensions"
fi
EXT=$(printf "'%s', " "${EXT_LIST[@]+"${EXT_LIST[@]}"}"); EXT="[${EXT%, }]"
dconf write /org/gnome/shell/enabled-extensions "$EXT"
dconf write /org/gnome/shell/disable-user-extensions false
dconf write /org/gnome/desktop/sound/event-sounds false
dconf write /org/gnome/desktop/interface/color-scheme "'$( [ "$SCHEME" = light ] && echo default || echo prefer-dark)'"
dconf write /org/gnome/shell/welcome-dialog-last-shown-version "'99'"
for kv in "${SETS[@]+"${SETS[@]}"}"; do dconf write "${kv%%=*}" "${kv#*=}"; done
echo "dconf: private db $HS_CONF/dconf/$HS_DCONF_DB; extensions: $EXT"

nohup gnome-shell --headless --wayland --no-x11 --wayland-display="$HS_WAYLAND" \
    --virtual-monitor 1920x1080 > "$HS_STATE/shell.log" 2>&1 &
echo $! > "$HS_STATE/shell.pid"
for _ in $(seq 120); do
  gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell -m org.freedesktop.DBus.Properties.Get org.gnome.Shell ShellVersion >/dev/null 2>&1 && break
  sleep 0.5
done
gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell -m org.freedesktop.DBus.Properties.Get org.gnome.Shell ShellVersion
# GNOME 51's notification daemon is a gjs process the bus would activate; the private bus lives in
# the toolbox and cannot start host programs, so start it by hand
if [ -f /usr/share/gnome-shell/org.gnome.Shell.Notifications ]; then
  nohup /usr/bin/gjs -m /usr/share/gnome-shell/org.gnome.Shell.Notifications > "$HS_STATE/notifications.log" 2>&1 &
  echo $! > "$HS_STATE/notifications.pid"
fi
sleep 8
gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell -m org.freedesktop.DBus.Properties.Set org.gnome.Shell OverviewActive "<false>" >/dev/null || true
echo "ready: WAYLAND_DISPLAY=$HS_WAYLAND (run commands with tools/headless/run.sh CMD)"
