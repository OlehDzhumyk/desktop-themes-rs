#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Generate the original (CC0) assets instead of storing them in git:
#   * UI sound themes macos-like and windows11-like (tools/synth-sounds.py, ~6 s)
#   * Windows 11 "bloom" wallpapers, light and dark (tools/generate-bloom-wallpaper.py, ~7 s each)
# Usage: scripts/generate-assets.sh [--dry-run]
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_common_flags "$@"
[ "$DRY_RUN" = 1 ] || ensure_toolbox

say "sound themes -> $DATA_HOME/sounds"
run mkdir -p "$DATA_HOME/sounds"
# the generator prints loudness statistics as JSON; keep the terminal quiet
if [ "$DRY_RUN" = 1 ]; then
  note "+ python3 tools/synth-sounds.py $DATA_HOME/sounds"
else
  tb python3 "$REPO/tools/synth-sounds.py" "$DATA_HOME/sounds" >/dev/null
fi

say "wallpapers -> $DATA_HOME/backgrounds/windows11"
run mkdir -p "$DATA_HOME/backgrounds/windows11"
for v in light dark; do
  tb python3 "$REPO/tools/generate-bloom-wallpaper.py" "$v" "$DATA_HOME/backgrounds/windows11/bloom-$v.png"
done
