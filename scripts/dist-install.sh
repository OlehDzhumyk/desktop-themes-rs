#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# install.sh of the release tarball: the same installation as scripts/bootstrap.sh, but with the
# prebuilt binaries (target/release/{themes,deskd,desk-shell}), the built bridge (bridge/build/...)
# and the Windows 11 labwc session files (data/labwc/) that the archive contains instead of
# building them. All bootstrap options work, for example:
#   ./install.sh --dry-run          read what it would do
#   ./install.sh --skip assets      skip the downloads of third-party themes/icons/fonts
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
for f in target/release/themes target/release/deskd target/release/desk-shell \
         data/labwc/desk-win11-session data/labwc/windows11/rc.xml; do
  [ -e "$f" ] || { echo "install.sh: $f is missing from the archive" >&2; exit 1; }
done
exec scripts/bootstrap.sh --skip build "$@"
