#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# The declared minimum Rust version must be one that is tested: `rust-version` in Cargo.toml has to
# equal the release pinned in rust-toolchain.toml (what CI and Fedora 45 build with). A lower
# number would be an untested promise (the GTK bindings alone need a recent compiler).
set -eu
cd "$(git rev-parse --show-toplevel)"
declared=$(sed -nE 's/^rust-version = "([0-9]+\.[0-9]+)(\.[0-9]+)?".*/\1/p' Cargo.toml)
pinned=$(sed -nE 's/^channel = "([0-9]+\.[0-9]+)(\.[0-9]+)?".*/\1/p' rust-toolchain.toml)
[ -n "$declared" ] && [ -n "$pinned" ] || { echo "check-rust-version: cannot read the versions" >&2; exit 2; }
if [ "$declared" != "$pinned" ]; then
  echo "check-rust-version: rust-version is $declared in Cargo.toml but rust-toolchain.toml pins $pinned." >&2
  echo "Bump them together (see docs/maintainer.md)." >&2
  exit 1
fi
echo "check-rust-version: ok ($declared)"
