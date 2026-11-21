#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fresh Fedora Silverblue (GNOME) -> everything installed, at user level, no sudo, no rpm-ostree.
#
#   1. Flathub user remote (+ Extension Manager and Refine, used by the app's menu)
#   2. build toolbox (compilers, sassc, GTK/libadwaita headers, Python generators)
#   3. build the Rust workspace (themes, deskd) and the TypeScript shell bridge -- or use --prebuilt PATH
#   4. themes -> theme root;  upstream assets (WhiteSur, Fluent, icons, cursors, fonts, wallpapers)
#   5. generate the original sounds and wallpapers (CC0)
#   6. GNOME Shell extensions from extensions.gnome.org (+ patches) and this repo's own
#   7. install the binary, .desktop file, D-Bus service, systemd doctor unit
#   8. baseline snapshot of your current dconf (only if none exists), then `themes list`
#
# Usage: scripts/bootstrap.sh [options]
#   --dry-run, -n       print every step, change nothing
#   --prebuilt PATH     install this binary instead of building
#   --rustup            if the toolbox Rust is too old, install a toolchain with rustup
#   --no-flatpak        skip the Flathub remote and the two Flatpak apps
#   --skip STEP         skip a step: flatpak build themes assets generate extensions install baseline
#                       (repeatable)
#   -h, --help
#
# The baseline (step 8) is `dconf dump /` of the machine as it is NOW: the state "reset" returns to.
# Run bootstrap before you apply any theme, and keep that file private (it is your settings).
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

PREBUILT=""; RUSTUP=0; SKIP=" "
set -- "$@"
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run|-n) DRY_RUN=1; shift ;;
    --prebuilt) PREBUILT=$2; shift 2 ;;
    --rustup) RUSTUP=1; shift ;;
    --no-flatpak) SKIP="$SKIP""flatpak "; shift ;;
    --skip) SKIP="$SKIP$2 "; shift 2 ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
done
export DRY_RUN
step() { case "$SKIP" in *" $1 "*) note "skipping step: $1"; return 1 ;; esac; return 0; }
FWD=()
[ "$DRY_RUN" = 1 ] && FWD=(--dry-run)

say "preflight"
[ "$(id -u)" -ne 0 ] || die "do not run as root: everything is installed for your user"
[ "$DRY_RUN" = 1 ] && note "DRY RUN: nothing will be changed"
note "repository: $REPO"
note "theme root: $DT_ROOT"
if have rpm-ostree; then note "rpm-ostree system detected (Silverblue/Kinoite); nothing will be layered"; fi
have gnome-shell || warn "gnome-shell not found: this is meant for a GNOME desktop"
if [ "$DRY_RUN" != 1 ]; then
  for c in git curl unzip; do have "$c" || warn "$c missing on the host (it is installed in the toolbox; the host needs curl and unzip for assets)"; done
fi

if step flatpak; then
  say "Flathub (user) and apps"
  if have flatpak; then
    run flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
    run flatpak install --user --noninteractive --or-update flathub com.mattjakeman.ExtensionManager page.tesk.Refine
  else
    warn "flatpak not found, skipping"
  fi
fi

if step build; then
  say "build the application"
  if [ -n "$PREBUILT" ]; then
    note "using prebuilt binary: $PREBUILT"
  else
    ensure_toolbox
    tb bash -c '
      set -e
      export PATH="$HOME/.cargo/bin:$PATH"
      minor=$(rustc --version 2>/dev/null | sed -E "s/^rustc 1\.([0-9]+).*/\1/")
      if ! [ "${minor:-0}" -ge 88 ] 2>/dev/null; then
        if [ "$2" = 1 ]; then
          curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
          export PATH="$HOME/.cargo/bin:$PATH"
        else
          echo "error: Rust >= 1.88 is required (found: $(rustc --version 2>&1)). Re-run with --rustup." >&2; exit 1
        fi
      fi
      cd "$1" && cargo build --release --locked
      cd "$1/bridge" && npm ci --no-audit --no-fund && npm run build
    ' _ "$REPO" "$RUSTUP"
  fi
fi

if step themes; then say "themes"; "$REPO/scripts/install-themes.sh" "${FWD[@]+"${FWD[@]}"}"; fi
if step assets; then "$REPO/scripts/install-assets.sh" "${FWD[@]+"${FWD[@]}"}"; fi
if step generate; then "$REPO/scripts/generate-assets.sh" "${FWD[@]+"${FWD[@]}"}"; fi
if step extensions; then "$REPO/scripts/install-extensions.sh" "${FWD[@]+"${FWD[@]}"}"; fi

if step install; then
  say "install binary and desktop integration"
  if [ -n "$PREBUILT" ]; then
    "$REPO/data/install.sh" "${FWD[@]+"${FWD[@]}"}" --bin "$PREBUILT"
  else
    "$REPO/data/install.sh" "${FWD[@]+"${FWD[@]}"}"
  fi
fi

if step baseline; then
  say "baseline snapshot"
  if [ -e "$DT_ROOT/baseline/full.dconf" ] || [ -e "$DT_ROOT/state.json" ]; then
    note "a baseline or state already exists in $DT_ROOT; leaving it untouched"
  else
    b="$DT_ROOT/baseline-$(date +%Y%m%d-%H%M%S)"
    run mkdir -p "$b"
    if [ "$DRY_RUN" = 1 ]; then
      note "+ dconf dump / > $b/full.dconf"
    else
      dconf dump / > "$b/full.dconf"
    fi
    run ln -sfn "$(basename "$b")" "$DT_ROOT/baseline"
    if [ "$DRY_RUN" = 1 ]; then note "+ echo fedora > $DT_ROOT/current"; else echo fedora > "$DT_ROOT/current"; fi
  fi
  say "themes list"
  if [ "$DRY_RUN" = 1 ]; then
    note "+ ${HOME}/.local/bin/themes list"
  else
    "$HOME/.local/bin/themes" list
  fi
fi

say "done"
note "Log out and in once so GNOME Shell loads the newly installed extensions."
note "Try: themes apply macos --dry-run   |   themes apply macos   |   themes gui"
