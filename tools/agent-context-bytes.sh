#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prints the total bytes of the agent context files: AGENTS.md, CLAUDE.md and .claude/**/*.md.
# Used by the agent-context-budget workflow and `just context-budget`.
set -eu
cd "$(git rev-parse --show-toplevel)"
total=0
while IFS= read -r -d '' f; do
  total=$((total + $(wc -c < "$f")))
done < <(find AGENTS.md CLAUDE.md .claude -type f \( -name '*.md' \) -print0 2>/dev/null | sort -z)
echo "$total"
