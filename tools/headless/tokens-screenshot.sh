#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Screenshots of a token theme in the isolated headless shell, light and dark: the desktop with a
# libadwaita window and the same window with its menu open (SHOT_QUICK_SETTINGS=1 adds quick
# settings, which show the real network name: keep those private). Nothing of the real
# session is touched: the generated files go to a private theme root under this environment's
# cache, the shell theme is linked into the PRIVATE data dir and gtk.css into the PRIVATE config dir.
#
#   tokens-screenshot.sh THEME OUTDIR [THEMES_BIN]     (THEMES_BIN default: target/debug/themes)
set -eu
H=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$H/../.." && pwd)
THEME=$1 OUT=$2 BIN=$(realpath "${3:-$REPO/target/debug/themes}")
export HS_NAME="${HS_NAME:-tokens}"
export THEMES_DIR="${THEMES_DIR:-$REPO/themes}"
. "$H/env.sh"
R="$H/run.sh"
mkdir -p "$OUT"
# private theme root: the repository's themes, cache inside the root
DT="$HS_ROOT/dt-root"
mkdir -p "$DT"
ln -sfn "$THEMES_DIR" "$DT/themes"
export DESKTOP_THEMES_ROOT="$DT"
"$BIN" tokens generate "$THEME"
CACHE="$DT/.cache/$THEME"
UT=user-theme@gnome-shell-extensions.gcampax.github.com
UT_DIR=""
for d in "$HOME/.local/share/gnome-shell/extensions/$UT" "/usr/share/gnome-shell/extensions/$UT"; do
  [ -d "$d" ] && UT_DIR=$d && break
done
trap '"$H/stop.sh" >/dev/null 2>&1' EXIT
for scheme in light dark; do
  args=(--theme "$THEME" "--$scheme")
  [ -n "$UT_DIR" ] && args+=(--ext "$UT_DIR")
  "$H/start.sh" "${args[@]}" </dev/null >"$HS_STATE/start.log" 2>&1 || { cat "$HS_STATE/start.log"; exit 1; }
  # private data dir: replace the link to the real themes dir by a dir of links + the generated theme
  if [ -d "$CACHE/shell" ]; then
    real=$(readlink "$HS_DATA/themes" || true)
    rm -f -- "$HS_DATA/themes"
    mkdir -p "$HS_DATA/themes"
    [ -n "$real" ] && for t in "$real"/*; do ln -s "$t" "$HS_DATA/themes/$(basename "$t")"; done
    ln -sfn "$CACHE/shell" "$HS_DATA/themes/desktop-themes-$THEME"
  fi
  mkdir -p "$HS_CONF/gtk-4.0"
  rm -f -- "$HS_CONF/gtk-4.0/gtk.css"
  [ -f "$CACHE/gtk4/gtk.css" ] && cp "$CACHE/gtk4/gtk.css" "$HS_CONF/gtk-4.0/gtk.css"
  "$BIN" tokens keys "$THEME" > "$HS_STATE/tokens.dconf"
  "$R" dconf load / < "$HS_STATE/tokens.dconf"
  sleep 3
  # no settings portal on the private bus: libadwaita is told the scheme directly
  ADW=prefer-light; [ "$scheme" = dark ] && ADW=prefer-dark
  "$R" env ADW_DEBUG_COLOR_SCHEME=$ADW setsid python3 "$H/adw-demo.py" </dev/null >"$HS_STATE/demo.log" 2>&1 &
  sleep 4
  "$R" python3 "$H/screenshot.py" "$OUT/$THEME-$scheme-desktop.png" </dev/null
  "$R" env ADW_DEBUG_COLOR_SCHEME=$ADW setsid python3 "$H/adw-demo.py" --menu </dev/null >"$HS_STATE/demo2.log" 2>&1 &
  sleep 4
  "$R" python3 "$H/screenshot.py" "$OUT/$THEME-$scheme-menu.png" </dev/null
  # quick settings show the machine's real network and device names: only on request, never commit
  if [ "${SHOT_QUICK_SETTINGS:-0}" = 1 ]; then
    "$R" python3 "$H/input.py" key:Escape sleep:0.5 key:Super+s sleep:1.5 "shot:$OUT/$THEME-$scheme-quick-settings.png" key:Escape </dev/null
  fi
  grep -i "user theme\|JS ERROR\|gnome-shell.css" "$HS_STATE/shell.log" | tail -n 5 || true
  "$H/stop.sh" >/dev/null 2>&1 || true
done
ls -1 "$OUT"/"$THEME"-*.png
