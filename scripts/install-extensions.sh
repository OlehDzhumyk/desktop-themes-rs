#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Install the GNOME Shell extensions the themes use, at user level.
#   * 10 extensions from extensions.gnome.org, pinned by version tag and verified by SHA-256
#     (tested with GNOME Shell 51; versions.sh)
#   * spotlight@nin gets the unified diffs from patches/spotlight@nin applied (see patches/README.md)
#   * this repository's own extensions: desk-bridge (built from bridge/ with npm), lockscreen-layout
# Newly installed extensions appear in GNOME Shell only after you log out and in again.
#
# Usage: scripts/install-extensions.sh [--dry-run]
# On another GNOME version, pin new versions first: tools/ego-pin.sh UUID SHELL_MAJOR prints the
# line for versions.sh with its checksum. There is no "newest build" mode: it could not be verified.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_common_flags "$@"
for a in "${DT_ARGS[@]+"${DT_ARGS[@]}"}"; do
  case "$a" in
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) die "unknown option: $a" ;;
  esac
done

EXT_DIR=$DATA_HOME/gnome-shell/extensions
run mkdir -p "$EXT_DIR"
SHELL_MAJOR=51
if have gnome-shell; then
  SHELL_MAJOR=$(gnome-shell --version | sed -E 's/[^0-9]*([0-9]+).*/\1/')
fi
note "GNOME Shell major version: $SHELL_MAJOR (extensions are pinned for 51)"
[ "$SHELL_MAJOR" = 51 ] || warn "running shell is $SHELL_MAJOR, not 51: the pinned extensions may not support it (tools/ego-pin.sh)"

compile_schemas() { # DIR
  local d=$1
  if [ "$DRY_RUN" = 1 ]; then note "+ glib-compile-schemas $d/schemas"; return 0; fi
  if [ -d "$d/schemas" ] && ls "$d"/schemas/*.gschema.xml >/dev/null 2>&1; then
    if have glib-compile-schemas; then
      glib-compile-schemas "$d/schemas"
    elif ! in_container && have toolbox && toolbox run -c "$DT_TOOLBOX" command -v glib-compile-schemas >/dev/null 2>&1; then
      tb glib-compile-schemas "$d/schemas"
    else
      die "glib-compile-schemas not found (install glib2-devel on the host or in toolbox '$DT_TOOLBOX')"
    fi
  fi
}

install_ego() { # uuid tag label sha256
  local uuid=$1 tag=$2 label=$3 sum=$4 zip=$DT_CACHE/ego/$1.zip url
  say "$uuid ($label)"
  [ -n "$sum" ] || die "$uuid: no sha256 in scripts/versions.sh (tools/ego-pin.sh $uuid)"
  url="https://extensions.gnome.org/download-extension/$uuid.shell-extension.zip?version_tag=$tag"
  # fetch_file checks the SHA-256 of the download (and of a cached copy) before anything is unpacked
  fetch_file "$url" "$sum" "$zip"
  if [ "$DRY_RUN" = 1 ]; then
    note "+ unzip -> $EXT_DIR/$uuid"
  else
    rm -rf "${EXT_DIR:?}/$uuid"; mkdir -p "$EXT_DIR/$uuid"
    unzip -q "$zip" -d "$EXT_DIR/$uuid"
  fi
  if [ -d "$REPO/patches/$uuid" ]; then
    for p in "$REPO/patches/$uuid"/*.patch; do
      note "patch: $(basename "$p")"
      if [ "$DRY_RUN" = 1 ]; then note "+ (cd $EXT_DIR/$uuid && git apply -p1 $p)"
      elif (cd "$EXT_DIR/$uuid" && git apply --check -p1 "$p" 2>/dev/null); then (cd "$EXT_DIR/$uuid" && git apply -p1 "$p")
      else warn "patch $(basename "$p") does not apply to this build of $uuid (already fixed upstream?), skipped"; fi
    done
  fi
  [ "$DRY_RUN" = 1 ] || find "$EXT_DIR/$uuid" -name '*.orig' -delete
  compile_schemas "$EXT_DIR/$uuid"
}

for e in "${EGO_EXTENSIONS[@]}"; do
  # shellcheck disable=SC2086
  install_ego $e
done

BRIDGE_DIR="$REPO/bridge/build/desk-bridge@desktop-themes-rs"
if [ ! -f "$BRIDGE_DIR/extension.js" ]; then
  say "build desk-bridge (TypeScript)"
  tb sh -c "cd '$REPO/bridge' && npm ci --no-audit --no-fund && npm run build"
fi
for d in "$REPO"/extensions/*/ "$BRIDGE_DIR/"; do
  [ "$DRY_RUN" = 1 ] || [ -d "$d" ] || continue
  uuid=$(basename "$d")
  say "$uuid (this repository)"
  run rm -rf "${EXT_DIR:?}/$uuid"
  run cp -a "${d%/}" "$EXT_DIR/$uuid"
  compile_schemas "$EXT_DIR/$uuid"
done
say "extensions done; log out and in again to load newly installed ones"
