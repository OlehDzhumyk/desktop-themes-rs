#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Claude Code PreToolUse hook for Bash. Exit 2 (stderr is shown to the agent) blocks a command;
# exit 0 allows everything else. A seatbelt for the mechanical cases of AGENTS.md "Ways to hurt
# yourself", not a security boundary. Blocks:
#   * pkill / killall / pgrep|kill (kill by pattern)
#   * dconf write|reset|load, gsettings set|reset, `themes apply|reset` unless the command is
#     visibly isolated (tools/headless/run.sh, GSETTINGS_BACKEND=memory, or DCONF_PROFILE /
#     XDG_CONFIG_HOME pointing into a temp dir); `themes apply --dry-run` is fine
#   * git push (agents never push)
#   * `themes apply <experimental theme>` without --yes (ADR 0006)
# Needs jq or python3 to read the hook JSON; without either it warns and allows.
set -u

input=$(cat)
if command -v jq >/dev/null 2>&1; then
  cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty' 2>/dev/null)
elif command -v python3 >/dev/null 2>&1; then
  cmd=$(printf '%s' "$input" | python3 -I -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("command",""))' 2>/dev/null)
else
  echo "guard.sh: neither jq nor python3 found; command not checked" >&2
  exit 0
fi
[ -n "$cmd" ] || exit 0

block() { printf 'BLOCKED by .claude/hooks/guard.sh: %s\n' "$1" >&2; exit 2; }

# A command word: after start, a separator, an opening quote or $( and optional sudo/env/VAR=x.
sep='(^|[;&|(`"'"'"']|\$\()[[:space:]]*'
pre='((sudo|env|command|exec|time)[[:space:]]+|[A-Za-z_][A-Za-z0-9_]*=[^[:space:]]*[[:space:]]+)*'
cp="$sep$pre"

# 1. kill by pattern
re_kill="${cp}(pkill|killall)([[:space:]]|\$)"
re_pgrep_pipe='pgrep[^|]*[|][^|]*kill'
re_kill_subst='kill[[:space:]][^|;]*[$][(][[:space:]]*pgrep'
if [[ $cmd =~ $re_kill ]] || [[ $cmd =~ $re_pgrep_pipe ]] || [[ $cmd =~ $re_kill_subst ]]; then
  block "never kill by pattern (pkill, killall, pgrep|kill). Kill only a PID you spawned and recorded."
fi

# isolation markers
isolated=0
tmpdirs='(/tmp/|/var/tmp/|[$]TMPDIR|[$][{]TMPDIR|[$][(]mktemp|headless)'
re_iso="(DCONF_PROFILE|XDG_CONFIG_HOME)=[^[:space:]]*$tmpdirs"
if [[ $cmd == *tools/headless/run.sh* ]] || [[ $cmd == *GSETTINGS_BACKEND=memory* ]] || [[ $cmd =~ $re_iso ]]; then
  isolated=1
fi

# 2. live session writes
if [ $isolated = 0 ]; then
  re_dconf="${cp}dconf[[:space:]]+(write|reset|load)"
  re_gset="${cp}gsettings[[:space:]]+(set|reset|reset-recursively)"
  re_themes="${cp}([^[:space:];&|]*/)?(themes|theme)[[:space:]]+(apply|reset)([[:space:]]|\$)"
  if [[ $cmd =~ $re_dconf ]]; then
    block "dconf writes touch the live session. Prefix with an isolated DCONF_PROFILE/XDG_CONFIG_HOME in a temp dir, or use tools/headless/run.sh."
  fi
  if [[ $cmd =~ $re_gset ]]; then
    block "gsettings writes touch the live session. Use GSETTINGS_BACKEND=memory, an isolated environment, or tools/headless/run.sh."
  fi
  if [[ $cmd =~ $re_themes ]] && [[ $cmd != *--dry-run* ]]; then
    block "themes apply/reset changes the live desktop. Run it only inside the isolated harness (tools/headless/run.sh) or with --dry-run."
  fi
fi

# 3. git push (global options such as -C dir / -c k=v may precede the subcommand)
re_push="${cp}git([[:space:]]+(-[Cc][[:space:]]+[^[:space:]]+|--[a-z-]+(=[^[:space:]]+)?|-[A-Za-z]))*[[:space:]]+push([[:space:]]|\$)"
if [[ $cmd =~ $re_push ]]; then
  block "agents never push. Commit locally and hand the branch to the maintainer."
fi

# 4. experimental themes need --yes
# not tied to command position: also catches `tools/headless/run.sh themes apply ...`
re_apply="(^|[[:space:]/])(themes|theme)[[:space:]]+apply[[:space:]]+([^;&|]*)"
if [[ $cmd =~ $re_apply ]]; then
  args=${BASH_REMATCH[$(( ${#BASH_REMATCH[@]} - 1 ))]}
  yes=0 id=""
  for w in $args; do
    case "$w" in --yes|-y) yes=1 ;; -*) ;; *) [ -z "$id" ] && id=$w ;; esac
  done
  root=${CLAUDE_PROJECT_DIR:-$PWD}
  conf="$root/themes/$id/theme.conf"
  if [ -n "$id" ] && [ "$yes" = 0 ] && [ -f "$conf" ] && grep -qiE 'experimental|ЕКСПЕРИМЕНТАЛЬН|експериментальн' "$conf"; then
    block "theme '$id' is EXPERIMENTAL (ADR 0006). Ask the user first, and only then add --yes."
  fi
fi

exit 0
