#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Smoke test of data/install.sh in a throw-away HOME (no session, no systemd, no real binaries):
#   1. --dry-run changes nothing,
#   2. a real install puts every expected file in place (compiled schemas of the app and of the
#      extensions that ship one, translated [uk] .desktop and metainfo, catalogues, units,
#      desk-shell and the labwc session files),
#   3. a second install over the first still works (upgrade path),
#   4. an install without a built desk-shell skips it.
# Needs glib-compile-schemas, msgfmt and a built bridge (bridge/build, see `npm run build`).
# Safe to run on a developer machine: HOME, XDG_* and PATH point into a temporary directory.
set -euo pipefail
SRC=$(cd "$(dirname "$0")/.." && pwd)
APP_ID=io.github.OlehDzhumyk.Themes
DESK_ID=io.github.OlehDzhumyk.Desk

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export HOME="$tmp/home"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_CONFIG_HOME="$HOME/.config"
unset DESKTOP_THEMES_ROOT DBUS_SESSION_BUS_ADDRESS
mkdir -p "$HOME" "$tmp/stub"
# stand-ins for what needs a session or a build: the binaries and systemctl
for b in themes deskd desk-shell systemctl; do printf '#!/bin/sh\nexit 0\n' > "$tmp/stub/$b"; chmod +x "$tmp/stub/$b"; done
export PATH="$tmp/stub:$PATH"
args=(--bin "$tmp/stub/themes" --deskd "$tmp/stub/deskd" --desk-shell "$tmp/stub/desk-shell")

fail=0
check() { # check DESCRIPTION TEST-ARGS...
  local what=$1; shift
  if "$@"; then echo "ok   $what"; else echo "FAIL $what" >&2; fail=1; fi
}
has() { test -e "$1"; }
has_text() { grep -q -- "$2" "$1"; }

echo "== dry run"
"$SRC/data/install.sh" --dry-run "${args[@]}" > "$tmp/dry.log"
check "dry run prints its plan" has_text "$tmp/dry.log" "glib-compile-schemas"
check "dry run creates nothing" test -z "$(find "$HOME" -mindepth 1 | head -n 1)"

echo "== real install"
"$SRC/data/install.sh" "${args[@]}" > "$tmp/install.log"
D=$XDG_DATA_HOME
for f in \
  "$HOME/.local/bin/themes" "$HOME/.local/bin/deskd" "$HOME/.local/bin/theme" \
  "$HOME/.local/bin/desk-shell" "$HOME/.local/bin/desk-win11-session" \
  "$D/desktop-themes/labwc/windows11/rc.xml" "$D/desktop-themes/labwc/windows11/autostart" \
  "$D/glib-2.0/schemas/$DESK_ID.gschema.xml" "$D/glib-2.0/schemas/gschemas.compiled" \
  "$D/dbus-1/services/$APP_ID.service" "$D/dbus-1/services/$DESK_ID.service" \
  "$D/applications/$APP_ID.desktop" "$D/metainfo/$APP_ID.metainfo.xml" \
  "$D/locale/uk/LC_MESSAGES/desktop-themes-rs.mo" \
  "$XDG_CONFIG_HOME/systemd/user/deskd.service" "$XDG_CONFIG_HOME/systemd/user/themes-doctor.service" \
  "$D/gnome-shell/extensions/desk-bridge@desktop-themes-rs/extension.js" \
  "$D/gnome-shell/extensions/desk-bridge@desktop-themes-rs/metadata.json"; do
  check "exists ${f#"$HOME"/}" has "$f"
done

# every extension of the repository is installed; those with a schema have it compiled
for d in "$SRC"/extensions/*/; do
  uuid=$(basename "$d")
  check "extension $uuid installed" has "$D/gnome-shell/extensions/$uuid/metadata.json"
  if ls "$d"schemas/*.gschema.xml >/dev/null 2>&1; then
    check "extension $uuid: schemas/gschemas.compiled" has "$D/gnome-shell/extensions/$uuid/schemas/gschemas.compiled"
  fi
done
# the bridge builds with a schema of its own, too
bd="$D/gnome-shell/extensions/desk-bridge@desktop-themes-rs"
if ls "$bd"/schemas/*.gschema.xml >/dev/null 2>&1; then
  check "desk-bridge: schemas/gschemas.compiled" has "$bd/schemas/gschemas.compiled"
fi

# translated entries made by msgfmt from the templates
check ".desktop has Name[uk]" has_text "$D/applications/$APP_ID.desktop" '^Name\[uk\]='
check ".desktop has Comment[uk]" has_text "$D/applications/$APP_ID.desktop" '^Comment\[uk\]='
check ".desktop Exec points into the temporary HOME" has_text "$D/applications/$APP_ID.desktop" "^Exec=$HOME/.local/bin/"
check "metainfo has a uk translation" has_text "$D/metainfo/$APP_ID.metainfo.xml" 'xml:lang="uk"'
check "no @BIN@ placeholder left" test -z "$(grep -rl '@BIN@' "$D/applications" "$D/dbus-1" 2>/dev/null || true)"

check "labwc autostart stays executable" test -x "$D/desktop-themes/labwc/windows11/autostart"

echo "== second install over the first"
"$SRC/data/install.sh" "${args[@]}" > "$tmp/install2.log"
check "upgrade keeps the compiled schemas" has "$D/glib-2.0/schemas/gschemas.compiled"
check "upgrade keeps the labwc config" has "$D/desktop-themes/labwc/windows11/rc.xml"

echo "== install without desk-shell"
rm -f "$HOME/.local/bin/desk-shell"
"$SRC/data/install.sh" --bin "$tmp/stub/themes" --deskd "$tmp/stub/deskd" --desk-shell "$tmp/none" > "$tmp/install3.log"
check "a missing desk-shell is skipped" test ! -e "$HOME/.local/bin/desk-shell"
check "the skip is reported" has_text "$tmp/install3.log" "Not installed: desk-shell"

[ "$fail" = 0 ] && echo "install smoke test passed" || { echo "install smoke test FAILED" >&2; exit 1; }
