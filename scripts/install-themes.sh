#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copy the themes from this repository into the theme root ($DESKTOP_THEMES_ROOT/themes).
# Bundled themes are synchronised: a *.dconf file or a symlink that no longer exists in the
# repository (old clipboard.dconf, the macos-glass alias links, sync-glass.sh) would still be read by
# the manager, so it is moved to $DESKTOP_THEMES_ROOT/trash/themes-<date>/<id>/. Other files you
# keep there (README, tools/, *.bak) and the generated gtk4/ and gtk3/ stay.
# Themes with other ids (your own snapshots, private themes) are never touched.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_common_flags "$@"

STAMP=$(date +%Y%m%d-%H%M%S)
say "installing themes into $DT_ROOT/themes"
run mkdir -p "$DT_ROOT/themes"
for d in "$REPO"/themes/*/; do
  id=$(basename "$d")
  dest="$DT_ROOT/themes/$id"
  note "theme: $id"
  run mkdir -p "$dest"
  if [ -d "$dest" ]; then
    for e in "$dest"/*; do
      [ -e "$e" ] || [ -L "$e" ] || continue
      n=$(basename "$e")
      [ -e "$d$n" ] && [ ! -L "$e" ] && continue          # still in the repository
      [ -L "$d$n" ] && [ -L "$e" ] && continue
      case "$n" in
        *.dconf|sync-glass.sh) ;;                         # would still be applied / is obsolete
        *) [ -L "$e" ] || continue ;;                     # other own files stay; old alias links go
      esac
      trash="$DT_ROOT/trash/themes-$STAMP/$id"
      note "  stale $n -> ${trash#"$DT_ROOT"/}/"
      run mkdir -p "$trash"
      run mv -- "$e" "$trash/"
    done
  fi
  run cp -a "$d." "$dest/"
done
