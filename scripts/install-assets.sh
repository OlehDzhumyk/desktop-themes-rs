#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fetch the third-party look-and-feel assets from upstream at pinned revisions and install them
# at user level (no sudo, no rpm-ostree). Nothing here is stored in the repository.
#
#   WhiteSur GTK + GNOME Shell theme, icons, cursors, wallpapers   (macos theme)
#   Fluent GTK + GNOME Shell theme, icons, cursors                  (windows11 theme)
#   Inter, JetBrains Mono, Cascadia Code/Mono fonts
#
# Also generates themes/<id>/gtk4/ (libadwaita CSS) inside the *installed* theme root,
# because those files are generated from the upstream themes and must not be committed.
#
# Usage: scripts/install-assets.sh [--dry-run] [--only whitesur|fluent|fonts]...
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_common_flags "$@"

ONLY=()
set -- "${DT_ARGS[@]+"${DT_ARGS[@]}"}"
while [ $# -gt 0 ]; do
  case "$1" in
    --only) ONLY+=("$2"); shift 2 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done
want() { [ ${#ONLY[@]} -eq 0 ] || printf '%s\n' "${ONLY[@]}" | grep -qx "$1"; }

THEMES_DIR=$DATA_HOME/themes
ICONS_DIR=$DATA_HOME/icons
FONTS_DIR=$DATA_HOME/fonts
BG_DIR=$DATA_HOME/backgrounds
run mkdir -p "$THEMES_DIR" "$ICONS_DIR" "$FONTS_DIR" "$BG_DIR"
[ "$DRY_RUN" = 1 ] || ensure_toolbox

install_whitesur() {
  say "WhiteSur GTK + shell theme (${WHITESUR_GTK_SHA:0:8})"
  fetch_git whitesur-gtk "$WHITESUR_GTK_URL" "$WHITESUR_GTK_SHA"; local gtk=$SRC
  # A throw-away HOME keeps the installer from touching ~/.config or ~/.local/bin.
  local fake; fake=$(mktmp whitesur)
  # the installer copies its helper app into $HOME/.local/{bin,share/applications}: give it a place
  [ "$DRY_RUN" = 1 ] || mkdir -p "$fake/.local/bin" "$fake/.local/share/applications" "$fake/.config"
  # -l additionally writes the libadwaita flavour of the CSS into $HOME/.config/gtk-4.0 (here: the
  # throw-away HOME), which is what the macos theme's gtk4/ directory consists of.
  tb env TERM="${TERM:-xterm}" HOME="$fake" bash -c "cd '$gtk' && ./install.sh -d '$THEMES_DIR' -c light -c dark -t default -o normal -l"
  # Light as gtk.css, Dark as gtk-dark.css (real files, not the script's absolute symlinks);
  # gtk-Light.css / gtk-Dark.css are kept as the originals; assets/ and windows-assets/ beside them.
  local dst=$DT_ROOT/themes/macos/gtk4 src=$fake/.config/gtk-4.0
  if [ "$DRY_RUN" = 1 ]; then
    note "+ assemble $dst from $src (-l output)"
  else
    rm -rf "$dst"; mkdir -p "$dst"
    cp -L "$src/gtk-Light.css" "$dst/gtk-Light.css"
    cp -L "$src/gtk-Dark.css"  "$dst/gtk-Dark.css"
    cp -L "$src/gtk-Light.css" "$dst/gtk.css"
    cp -L "$src/gtk-Dark.css"  "$dst/gtk-dark.css"
    cp -rL "$src/assets" "$dst/assets"
    [ -d "$src/windows-assets" ] && cp -rL "$src/windows-assets" "$dst/windows-assets"
    rm -rf "$fake"
  fi

  say "WhiteSur icons (${WHITESUR_ICONS_SHA:0:8})"
  fetch_git whitesur-icons "$WHITESUR_ICONS_URL" "$WHITESUR_ICONS_SHA"
  tb env TERM="${TERM:-xterm}" HOME="$(mktmp icons)" bash -c "cd '$SRC' && ./install.sh -d '$ICONS_DIR'"

  say "WhiteSur cursors (${WHITESUR_CURSORS_SHA:0:8})"
  fetch_git whitesur-cursors "$WHITESUR_CURSORS_URL" "$WHITESUR_CURSORS_SHA"
  run rm -rf "$ICONS_DIR/WhiteSur-cursors"
  run cp -r "$SRC/dist" "$ICONS_DIR/WhiteSur-cursors"

  say "WhiteSur wallpapers, 2k (${WHITESUR_WALLPAPERS_SHA:0:8})"
  fetch_git whitesur-wallpapers "$WHITESUR_WALLPAPERS_URL" "$WHITESUR_WALLPAPERS_SHA"
  run mkdir -p "$BG_DIR/macos"
  run cp "$SRC"/2k/*.jpg "$BG_DIR/macos/"
  run cp "$SRC/LICENSE" "$BG_DIR/macos/LICENSE-WhiteSur-wallpapers"
}

install_fluent() {
  say "Fluent GTK + shell theme, round tweak (${FLUENT_GTK_SHA:0:8})"
  fetch_git fluent-gtk "$FLUENT_GTK_URL" "$FLUENT_GTK_SHA"; local gtk=$SRC
  # blue accent, standard size, GNOME icon in the shell panel (the default would be a Windows logo)
  tb env TERM="${TERM:-xterm}" HOME="$(mktmp fluent)" bash -c \
    "cd '$gtk' && ./install.sh -d '$THEMES_DIR' -t default -s standard -i gnome --tweaks round"
  local dst=$DT_ROOT/themes/windows11/gtk4
  if [ "$DRY_RUN" = 1 ]; then
    note "+ assemble $dst from $THEMES_DIR/Fluent-round/gtk-4.0"
  else
    rm -rf "$dst"; mkdir -p "$dst"
    cp -L "$THEMES_DIR/Fluent-round/gtk-4.0/gtk.css" "$THEMES_DIR/Fluent-round/gtk-4.0/gtk-dark.css" "$dst/"
    cp -rL "$THEMES_DIR/Fluent-round/gtk-4.0/assets" "$dst/assets"
  fi

  say "Fluent icons and cursors (${FLUENT_ICONS_SHA:0:8})"
  fetch_git fluent-icons "$FLUENT_ICONS_URL" "$FLUENT_ICONS_SHA"; local ic=$SRC
  tb env TERM="${TERM:-xterm}" HOME="$(mktmp ficons)" bash -c "cd '$ic' && ./install.sh -d '$ICONS_DIR'"
  run rm -rf "$ICONS_DIR/Fluent-cursors" "$ICONS_DIR/Fluent-dark-cursors"
  run cp -r "$ic/cursors/dist" "$ICONS_DIR/Fluent-cursors"
  run cp -r "$ic/cursors/dist-dark" "$ICONS_DIR/Fluent-dark-cursors"
}

install_fonts() {
  local tmp="$DT_CACHE/fonts"
  say "Inter 4.1 (OFL-1.1)"
  fetch_file "$INTER_URL" "$INTER_SHA256" "$tmp/inter.zip"
  say "JetBrains Mono 2.304 (OFL-1.1)"
  fetch_file "$JETBRAINS_MONO_URL" "$JETBRAINS_MONO_SHA256" "$tmp/jetbrains-mono.zip"
  say "Cascadia Code v2407.24 (OFL-1.1), large download"
  fetch_file "$CASCADIA_URL" "$CASCADIA_SHA256" "$tmp/cascadia.zip"
  fetch_file "$CASCADIA_LICENSE_URL" "$CASCADIA_LICENSE_SHA256" "$tmp/cascadia-OFL.txt"
  if [ "$DRY_RUN" = 1 ]; then
    note "+ unzip fonts into $FONTS_DIR/{Inter,JetBrainsMono,CascadiaCode}; fc-cache"
    return 0
  fi
  local x; x=$(mktmp fonts)
  mkdir -p "$FONTS_DIR/Inter" "$FONTS_DIR/JetBrainsMono" "$FONTS_DIR/CascadiaCode"
  unzip -qo "$tmp/inter.zip" 'extras/ttf/Inter-*.ttf' LICENSE.txt -d "$x/inter"
  cp "$x"/inter/extras/ttf/Inter-*.ttf "$x/inter/LICENSE.txt" "$FONTS_DIR/Inter/"
  unzip -qo "$tmp/jetbrains-mono.zip" 'fonts/ttf/JetBrainsMono-*.ttf' OFL.txt -d "$x/jb"
  cp "$x"/jb/fonts/ttf/JetBrainsMono-*.ttf "$x/jb/OFL.txt" "$FONTS_DIR/JetBrainsMono/"
  unzip -qo "$tmp/cascadia.zip" 'ttf/CascadiaCode.ttf' 'ttf/CascadiaCodeItalic.ttf' 'ttf/CascadiaMono.ttf' 'ttf/CascadiaMonoItalic.ttf' -d "$x/casc"
  cp "$x"/casc/ttf/*.ttf "$FONTS_DIR/CascadiaCode/"
  cp "$tmp/cascadia-OFL.txt" "$FONTS_DIR/CascadiaCode/OFL.txt"
  rm -rf "$x"
  have fc-cache && fc-cache -f "$FONTS_DIR" || true
}

if want whitesur; then install_whitesur; fi
if want fluent;   then install_fluent;   fi
if want fonts;    then install_fonts;    fi
say "assets done"
