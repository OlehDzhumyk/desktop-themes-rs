#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# User-level install of the built binaries and their desktop integration (no sudo):
#   themes (CLI/GUI/theme service), deskd (desktop features daemon), their D-Bus activation files,
#   systemd user units, the GSettings schema of deskd, the translations (po/ → ~/.local/share/locale,
#   translated .desktop and metainfo) and the GNOME Shell extensions of this repository (desk-bridge
#   is built from bridge/ with npm when bridge/build is missing).
#   Upgrading an install from before the language setting existed (Ukrainian-only builds) sets
#   io.github.OlehDzhumyk.Desk language='uk' once, so the interface stays Ukrainian on an English
#   session; new installs follow the session language (change it in the window: Features → General).
#   Also: desk-shell (when built; the Wayland taskbar of ADR 0012) and the Windows 11 labwc session
#   files (desk-win11-session in ~/.local/bin, data/labwc/windows11 → ~/.local/share/desktop-themes/
#   labwc/windows11; the session entry needs sudo, see docs/wayland-session.md).
# Run on the host after `cargo build --release` (scripts/bootstrap.sh does all of it).
#
# Usage: data/install.sh [--dry-run] [--bin PATH] [--deskd PATH] [--desk-shell PATH]
#   --bin PATH         install this binary instead of target/release/themes
#   --deskd PATH       install this binary instead of target/release/deskd
#   --desk-shell PATH  install this binary instead of target/release/desk-shell (optional: skipped
#                      when it does not exist)
set -euo pipefail
SRC=$(cd "$(dirname "$0")/.." && pwd)
BIN_SRC="$SRC/target/release/themes"
DESKD_SRC="$SRC/target/release/deskd"
SHELL_SRC="$SRC/target/release/desk-shell"
DRY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run|-n) DRY=1; shift ;;
    --bin) BIN_SRC=$2; shift 2 ;;
    --deskd) DESKD_SRC=$2; shift 2 ;;
    --desk-shell) SHELL_SRC=$2; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
run() { if [ "$DRY" = 1 ]; then printf '    + '; printf '%q ' "$@"; printf '\n'; else "$@"; fi; }

APP_ID=io.github.OlehDzhumyk.Themes
DESK_ID=io.github.OlehDzhumyk.Desk
BRIDGE=desk-bridge@desktop-themes-rs
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
BIN="$HOME/.local/bin"
ROOT="${DESKTOP_THEMES_ROOT:-$DATA/desktop-themes}"

# an existing install from before the language setting? (decided before the binary is replaced)
UPGRADE_LANG=0
if [ -x "$BIN/themes" ] && command -v dconf >/dev/null 2>&1 && [ -z "$(dconf read /io/github/OlehDzhumyk/Desk/language 2>/dev/null)" ] \
   && ! grep -q 'name="language"' "$DATA/glib-2.0/schemas/$DESK_ID.gschema.xml" 2>/dev/null; then
  UPGRADE_LANG=1
fi

[ "$DRY" = 1 ] || [ -x "$BIN_SRC" ] || { echo "binary not found: $BIN_SRC (build it first: cargo build --release)" >&2; exit 1; }
[ "$DRY" = 1 ] || [ -x "$DESKD_SRC" ] || { echo "binary not found: $DESKD_SRC (build it first: cargo build --release)" >&2; exit 1; }
run mkdir -p "$BIN" "$DATA/dbus-1/services" "$HOME/.config/systemd/user" "$DATA/applications" \
  "$DATA/gnome-shell/extensions" "$DATA/glib-2.0/schemas" "$DATA/metainfo"

run install -m 0755 "$BIN_SRC" "$BIN/themes.new"
run mv -f "$BIN/themes.new" "$BIN/themes"
run ln -sfn themes "$BIN/theme"          # short alias
run ln -sfn themes "$BIN/themes-gui"     # launcher name
run install -m 0755 "$DESKD_SRC" "$BIN/deskd.new"
run mv -f "$BIN/deskd.new" "$BIN/deskd"
# desk-shell is optional (not every build makes it); a running one keeps its old binary until the
# next login
SHELL_INSTALLED=0
if [ -x "$SHELL_SRC" ]; then
  run install -m 0755 "$SHELL_SRC" "$BIN/desk-shell.new"
  run mv -f "$BIN/desk-shell.new" "$BIN/desk-shell"
  SHELL_INSTALLED=1
fi

# Windows 11 session on labwc: the launcher and its bundled config (docs/wayland-session.md)
LABWC_DIR="$DATA/desktop-themes/labwc/windows11"
run install -m 0755 "$SRC/data/labwc/desk-win11-session" "$BIN/desk-win11-session"
run mkdir -p "$DATA/desktop-themes/labwc"
run rm -rf "${LABWC_DIR:?}"
run cp -a "$SRC/data/labwc/windows11" "$LABWC_DIR"

# GSettings schema shared by deskd, the bridge and the window
run install -m 0644 "$SRC/data/$DESK_ID.gschema.xml" "$DATA/glib-2.0/schemas/$DESK_ID.gschema.xml"
if command -v glib-compile-schemas >/dev/null 2>&1; then run glib-compile-schemas "$DATA/glib-2.0/schemas"
else run toolbox run -c "${DT_TOOLBOX:-desktop-themes-build}" glib-compile-schemas "$DATA/glib-2.0/schemas"; fi

if [ "$UPGRADE_LANG" = 1 ]; then
  run gsettings set "$DESK_ID" language uk
fi

# translations: one gettext domain for the programs, the bridge and the desktop files
msgfmt_() {
  if command -v msgfmt >/dev/null 2>&1; then run msgfmt "$@"
  else run toolbox run -c "${DT_TOOLBOX:-desktop-themes-build}" msgfmt "$@"; fi
}
for po in "$SRC"/po/*.po; do
  lang=$(basename "$po" .po)
  run mkdir -p "$DATA/locale/$lang/LC_MESSAGES"
  msgfmt_ --check -o "$DATA/locale/$lang/LC_MESSAGES/desktop-themes-rs.mo" "$po"
done
TMPD=$(mktemp -d)
trap 'rm -rf "$TMPD"' EXIT
msgfmt_ --desktop -d "$SRC/po" --template "$SRC/data/$APP_ID.desktop.in" -o "$TMPD/$APP_ID.desktop"
msgfmt_ --xml -d "$SRC/po" --template "$SRC/data/$APP_ID.metainfo.xml.in" -o "$TMPD/$APP_ID.metainfo.xml"

if [ "$DRY" = 1 ]; then
  echo "    + sed @BIN@ -> $BIN: data/$APP_ID.{service,desktop} -> $DATA/{dbus-1/services,applications}"
else
  sed "s|@BIN@|$BIN|g" "$SRC/data/$APP_ID.service" > "$DATA/dbus-1/services/$APP_ID.service"
  sed "s|@BIN@|$BIN|g" "$TMPD/$APP_ID.desktop" > "$DATA/applications/$APP_ID.desktop"
  sed "s|@BIN@|$BIN|g" "$SRC/data/$DESK_ID.service" > "$DATA/dbus-1/services/$DESK_ID.service"
  install -m 0644 "$TMPD/$APP_ID.metainfo.xml" "$DATA/metainfo/$APP_ID.metainfo.xml"
fi
run install -m 0644 "$SRC/data/themes-doctor.service" "$HOME/.config/systemd/user/themes-doctor.service"
run install -m 0644 "$SRC/data/deskd.service" "$HOME/.config/systemd/user/deskd.service"

# GNOME Shell extensions that belong to this repository (build/rebuild the TypeScript bridge if missing or stale)
BRIDGE_JS="$SRC/bridge/build/$BRIDGE/extension.js"
NEED_BRIDGE_BUILD=0
if [ ! -f "$BRIDGE_JS" ]; then
  NEED_BRIDGE_BUILD=1
elif [ -n "$(find "$SRC/bridge/src" "$SRC/bridge/static" "$SRC/bridge/package.json" -newer "$BRIDGE_JS" 2>/dev/null | head -n 1)" ]; then
  NEED_BRIDGE_BUILD=1
fi
if [ "$NEED_BRIDGE_BUILD" = 1 ]; then
  if command -v npm >/dev/null 2>&1; then
    # the path goes in as $1, never inside the script text (a quote in it cannot break out)
    run sh -c 'cd "$1" && ([ -d node_modules ] || npm ci --no-audit --no-fund) && npm run build' _ "$SRC/bridge"
  else
    run toolbox run -c "${DT_TOOLBOX:-desktop-themes-build}" \
      sh -c 'cd "$1" && ([ -d node_modules ] || npm ci --no-audit --no-fund) && npm run build' _ "$SRC/bridge"
  fi
fi
for d in "$SRC"/extensions/*/ "$SRC/bridge/build/$BRIDGE/"; do
  uuid=$(basename "$d")
  [ "$DRY" = 1 ] || [ -d "$d" ] || continue
  run rm -rf "${DATA:?}/gnome-shell/extensions/${uuid:?}"
  run cp -a "${d%/}" "$DATA/gnome-shell/extensions/$uuid"
  # Extensions with their own GSettings schema need schemas/gschemas.compiled (not in git).
  if ls "${d%/}"/schemas/*.gschema.xml >/dev/null 2>&1; then
    if command -v glib-compile-schemas >/dev/null 2>&1; then
      run glib-compile-schemas "$DATA/gnome-shell/extensions/$uuid/schemas"
    else
      run toolbox run -c "${DT_TOOLBOX:-desktop-themes-build}" glib-compile-schemas "$DATA/gnome-shell/extensions/$uuid/schemas"
    fi
  fi
done

run systemctl --user daemon-reload
run systemctl --user enable themes-doctor.service
if command -v update-desktop-database >/dev/null 2>&1; then run update-desktop-database "$DATA/applications"; fi
# a running deskd (older build) is replaced; the next D-Bus call activates the new one
run systemctl --user try-restart deskd.service
if [ "$SHELL_INSTALLED" = 1 ]; then
  echo "Installed: $BIN/themes, $BIN/deskd, $BIN/desk-shell, $BIN/desk-win11-session, $BRIDGE (theme root: $ROOT)"
else
  echo "Installed: $BIN/themes, $BIN/deskd, $BIN/desk-win11-session, $BRIDGE (theme root: $ROOT)"
  echo "Not installed: desk-shell (not built: $SHELL_SRC)"
fi
