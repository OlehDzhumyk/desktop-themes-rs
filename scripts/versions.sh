#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# shellcheck disable=SC2034  # variables are consumed by the scripts that source this file
# Pinned upstream versions. Everything third-party is fetched from upstream at exactly these
# revisions; nothing from these projects is stored in this repository. Update deliberately and
# re-test (tools/headless, tools/integration-test.sh).

# Packages for the build toolbox (Fedora): Rust app + GTK theme builds + sound/wallpaper generators.
DT_TOOLBOX_PACKAGES="git curl unzip sassc glib2-devel gtk-update-icon-cache gcc pkgconf-pkg-config gtk4-devel libadwaita-devel pipewire-devel clang-devel rust cargo python3-numpy python3-scipy python3-soundfile python3-pillow nodejs npm gettext"

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
# uuid  version-tag  (human version)  sha256 of the downloaded ZIP (verified before it is unpacked).
# To update one: tools/ego-pin.sh UUID  prints the new line.
EGO_EXTENSIONS=(
  "user-theme@gnome-shell-extensions.gcampax.github.com 75038 v78 f5e8b7104f2385ac8f7df7fa151ced9d25ed84cfa80ea9dd56329f372001fa07"
  "appindicatorsupport@rgcjonas.gmail.com 75340 v66 94fe0ef32b84f184fee0ce2bb0edca7020a99e48210c43bbfc0c884a5c4660de"
  "blur-my-shell@aunetx 78246 v78 065d53b27dff3bbdd867deeab0316c030da7702c8c313c2a734d0eb05cf4f4a7"
  "dash-to-dock@micxgx.gmail.com 76105 v110 b05003e79835a35451e94931539ba1977f2a7a93b652a4fc6b7be60a77c18ae1"
  "logomenu@aryan_k 75631 v45 78e6507d9aa033a17e7e2997ac5a1190b327b97ba019a313f1a9285854753ed2"
  "compiz-alike-magic-lamp-effect@hermes83.github.com 76122 v25 a3a445a0f0329b581fec6edd2c33f143eec9b267cbb9b028bb6d7bff990cc420"
  "spotlight@nin 75194 v15 783862968f999b51eaa4fd10ca2e4747f309d64efa45db7fa580a884b2a214cd"
  "dash-to-panel@jderose9.github.com 75054 v74 8989c892f9795948e4490f47dc092218e42d771a6fec3dd44431cb48700ac2e5"
  "arcmenu@arcmenu.com 75483 v74 ef414c90dcb5f2b0c6ccb5f8e59fd9963c584789d3d8d26740ddb731100c4b40"
  "tilingshell@ferrarodomenico.com 76684 v77 17ab42f990d9b55f66009f50089ca172c62d206b03d0534ac5d9b63cf44e1cd1"
  "gtk4-ding@smedius.gitlab.com 78597 v148 c695a52a8719209daa2c9e83e510e36618dbe201c357a58d8b2d6d5a5aadfd22"
)
# Not on extensions.gnome.org: installed from the upstream repository (subdirectory of the repo).
LIQUID_GLASS_URL=https://github.com/ryohsuke1231/liquid-glass.git
LIQUID_GLASS_SHA=9ec82ed088104ef8d2a3a2a5d9101272a68e84b1
LIQUID_GLASS_UUID=liquid-glass@thinkingcoding1231.gmail.com

# --- rustup (only for `scripts/bootstrap.sh --rustup`): the rustup-init binary, verified by SHA-256 ---
RUSTUP_INIT_URL=https://static.rust-lang.org/rustup/archive/1.29.3/x86_64-unknown-linux-gnu/rustup-init
RUSTUP_INIT_SHA256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
