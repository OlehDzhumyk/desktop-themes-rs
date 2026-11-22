#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Run the end-to-end tests (tests/e2e) in a throw-away podman container with systemd and
# GNOME Shell 51 (ADR 0007). The repository is mounted read-only; nothing of the host session is
# reachable from the container (no host bus, runtime dir or home is mounted).
#
#   tools/e2e.sh [--no-build] [--out DIR] [PYTEST ARGS...]     e.g. tools/e2e.sh -k clipboard
#   --no-build   use target/debug and bridge/build as they are (default: build them first)
#   --out DIR    screenshots, shell logs and the container journal (default: target/e2e)
# Environment: E2E_IMAGE (image name), E2E_TOOLBOX (toolbox to build in, default themes-rs-build;
# "none" builds on the host, as in CI).
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
OUT="$REPO/target/e2e" BUILD=1
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0; shift ;;
    --out) OUT=$2; shift 2 ;;
    -h|--help) sed -n '3,11p' "$0"; exit 0 ;;
    *) break ;;
  esac
done
TBX=${E2E_TOOLBOX:-themes-rs-build}
in_build_env() { if [ "$TBX" = none ]; then "$@"; else toolbox run -c "$TBX" "$@"; fi; }
if [ "$BUILD" = 1 ]; then
  in_build_env bash -c "cd '$REPO' && PATH=\$HOME/.cargo/bin:\$PATH cargo build --locked -p deskd -p themes \
    && cd bridge && { [ -d node_modules ] || npm ci --no-audit --no-fund; } && npm run build"
fi
# the image is tagged with a hash of its Containerfile, so a change rebuilds it
TAG=$(sha256sum "$REPO/tests/e2e/Containerfile" | cut -c1-12)
IMAGE=${E2E_IMAGE:-localhost/desktop-themes-e2e:$TAG}
if ! podman image exists "$IMAGE"; then
  podman build -t "$IMAGE" -f "$REPO/tests/e2e/Containerfile" "$REPO/tests/e2e"
fi
mkdir -p "$OUT"
find "$OUT" -mindepth 1 -maxdepth 1 \( -name '*.png' -o -name '*.log' -o -name 'journal.txt' -o -name 'processes.txt' \) -delete
CID=$(podman run -d --rm --systemd=always --security-opt label=disable --pids-limit=-1 \
  -v "$REPO:/src:ro" -v "$OUT:/out" "$IMAGE" /sbin/init)
# only the container this script created is removed
trap 'podman rm -f -t 5 "$CID" >/dev/null 2>&1 || true' EXIT
podman exec "$CID" systemctl is-system-running --wait >/dev/null || true
rc=0
podman exec -w /src/tests/e2e -e PYTHONDONTWRITEBYTECODE=1 -e E2E_OUT=/out \
  -e E2E_KEEP_LOGS="${E2E_KEEP_LOGS:-}" -e GITHUB_ACTIONS="${GITHUB_ACTIONS:-}" "$CID" python3 -m pytest "$@" || rc=$?
podman exec "$CID" ps -eo pid,ppid,nlwp,args > "$OUT/processes.txt" 2>&1 || true
podman exec "$CID" journalctl -b --no-pager -o short-monotonic > "$OUT/journal.txt" 2>&1 || true
exit $rc
