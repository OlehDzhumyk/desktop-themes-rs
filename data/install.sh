#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# User-level install of the built binary and its desktop integration (no sudo).
# Run on the host after `cargo build --release` (scripts/bootstrap.sh does all of it).
#
# Usage: data/install.sh [--dry-run] [--bin PATH]
#   --bin PATH  install this binary instead of target/release/themes
set -euo pipefail
SRC=$(cd "$(dirname "$0")/.." && pwd)
BIN_SRC="$SRC/target/release/themes"
DRY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run|-n) DRY=1; shift ;;
    --bin) BIN_SRC=$2; shift 2 ;;
    -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
run() { if [ "$DRY" = 1 ]; then printf '    + '; printf '%q ' "$@"; printf '\n'; else "$@"; fi; }

APP_ID=io.github.OlehDzhumyk.Themes
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
BIN="$HOME/.local/bin"
ROOT="${DESKTOP_THEMES_ROOT:-$DATA/desktop-themes}"

[ "$DRY" = 1 ] || [ -x "$BIN_SRC" ] || { echo "binary not found: $BIN_SRC (build it first: cargo build --release)" >&2; exit 1; }
run mkdir -p "$BIN" "$DATA/dbus-1/services" "$HOME/.config/systemd/user" "$DATA/applications" "$DATA/gnome-shell/extensions"

run install -m 0755 "$BIN_SRC" "$BIN/themes.new"
run mv -f "$BIN/themes.new" "$BIN/themes"
run ln -sfn themes "$BIN/theme"          # short alias
run ln -sfn themes "$BIN/themes-gui"     # launcher name

if [ "$DRY" = 1 ]; then
  echo "    + sed @BIN@ -> $BIN: data/$APP_ID.{service,desktop} -> $DATA/{dbus-1/services,applications}"
else
  sed "s|@BIN@|$BIN|g" "$SRC/data/$APP_ID.service" > "$DATA/dbus-1/services/$APP_ID.service"
  sed "s|@BIN@|$BIN|g" "$SRC/data/$APP_ID.desktop" > "$DATA/applications/$APP_ID.desktop"
fi
run install -m 0644 "$SRC/data/themes-doctor.service" "$HOME/.config/systemd/user/themes-doctor.service"

# GNOME Shell extensions that belong to this repository
for d in "$SRC"/extensions/*/; do
  uuid=$(basename "$d")
  run rm -rf "$DATA/gnome-shell/extensions/$uuid"
  run cp -a "${d%/}" "$DATA/gnome-shell/extensions/$uuid"
done

run systemctl --user daemon-reload
run systemctl --user enable themes-doctor.service
if command -v update-desktop-database >/dev/null 2>&1; then run update-desktop-database "$DATA/applications"; fi
echo "Installed: $BIN/themes (theme root: $ROOT)"
