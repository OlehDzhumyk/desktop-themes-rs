#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Print a theme's dconf as one `dconf load /` stream, following INHERITS like the manager does (a
# child's file replaces the parent's file of the same name), with ${HOME} expanded. The resolved
# extension list goes to $HS_STATE/theme.dconf.extensions.
#   theme-dconf.sh ID          (themes are read from $DESKTOP_THEMES_ROOT/themes, default
#                               ~/.local/share/desktop-themes/themes; THEMES_DIR overrides)
set -eu
H=$(cd "$(dirname "$0")" && pwd); . "$H/env.sh"
ROOT="${THEMES_DIR:-${DESKTOP_THEMES_ROOT:-$HOME/.local/share/desktop-themes}/themes}"
id=$1
chain=()
while [ -n "$id" ]; do
  [ -d "$ROOT/$id" ] || { echo "no theme $id in $ROOT" >&2; exit 1; }
  chain+=("$ROOT/$id")
  id=$(sed -n "s/^INHERITS=[\"']\{0,1\}\([A-Za-z0-9._-]*\).*/\1/p" "$ROOT/$id/theme.conf" 2>/dev/null | head -1)
  [ ${#chain[@]} -lt 8 ] || break
done
declare -A seen
for d in "${chain[@]}"; do
  for f in "$d"/*.dconf; do
    [ -f "$f" ] || continue
    n=$(basename "$f")
    [ -n "${seen[$n]+x}" ] && continue
    seen[$n]=1
    # sections with the Ptyxis profile placeholder are skipped (no Ptyxis profile in the test db)
    awk '/^\[/{skip = index($0, "@DEFAULT_PTYXIS_PROFILE@") > 0} !skip' "$f" |
      sed "s|\${HOME}|$HOME|g; s|\$HOME|$HOME|g"; echo
  done
done
# extensions: the root-most theme first, then the +/- lines of each child
exts=()
for (( i=${#chain[@]}-1; i>=0; i-- )); do
  f="${chain[$i]}/extensions.txt"; [ -f "$f" ] || continue
  if grep -q '^[[:space:]]*[^+#[:space:]-]' "$f"; then exts=(); fi
  while read -r l; do
    l=${l%%#*}; l=$(printf '%s' "$l" | tr -d '[:space:]'); [ -n "$l" ] || continue
    case "$l" in
      -*) u=${l#-}; n=(); for x in "${exts[@]+"${exts[@]}"}"; do [ "$x" = "$u" ] || n+=("$x"); done
          exts=("${n[@]+"${n[@]}"}") ;;
      *) u=${l#+}; case " ${exts[*]-} " in *" $u "*) ;; *) exts+=("$u") ;; esac ;;
    esac
  done < "$f"
done
printf '%s\n' "${exts[@]+"${exts[@]}"}" > "$HS_STATE/theme.dconf.extensions"
