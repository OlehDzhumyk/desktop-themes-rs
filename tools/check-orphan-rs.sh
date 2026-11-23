#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fail on tracked .rs files that no crate compiles (not reachable from any module tree).
# rustc lists every source file it read in its dep-info (.d) files, so no parser is needed:
# check all targets, collect the listed files, compare with `git ls-files '*.rs'`.
# Files behind a cfg that is off on this platform show up as orphans; list them in
# tools/orphan-rs-allowlist.txt (one path per line, '#' comments).
set -eu
cd "$(git rev-parse --show-toplevel)"

# Own target dir: other builds cannot leave stale dep-info behind. Workspace crates are
# cleaned first, so the dep-info is fresh; third-party dependencies stay cached.
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}/orphan-check
members=$(cargo metadata --format-version 1 --no-deps --locked \
  | python3 -c 'import json,sys; print(" ".join("-p " + p["name"] for p in json.load(sys.stdin)["packages"]))')
# shellcheck disable=SC2086
if [ -d "$CARGO_TARGET_DIR" ]; then cargo clean --quiet $members; fi
cargo check --workspace --all-targets --locked --quiet

used=$(mktemp) tracked=$(mktemp)
trap 'rm -f "$used" "$tracked"' EXIT
find "$CARGO_TARGET_DIR" -name '*.d' -print0 \
  | xargs -0 cat \
  | grep -oE '[^ :]+\.rs' \
  | sed "s|^$PWD/||" | grep -v '^/' | sort -u > "$used"
git ls-files '*.rs' | sort -u > "$tracked"

allow=tools/orphan-rs-allowlist.txt
orphans=$(comm -23 "$tracked" "$used")
if [ -f "$allow" ] && [ -n "$orphans" ]; then
  orphans=$(grep -vxFf <(grep -vE '^\s*(#|$)' "$allow") <<<"$orphans" || true)
fi
if [ -n "$orphans" ]; then
  echo "check-orphan-rs: .rs files not reachable from any crate's module tree:" >&2
  sed 's/^/  /' <<<"$orphans" >&2
  echo "Delete them, add the missing 'mod', or list platform-gated files in $allow." >&2
  exit 1
fi
echo "check-orphan-rs: ok ($(wc -l < "$tracked") files)"
