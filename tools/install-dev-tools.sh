#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Install pinned developer tools that Fedora does not package: cargo-nextest and ast-grep
# (x86_64 Linux). Used by CI, the Containerfile and humans. Checksums are verified.
#   tools/install-dev-tools.sh [DEST [nextest|ast-grep|all]]      DEST defaults to ~/.local/bin
set -eu
dest=${1:-$HOME/.local/bin}
which=${2:-all}
NEXTEST_VERSION=0.9.152
NEXTEST_SHA256=d039b43c3e0958441b6ad0a39c8884d018b3d2e8fb9928f209074b1100b5e17a
ASTGREP_VERSION=0.51.3
ASTGREP_SHA256=22803eb0c5bf83164fb7fd76d4721d355ecf0c8466ef47342c1ec7d80897ca47

[ "$(uname -s)-$(uname -m)" = Linux-x86_64 ] || { echo "install-dev-tools: only x86_64 Linux is pinned; install cargo-nextest and ast-grep yourself" >&2; exit 1; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
mkdir -p "$dest"

if [ "$which" = all ] || [ "$which" = nextest ]; then
  curl -fsSL "https://get.nexte.st/$NEXTEST_VERSION/linux" -o "$tmp/nextest.tgz"
  echo "$NEXTEST_SHA256  $tmp/nextest.tgz" | sha256sum -c - >/dev/null
  tar -xzf "$tmp/nextest.tgz" -C "$dest" cargo-nextest
  chmod +x "$dest/cargo-nextest"
fi
if [ "$which" = all ] || [ "$which" = ast-grep ]; then
  curl -fsSL "https://github.com/ast-grep/ast-grep/releases/download/$ASTGREP_VERSION/app-x86_64-unknown-linux-gnu.zip" -o "$tmp/ast-grep.zip"
  echo "$ASTGREP_SHA256  $tmp/ast-grep.zip" | sha256sum -c - >/dev/null
  # Only the `ast-grep` binary: the zip also holds `sg`, which would shadow the system `sg`.
  if command -v unzip >/dev/null 2>&1; then unzip -oq "$tmp/ast-grep.zip" ast-grep -d "$dest"
  else python3 -I -c 'import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extract("ast-grep", sys.argv[2])' "$tmp/ast-grep.zip" "$dest"; fi
  chmod +x "$dest/ast-grep"
fi
echo "installed $which (cargo-nextest $NEXTEST_VERSION, ast-grep $ASTGREP_VERSION) into $dest"
