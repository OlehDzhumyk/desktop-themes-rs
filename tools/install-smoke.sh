#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Smoke test of data/install.sh in a throw-away HOME (no session, no systemd, no real binaries):
#   1. --dry-run changes nothing,
#   2. a real install puts every expected file in place (compiled schemas of the app and of the
#      extensions that ship one, translated [uk] .desktop and metainfo, catalogues, units,
#      desk-shell and the labwc session files),
#   3. a second install over the first still works (upgrade path),
#   4. an install without a built desk-shell skips it,
#   5. desk-win11-session --dry-run refreshes a bundled themerc and keeps a token-generated one,
#   6. --no-gnome-bridge installs core, desk-shell and labwc without desk-bridge or npm.
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
# stand-ins for what needs a session or a build: the binaries, systemctl and labwc
for b in themes deskd desk-shell systemctl labwc; do printf '#!/bin/sh\nexit 0\n' > "$tmp/stub/$b"; chmod +x "$tmp/stub/$b"; done
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

echo "== desk-win11-session --dry-run"
"$HOME/.local/bin/desk-win11-session" --dry-run > "$tmp/session1.log"
themerc="$D/themes/desk-win11/openbox-3/themerc"
check "session --dry-run prints labwc command" has_text "$tmp/session1.log" "labwc -C"
check "session sets up desk-labwc rc.xml" has "$XDG_CONFIG_HOME/desk-labwc/rc.xml"
check "session installs bundled themerc on first run" has_text "$themerc" 'labwc theme "desk-win11"'

printf '# stale bundled themerc\n' > "$themerc"
"$HOME/.local/bin/desk-win11-session" --dry-run > /dev/null
check "session refreshes non-token themerc" has_text "$themerc" 'labwc theme "desk-win11"'

cat > "$themerc" <<'EOF'
# Generated by desktop-themes-rs from themes/lavender/tokens.toml. Do not edit: change the tokens and apply the theme again.
window.active.title.bg.color: #262232
EOF
"$HOME/.local/bin/desk-win11-session" --dry-run > /dev/null
check "session preserves token-generated themerc header" \
  has_text "$themerc" '^# Generated by desktop-themes-rs from themes/lavender/tokens\.toml\.'
check "session preserves token-generated themerc values" \
  has_text "$themerc" '^window\.active\.title\.bg\.color: #262232$'

echo "== install with --no-gnome-bridge"
for b in npm toolbox; do printf '#!/bin/sh\nexit 1\n' > "$tmp/stub/$b"; chmod +x "$tmp/stub/$b"; done
export HOME="$tmp/home-nobridge"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_CONFIG_HOME="$HOME/.config"
mkdir -p "$HOME" "$tmp/src-nobridge/data"
ln -s "$SRC"/data/* "$tmp/src-nobridge/data/"
ln -s "$SRC/po" "$SRC/extensions" "$tmp/src-nobridge/"
"$tmp/src-nobridge/data/install.sh" --no-gnome-bridge "${args[@]}" > "$tmp/nobridge-missing.log"
check "--no-gnome-bridge works without bridge/build or npm" \
  has "$XDG_DATA_HOME/glib-2.0/schemas/gschemas.compiled"

rm -rf "$HOME"
mkdir -p "$HOME"
"$SRC/data/install.sh" --no-gnome-bridge "${args[@]}" > "$tmp/nobridge.log"
D=$XDG_DATA_HOME
check "--no-gnome-bridge skips desk-bridge@desktop-themes-rs" \
  test ! -e "$D/gnome-shell/extensions/desk-bridge@desktop-themes-rs"
check "--no-gnome-bridge reports skipped bridge" \
  has_text "$tmp/nobridge.log" "Not installed: desk-bridge@desktop-themes-rs"
for f in \
  "$HOME/.local/bin/themes" "$HOME/.local/bin/theme" "$HOME/.local/bin/themes-gui" \
  "$HOME/.local/bin/deskd" "$HOME/.local/bin/desk-shell" "$HOME/.local/bin/desk-win11-session" \
  "$D/desktop-themes/labwc/windows11/rc.xml" "$D/desktop-themes/labwc/windows11/autostart" \
  "$D/desktop-themes/labwc/windows11/capture" "$D/desktop-themes/labwc/windows11/themerc" \
  "$D/glib-2.0/schemas/$DESK_ID.gschema.xml" "$D/glib-2.0/schemas/gschemas.compiled" \
  "$D/dbus-1/services/$APP_ID.service" "$D/dbus-1/services/$DESK_ID.service" \
  "$D/applications/$APP_ID.desktop" "$D/metainfo/$APP_ID.metainfo.xml" \
  "$D/locale/uk/LC_MESSAGES/desktop-themes-rs.mo" \
  "$XDG_CONFIG_HOME/systemd/user/deskd.service" "$XDG_CONFIG_HOME/systemd/user/themes-doctor.service"; do
  check "--no-gnome-bridge exists ${f#"$HOME"/}" has "$f"
done

[ "$fail" = 0 ] && echo "install smoke test passed" || { echo "install smoke test FAILED" >&2; exit 1; }
