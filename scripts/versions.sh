#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# shellcheck disable=SC2034  # variables are consumed by the scripts that source this file
# Pinned upstream versions. Everything third-party is fetched from upstream at exactly these
# revisions; nothing from these projects is stored in this repository. Update deliberately and
# re-test (tools/headless, tools/integration-test.sh).

# Packages for the build toolbox (Fedora): Rust app + GTK theme builds + sound/wallpaper generators.
DT_TOOLBOX_PACKAGES="git curl unzip sassc glib2-devel gtk-update-icon-cache gcc pkgconf-pkg-config gtk4-devel libadwaita-devel rust cargo python3-numpy python3-scipy python3-soundfile python3-pillow"

# --- GTK / shell themes, icons, cursors, wallpapers (vinceliuice) ---
WHITESUR_GTK_URL=https://github.com/vinceliuice/WhiteSur-gtk-theme.git
WHITESUR_GTK_SHA=d5782652d412137e26fb8ff55b55a5572e4c6995
WHITESUR_ICONS_URL=https://github.com/vinceliuice/WhiteSur-icon-theme.git
WHITESUR_ICONS_SHA=73d8040da51a9ed74e47c7366e7e9ff437601a5c
WHITESUR_CURSORS_URL=https://github.com/vinceliuice/WhiteSur-cursors.git
WHITESUR_CURSORS_SHA=e190baf618ed95ee217d2fd45589bd309b37672b
WHITESUR_WALLPAPERS_URL=https://github.com/vinceliuice/WhiteSur-wallpapers.git
WHITESUR_WALLPAPERS_SHA=5c1d7ca20b8de0a7efe443792c19e49277262e02
FLUENT_GTK_URL=https://github.com/vinceliuice/Fluent-gtk-theme.git
FLUENT_GTK_SHA=7a49a464b0188c340101c52965c18190b1c694cf
FLUENT_ICONS_URL=https://github.com/vinceliuice/Fluent-icon-theme.git
FLUENT_ICONS_SHA=c8a244f571ef8fb47812bdc9dbce76546cedce69

# --- Fonts (release archives, verified by SHA-256) ---
INTER_URL=https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip
INTER_SHA256=9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e
JETBRAINS_MONO_URL=https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip
JETBRAINS_MONO_SHA256=6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf
# Large download (~150 MB), only the Cascadia Code/Mono variable TTFs are installed.
CASCADIA_URL=https://github.com/microsoft/cascadia-code/releases/download/v2407.24/CascadiaCode-2407.24.zip
CASCADIA_SHA256=e67a68ee3386db63f48b9054bd196ea752bc6a4ebb4df35adce6733da50c8474

# --- GNOME Shell extensions from extensions.gnome.org, pinned by version tag (shell 51) ---
# uuid  version-tag  (human version)
EGO_EXTENSIONS=(
  "user-theme@gnome-shell-extensions.gcampax.github.com 75038 v78"
  "appindicatorsupport@rgcjonas.gmail.com 75340 v66"
  "blur-my-shell@aunetx 78246 v78"
  "dash-to-dock@micxgx.gmail.com 76105 v110"
  "logomenu@aryan_k 75631 v45"
  "compiz-alike-magic-lamp-effect@hermes83.github.com 76122 v25"
  "spotlight@nin 75194 v15"
  "clipboard-indicator@tudmotu.com 78425 v74"
  "dash-to-panel@jderose9.github.com 75054 v74"
  "arcmenu@arcmenu.com 75483 v74"
  "tilingshell@ferrarodomenico.com 76684 v77"
  "gtk4-ding@smedius.gitlab.com 78597 v148"
)
# Not on extensions.gnome.org: installed from the upstream repository (subdirectory of the repo).
LIQUID_GLASS_URL=https://github.com/ryohsuke1231/liquid-glass.git
LIQUID_GLASS_SHA=9ec82ed088104ef8d2a3a2a5d9101272a68e84b1
LIQUID_GLASS_UUID=liquid-glass@thinkingcoding1231.gmail.com
