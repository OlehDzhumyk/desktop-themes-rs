#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Run a command inside the isolated headless environment: tools/headless/run.sh dconf dump /
H=$(cd "$(dirname "$0")" && pwd); . "$H/env.sh"
while IFS= read -r kv; do export "${kv?}"; done < <(hs_env)
export WAYLAND_DISPLAY="$HS_WAYLAND"
unset DISPLAY
exec "$@"
