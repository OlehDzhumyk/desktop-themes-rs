#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Install the GNOME Shell extensions the themes use, at user level.
#   * 12 extensions from extensions.gnome.org, pinned by version tag (tested with GNOME Shell 51)
#   * spotlight@nin gets the unified diffs from patches/spotlight@nin applied (see patches/README.md)
#   * Liquid Glass from its upstream repository at a pinned commit (it is not on extensions.gnome.org)
#   * this repository's own extensions (themes-quick, lockscreen-layout)
# Newly installed extensions appear in GNOME Shell only after you log out and in again.
#
# Usage: scripts/install-extensions.sh [--dry-run] [--latest]
#   --latest  ignore the pinned version tags and ask extensions.gnome.org for the newest build
#             that supports the running shell (useful on a GNOME version other than 51)
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_common_flags "$@"
LATEST=0
for a in "${DT_ARGS[@]+"${DT_ARGS[@]}"}"; do
  case "$a" in
    --latest) LATEST=1 ;;
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
[ "$SHELL_MAJOR" = 51 ] || [ "$LATEST" = 1 ] || warn "running shell is $SHELL_MAJOR, not 51: consider --latest"

compile_schemas() { # DIR
  local d=$1
  if [ "$DRY_RUN" = 1 ]; then note "+ glib-compile-schemas $d/schemas"; return 0; fi
  if [ -d "$d/schemas" ] && ls "$d"/schemas/*.gschema.xml >/dev/null 2>&1; then
    if have glib-compile-schemas; then glib-compile-schemas "$d/schemas"
    else tb glib-compile-schemas "$d/schemas"; fi
  fi
}

install_ego() { # uuid tag label
  local uuid=$1 tag=$2 label=$3 zip=$DT_CACHE/ego/$1.zip url
  say "$uuid ($label)"
  if [ "$LATEST" = 1 ]; then
    if [ "$DRY_RUN" = 1 ]; then note "+ query extension-info for $uuid (shell $SHELL_MAJOR)"; tag=latest
    else
      tag=$(curl -fsSL "https://extensions.gnome.org/extension-info/?uuid=$uuid&shell_version=$SHELL_MAJOR" \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["download_url"].split("version_tag=")[1])') \
        || { warn "$uuid is not available for shell $SHELL_MAJOR, skipped"; return 0; }
    fi
  fi
  url="https://extensions.gnome.org/download-extension/$uuid.shell-extension.zip?version_tag=$tag"
  if [ "$DRY_RUN" = 1 ]; then
    note "+ curl $url"
    note "+ unzip -> $EXT_DIR/$uuid"
  else
    mkdir -p "$(dirname "$zip")"
    curl -fsSL --retry 3 -o "$zip" "$url"
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
  compile_schemas "$EXT_DIR/$uuid"
}

for e in "${EGO_EXTENSIONS[@]}"; do
  # shellcheck disable=SC2086
  install_ego $e
done

say "$LIQUID_GLASS_UUID (upstream repository, ${LIQUID_GLASS_SHA:0:8}, MIT)"
fetch_git liquid-glass "$LIQUID_GLASS_URL" "$LIQUID_GLASS_SHA"
if [ "$DRY_RUN" = 1 ]; then
  note "+ copy $SRC/$LIQUID_GLASS_UUID -> $EXT_DIR/$LIQUID_GLASS_UUID"
else
  rm -rf "${EXT_DIR:?}/$LIQUID_GLASS_UUID"
  cp -a "$SRC/$LIQUID_GLASS_UUID" "$EXT_DIR/$LIQUID_GLASS_UUID"
fi
compile_schemas "$EXT_DIR/$LIQUID_GLASS_UUID"

for d in "$REPO"/extensions/*/; do
  uuid=$(basename "$d")
  say "$uuid (this repository)"
  run rm -rf "${EXT_DIR:?}/$uuid"
  run cp -a "${d%/}" "$EXT_DIR/$uuid"
  compile_schemas "$EXT_DIR/$uuid"
done
say "extensions done; log out and in again to load newly installed ones"
