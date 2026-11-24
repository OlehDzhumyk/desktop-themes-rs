#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Run the Wayland end-to-end tests (tests/e2e-wayland): headless labwc, deskd and desk-shell in a
# throw-away container without systemd (plain podman or Docker). Only the built binaries, data/
# and the tests are mounted, read-only; nothing of the host session is reachable from it.
#
#   tools/e2e-wayland.sh [--no-build] [--out DIR] [PYTEST ARGS...]   e.g. tools/e2e-wayland.sh -k toggle
#   --no-build   use target/debug as it is (default: build deskd and desk-shell first)
#   --out DIR    logs of labwc, deskd and desk-shell (default: target/e2e-wayland)
# Environment: E2E_ENGINE (podman or docker, default: podman when installed), E2E_WAYLAND_IMAGE
# (image name), E2E_TOOLBOX (toolbox to build in, default themes-rs-build; "none" builds on the
# host, as in CI).
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
OUT="$REPO/target/e2e-wayland" BUILD=1
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0; shift ;;
    --out) OUT=$2; shift 2 ;;
    -h|--help) sed -n '3,12p' "$0"; exit 0 ;;
    *) break ;;
  esac
done
TBX=${E2E_TOOLBOX:-themes-rs-build}
in_build_env() { if [ "$TBX" = none ]; then "$@"; else toolbox run -c "$TBX" "$@"; fi; }
if [ "$BUILD" = 1 ]; then
  in_build_env bash -c "cd '$REPO' && PATH=\$HOME/.cargo/bin:\$PATH cargo build --locked -p deskd -p desk-shell"
fi
BIN="$REPO/target/debug"
for b in deskd desk-shell; do
  [ -x "$BIN/$b" ] || { echo "e2e-wayland: $BIN/$b is not built (cargo build -p deskd -p desk-shell)" >&2; exit 1; }
done
ENGINE=${E2E_ENGINE:-}
if [ -z "$ENGINE" ]; then
  if command -v podman >/dev/null 2>&1; then ENGINE=podman; else ENGINE=docker; fi
fi
# the image is tagged with a hash of its Containerfile, so a change rebuilds it
TAG=$(sha256sum "$REPO/tests/e2e-wayland/Containerfile" | cut -c1-12)
IMAGE=${E2E_WAYLAND_IMAGE:-localhost/desktop-themes-e2e-wayland:$TAG}
if ! "$ENGINE" image inspect "$IMAGE" >/dev/null 2>&1; then
  "$ENGINE" build -t "$IMAGE" -f "$REPO/tests/e2e-wayland/Containerfile" "$REPO/tests/e2e-wayland"
fi
mkdir -p "$OUT"
find "$OUT" -mindepth 1 -maxdepth 1 -name '*.log' -delete
mounts=(-v "$REPO/tests/e2e-wayland:/src/tests/e2e-wayland:ro" -v "$REPO/data:/src/data:ro"
        -v "$OUT:/out")
for b in deskd desk-shell themes; do
  [ -e "$BIN/$b" ] && mounts+=(-v "$BIN/$b:/src/target/debug/$b:ro")
done
# The container runs as root (dbus-daemon needs a passwd entry). Rootless podman maps that root
# to the caller; under Docker the logs are given back to the caller at the end.
owner=
[ "$ENGINE" = docker ] && owner="$(id -u):$(id -g)"
# shellcheck disable=SC2016 # the inner script is expanded by the container's sh
exec "$ENGINE" run --rm --init --security-opt label=disable "${mounts[@]}" \
  -w /src/tests/e2e-wayland -e PYTHONDONTWRITEBYTECODE=1 -e E2E_OUT=/out \
  -e E2E_KEEP_WORK="${E2E_KEEP_WORK:-}" -e OUT_OWNER="$owner" "$IMAGE" \
  sh -c 'rc=0; python3 -m pytest "$@" || rc=$?; [ -z "$OUT_OWNER" ] || chown -R "$OUT_OWNER" /out; exit $rc' \
  e2e-wayland "$@"
