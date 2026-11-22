#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# End-to-end test of the clipboard history in an isolated headless shell:
#   start the shell with a theme (skin) + the freshly built bridge + target/debug/deskd, open a GTK 4
#   text field, copy three sample texts with wl-copy, open the panel with the theme's shortcut,
#   take screenshots, pick the 2nd item with the keyboard and check it was pasted into the field.
#
#   clipboard-test.sh THEME SHORTCUT OUTDIR      e.g. clipboard-test.sh windows11 Super+v /tmp/shots
# Themes are read from the repository (THEMES_DIR defaults to ../../themes).
set -eu
H=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$H/../.." && pwd)
THEME=$1 KEYS=$2 OUT=$3
export THEMES_DIR="${THEMES_DIR:-$REPO/themes}"
mkdir -p "$OUT"
R="$H/run.sh"
. "$H/env.sh"
"$H/start.sh" --theme "$THEME" --ext "$REPO/bridge/build/desk-bridge@desktop-themes-rs" </dev/null >"$HS_STATE/start.log" 2>&1 \
  || { cat "$HS_STATE/start.log"; exit 1; }
trap '"$H/stop.sh" >/dev/null 2>&1' EXIT
"$R" "$H/ext-states.sh" | grep desk-bridge
PASTED="$HS_STATE/pasted.txt"
rm -f -- "${PASTED:?}"
"$R" setsid python3 "$H/paste-target.py" "$PASTED" </dev/null >"$HS_STATE/target.log" 2>&1 &
sleep 3
SAMPLES=("Перший зразок тексту" "Second sample: hello world" "Третій зразок — 3")
for t in "${SAMPLES[@]}"; do
  "$R" setsid wl-copy -- "$t" </dev/null >/dev/null 2>&1
  sleep 1.2
done
echo "deskd List:"
"$R" gdbus call --session -d io.github.OlehDzhumyk.Desk -o /io/github/OlehDzhumyk/Desk/Clipboard \
  -m io.github.OlehDzhumyk.Desk.Clipboard.List "" 0
"$R" python3 "$H/input.py" "key:$KEYS" sleep:1.5 "shot:$OUT/$THEME-panel.png" key:Down sleep:0.6 \
  "shot:$OUT/$THEME-selected.png" key:Return sleep:1.5 "shot:$OUT/$THEME-pasted.png" </dev/null
got=$(cat "$PASTED" 2>/dev/null || true)
want=${SAMPLES[1]}
if [ "$got" = "$want" ]; then
  echo "PASS [$THEME]: pasted «$got»"
else
  echo "FAIL [$THEME]: pasted «$got», expected «$want»"
  grep -i "desk-bridge\|JS ERROR" "$HS_STATE/shell.log" | tail -n 20 || true
  exit 1
fi
