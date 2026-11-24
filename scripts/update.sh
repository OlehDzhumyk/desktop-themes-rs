#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fast incremental update for an existing desktop-themes-rs installation on Fedora (no sudo).
#
#   1. `git pull --ff-only` (unless --no-pull) and check whether scripts/versions.sh changed
#   2. build Rust release binaries (themes, deskd) and the TypeScript bridge in the build toolbox
#      (skips dnf install when the toolbox already exists; runs `npm ci` only when lockfile changed)
#   3. synchronise bundled themes (`scripts/install-themes.sh`)
#   4. optionally refresh third-party assets/extensions (automatically when versions.sh changed,
#      or when --with-assets / --with-extensions / --all is passed)
#   5. install binaries, GSettings schemas, translations, D-Bus/systemd units and own extensions
#      (`data/install.sh`) and run `themes doctor`
#
# Usage: scripts/update.sh [options]
#   --dry-run, -n       print every step, change nothing
#   --no-pull           skip `git pull --ff-only` (build and install the current checkout)
#   --with-assets       also re-run install-assets.sh and generate-assets.sh
#   --with-extensions   also re-run install-extensions.sh
#   --all               same as --with-assets --with-extensions
#   --refresh-toolbox   re-run `dnf install` for toolbox packages before building
#   --rustup            if the toolbox Rust is too old, install a toolchain with rustup
#   -h, --help
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

NO_PULL=0
WITH_ASSETS=0
WITH_EXTENSIONS=0
REFRESH_TOOLBOX=0
RUSTUP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run|-n) DRY_RUN=1; shift ;;
    --no-pull) NO_PULL=1; shift ;;
    --with-assets) WITH_ASSETS=1; shift ;;
    --with-extensions) WITH_EXTENSIONS=1; shift ;;
    --all) WITH_ASSETS=1; WITH_EXTENSIONS=1; shift ;;
    --refresh-toolbox) REFRESH_TOOLBOX=1; shift ;;
    --rustup) RUSTUP=1; shift ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
done
export DRY_RUN

FWD=()
[ "$DRY_RUN" = 1 ] && FWD=(--dry-run)

say "preflight"
[ "$(id -u)" -ne 0 ] || die "do not run as root: everything is installed for your user"
[ "$DRY_RUN" = 1 ] && note "DRY RUN: nothing will be changed"
note "repository: $REPO"
note "theme root: $DT_ROOT"

VERSIONS_BEFORE=""
if [ -f "$REPO/scripts/versions.sh" ] && have sha256sum; then
  VERSIONS_BEFORE=$(sha256sum "$REPO/scripts/versions.sh" | cut -d' ' -f1)
fi

if [ "$NO_PULL" = 0 ]; then
  say "pull latest changes"
  if [ "$DRY_RUN" = 0 ] && [ -n "$(git -C "$REPO" status --porcelain)" ]; then
    die "working tree has local changes; commit/stash them or re-run with --no-pull"
  fi
  run git -C "$REPO" pull --ff-only
fi

if [ -n "$VERSIONS_BEFORE" ] && [ -f "$REPO/scripts/versions.sh" ] && have sha256sum; then
  VERSIONS_AFTER=$(sha256sum "$REPO/scripts/versions.sh" | cut -d' ' -f1)
  if [ "$VERSIONS_BEFORE" != "$VERSIONS_AFTER" ]; then
    note "scripts/versions.sh changed: refreshing toolbox, assets and extensions"
    REFRESH_TOOLBOX=1
    WITH_ASSETS=1
    WITH_EXTENSIONS=1
  fi
fi

# Re-source versions.sh in case git pull updated pinned revisions.
# shellcheck source=scripts/versions.sh
. "$REPO/scripts/versions.sh"

say "build workspace and bridge"
if ! in_container && have toolbox; then
  if [ "$REFRESH_TOOLBOX" = 1 ] || ! toolbox list --containers 2>/dev/null | awk '{print $2}' | grep -qx "$DT_TOOLBOX"; then
    ensure_toolbox
  else
    note "reusing existing toolbox '$DT_TOOLBOX' (pass --refresh-toolbox to update dnf packages)"
  fi
fi

tb bash -c '
  set -euo pipefail
  export PATH="$HOME/.cargo/bin:$PATH"
  repo=$1 rustup_flag=$2 rustup_url=$3 rustup_sha=$4
  minor=$(rustc --version 2>/dev/null | sed -E "s/^rustc 1\.([0-9]+).*/\1/" || true)
  want=$(sed -nE "s/^rust-version = \"1\.([0-9]+)\".*/\1/p" "$repo/Cargo.toml")
  if ! [ "${minor:-0}" -ge "${want:-0}" ] 2>/dev/null; then
    if [ "$rustup_flag" = 1 ]; then
      init=$(mktemp)
      curl -fsSL --retry 3 -o "$init" "$rustup_url"
      echo "$rustup_sha  $init" | sha256sum --check --status || { echo "error: rustup-init checksum mismatch" >&2; rm -f "$init"; exit 1; }
      chmod +x "$init" && "$init" -y --profile minimal
      rm -f "$init"
      export PATH="$HOME/.cargo/bin:$PATH"
    else
      echo "error: Rust >= 1.$want is required (found: $(rustc --version 2>&1 || echo none)). Re-run with --rustup." >&2
      exit 1
    fi
  fi
  cd "$repo"
  cargo build --release --locked
  cd "$repo/bridge"
  if [ ! -d node_modules ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
    npm ci --no-audit --no-fund
  fi
  npm run build
' _ "$REPO" "$RUSTUP" "$RUSTUP_INIT_URL" "$RUSTUP_INIT_SHA256"

say "update bundled themes"
"$REPO/scripts/install-themes.sh" "${FWD[@]+"${FWD[@]}"}"

if [ "$WITH_ASSETS" = 1 ]; then
  "$REPO/scripts/install-assets.sh" "${FWD[@]+"${FWD[@]}"}"
  "$REPO/scripts/generate-assets.sh" "${FWD[@]+"${FWD[@]}"}"
fi

if [ "$WITH_EXTENSIONS" = 1 ]; then
  "$REPO/scripts/install-extensions.sh" "${FWD[@]+"${FWD[@]}"}"
fi

say "install binaries, schemas, translations and extensions"
"$REPO/data/install.sh" "${FWD[@]+"${FWD[@]}"}"

say "verify installation"
if [ "$DRY_RUN" = 1 ]; then
  note "+ ${HOME}/.local/bin/themes doctor"
else
  "$HOME/.local/bin/themes" doctor || warn "themes doctor reported findings above"
fi

say "done"
note "deskd has been restarted automatically. If desk-bridge or lockscreen-layout changed,"
note "log out and log back in once so GNOME Shell reloads the updated extension JavaScript."
