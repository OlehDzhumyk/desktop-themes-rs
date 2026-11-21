#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# shellcheck disable=SC2034  # variables are consumed by the scripts that source this file
# Shared helpers for the scripts in this directory (source it, do not run it).
#
# Environment overrides:
#   DESKTOP_THEMES_ROOT  where themes/, baseline/ and state live (default: ~/.local/share/desktop-themes)
#   DT_TOOLBOX           name of the toolbox used for builds (default: desktop-themes-build)
#   DRY_RUN=1            print what would happen, change nothing (same as --dry-run)

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DATA_HOME=${XDG_DATA_HOME:-$HOME/.local/share}
CACHE_HOME=${XDG_CACHE_HOME:-$HOME/.cache}
DT_ROOT=${DESKTOP_THEMES_ROOT:-$DATA_HOME/desktop-themes}
DT_CACHE=$CACHE_HOME/desktop-themes
DT_TOOLBOX=${DT_TOOLBOX:-desktop-themes-build}
DRY_RUN=${DRY_RUN:-0}

# shellcheck source=scripts/versions.sh
. "$REPO/scripts/versions.sh"

say()  { printf '\n==> %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

# Run a command, or only print it in dry-run mode.
run() {
  if [ "$DRY_RUN" = 1 ]; then
    printf '    + '; printf '%q ' "$@"; printf '\n'
  else
    "$@"
  fi
}

# Parse flags shared by every script. Unknown flags are left in DT_ARGS.
DT_ARGS=()
parse_common_flags() {
  DT_ARGS=()
  for a in "$@"; do
    case "$a" in
      --dry-run|-n) DRY_RUN=1 ;;
      *) DT_ARGS+=("$a") ;;
    esac
  done
}

# mktmp PREFIX: a scratch directory under the cache (visible inside toolbox too); not created in dry-run.
mktmp() {
  local d=$DT_CACHE/tmp/$1.$$.$RANDOM
  [ "$DRY_RUN" = 1 ] || mkdir -p "$d"
  printf '%s\n' "$d"
}

in_container() { [ -f /run/.containerenv ] || [ -f /.dockerenv ]; }
have() { command -v "$1" >/dev/null 2>&1; }

# Run a build command inside the build toolbox when we are on the host (Silverblue has no
# compilers/sassc); run it directly when already inside a container or when toolbox is absent.
tb() {
  if in_container || ! have toolbox; then
    run "$@"
  else
    run toolbox run -c "$DT_TOOLBOX" "$@"
  fi
}

# Create the build toolbox on first use and install the packages the build steps need.
ensure_toolbox() {
  in_container && return 0
  have toolbox || { warn "toolbox not found; build steps run on the host and need the packages listed in docs/architecture.md"; return 0; }
  if ! toolbox list --containers 2>/dev/null | awk '{print $2}' | grep -qx "$DT_TOOLBOX"; then
    say "creating toolbox '$DT_TOOLBOX'"
    run toolbox create --assumeyes "$DT_TOOLBOX"
  fi
  # shellcheck disable=SC2086
  run toolbox run -c "$DT_TOOLBOX" sudo dnf install -y --setopt=install_weak_deps=False $DT_TOOLBOX_PACKAGES
}

# fetch_git NAME URL SHA  ->  checks out exactly SHA into $DT_CACHE/src/NAME
fetch_git() {
  local name=$1 url=$2 sha=$3 dir=$DT_CACHE/src/$1
  SRC=$dir
  if [ "$DRY_RUN" = 1 ]; then
    note "+ git fetch --depth 1 $url $sha -> $dir"
    return 0
  fi
  if [ -d "$dir/.git" ] && [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" = "$sha" ]; then
    note "$name already at ${sha:0:8}"
    return 0
  fi
  rm -rf "$dir"; mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" remote add origin "$url"
  git -C "$dir" fetch -q --depth 1 origin "$sha"
  git -C "$dir" checkout -q FETCH_HEAD
  [ "$(git -C "$dir" rev-parse HEAD)" = "$sha" ] || die "$name: checked out commit differs from the pinned $sha"
}

# fetch_file URL SHA256 DEST  (verifies the checksum, caches the download)
fetch_file() {
  local url=$1 sum=$2 dest=$3
  if [ "$DRY_RUN" = 1 ]; then note "+ curl $url -> $dest (sha256 ${sum:0:12}...)"; return 0; fi
  if [ -f "$dest" ] && [ "$(sha256sum "$dest" | cut -d' ' -f1)" = "$sum" ]; then return 0; fi
  mkdir -p "$(dirname "$dest")"
  curl -fsSL --retry 3 -o "$dest.part" "$url"
  [ "$(sha256sum "$dest.part" | cut -d' ' -f1)" = "$sum" ] || { rm -f "$dest.part"; die "checksum mismatch for $url"; }
  mv "$dest.part" "$dest"
}
