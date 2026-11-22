#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# install.sh of the release tarball: the same installation as scripts/bootstrap.sh, but with the
# prebuilt binaries (target/release/{themes,deskd}) and the built bridge (bridge/build/...) that the
# archive contains instead of building them. All bootstrap options work, for example:
#   ./install.sh --dry-run          read what it would do
#   ./install.sh --skip assets      skip the downloads of third-party themes/icons/fonts
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
exec scripts/bootstrap.sh --skip build "$@"
