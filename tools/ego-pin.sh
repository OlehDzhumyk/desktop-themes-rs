#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Print the line for EGO_EXTENSIONS in scripts/versions.sh for the newest build of an extension
# that extensions.gnome.org offers for a shell version, with the SHA-256 of that build's ZIP.
#   tools/ego-pin.sh UUID [SHELL_MAJOR]      (SHELL_MAJOR defaults to 51)
# Review the extension's changes before you commit the line (it is code that runs in your shell).
set -euo pipefail
uuid=${1:?usage: tools/ego-pin.sh UUID [SHELL_MAJOR]}
shell=${2:-51}
info=$(curl -fsSL "https://extensions.gnome.org/extension-info/?uuid=$uuid&shell_version=$shell")
tag=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["download_url"].split("version_tag=")[1])' <<<"$info")
ver=$(python3 -c 'import json,sys; d=json.load(sys.stdin); print("v" + str(d.get("version", "?")))' <<<"$info")
zip=$(mktemp)
trap 'rm -f "$zip"' EXIT
curl -fsSL --retry 3 -o "$zip" "https://extensions.gnome.org/download-extension/$uuid.shell-extension.zip?version_tag=$tag"
printf '  "%s %s %s %s"\n' "$uuid" "$tag" "$ver" "$(sha256sum "$zip" | cut -d' ' -f1)"
