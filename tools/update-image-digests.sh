#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Container images are pinned as name:tag@sha256:digest (CI containers, Containerfile, actionlint).
# This script looks up the digest each tag points to now and rewrites the pins in place.
#   tools/update-image-digests.sh           update the files
#   tools/update-image-digests.sh --check   only report; exit 1 when a pin is out of date
# Needs skopeo (Fedora: `dnf install skopeo`) and network access. Dependabot's docker ecosystem
# does the same for the files it understands; this covers the rest (workflow `container:`,
# `docker run` lines). Review the diff, then run `just check`.
set -eu
cd "$(git rev-parse --show-toplevel)"
check=0
[ "${1:-}" = "--check" ] && check=1
command -v skopeo >/dev/null || { echo "skopeo not found" >&2; exit 2; }

mapfile -t files < <(git ls-files '.github/workflows/*.yml' 'Containerfile' '*/Containerfile')
mapfile -t refs < <(grep -hoE '[a-z0-9][a-z0-9./_-]*:[A-Za-z0-9._-]+@sha256:[0-9a-f]{64}' "${files[@]}" | sort -u)
stale=0
for ref in "${refs[@]}"; do
  name=${ref%@*}
  old=${ref#*@}
  new=$(skopeo inspect --no-tags --format '{{.Digest}}' "docker://$name")
  if [ "$new" = "$old" ]; then
    echo "up to date  $name"
    continue
  fi
  stale=1
  echo "NEW DIGEST  $name"
  echo "            $old"
  echo "         -> $new"
  [ $check = 1 ] || sed -i "s|$name@$old|$name@$new|g" "${files[@]}"
done
[ $check = 0 ] || exit $stale
