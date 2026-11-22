#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Privacy check for a PUBLIC repository: finds personal or machine-specific strings.
#
#   tools/privacy-check.sh --all            every tracked file (CI)
#   tools/privacy-check.sh --staged         files staged for commit (pre-commit hook)
#   tools/privacy-check.sh --diff [BASE]    files changed since BASE (default origin/main) incl. worktree
#   tools/privacy-check.sh FILE...          the given files
#
# Generic patterns live here (home paths, private IPv4 ranges, .local hostnames, e-mail addresses
# other than GitHub noreply). Your own words (user name, host names, SSIDs, a private theme's
# name) must NEVER be committed: put them, one extended regex per line (case-insensitive,
# '#' comments), in an untracked file:
#   $(git rev-parse --git-path info/privacy-patterns)       i.e. .git/info/privacy-patterns
#   ${XDG_CONFIG_HOME:-~/.config}/desktop-themes-rs/privacy-patterns
# Known harmless matches (public third-party extension UUIDs, placeholders) are in
# tools/privacy-allowlist.txt (regexes that a matching string must fully match).
# Exit status: 0 clean, 1 findings, 2 usage error.
set -u
cd "$(git rev-parse --show-toplevel)" || exit 2

mode=${1:---all}
case "$mode" in
  --all) mapfile -d '' -t files < <(git ls-files -z) ;;
  --staged) mapfile -d '' -t files < <(git diff --cached --name-only -z --diff-filter=ACMR) ;;
  --diff)
    base=${2:-origin/main}
    mapfile -d '' -t files < <({ git diff --name-only -z --diff-filter=ACMR "$base"...HEAD
                                 git diff --name-only -z --diff-filter=ACMR HEAD
                                 git ls-files -z --others --exclude-standard; } | sort -zu) ;;
  -*) echo "usage: $0 [--all|--staged|--diff [BASE]|FILE...]" >&2; exit 2 ;;
  *) files=("$@") ;;
esac

# Never scan lock files, this script, or the allowlist (they contain the patterns themselves).
filtered=()
for f in "${files[@]}"; do
  case "$f" in
    *Cargo.lock|*package-lock.json|tools/privacy-check.sh|tools/privacy-allowlist.txt|*.snap) continue ;;
  esac
  [ -f "$f" ] && filtered+=("$f")
done
[ ${#filtered[@]} -gt 0 ] || { echo "privacy-check: nothing to scan"; exit 0; }

# name: extended regex. Placeholders such as /home/user are not findings (see $placeholder).
placeholder='^([A-Za-z]|user|username|you|name|me|example|runner|USER|YOU|NAME|USERNAME)$'
generic=(
  'home path:/(var/)?home/[A-Za-z_][A-Za-z0-9._-]*/?'
  'macOS home path:/Users/[A-Za-z_][A-Za-z0-9._-]*/?'
  'private IPv4:(^|[^0-9.])(10(\.[0-9]{1,3}){3}|192\.168(\.[0-9]{1,3}){2}|172\.(1[6-9]|2[0-9]|3[01])(\.[0-9]{1,3}){2})([^0-9]|$)'
  '.local hostname:[A-Za-z0-9][A-Za-z0-9-]*\.local([^A-Za-z0-9_/.-]|$)'
  'e-mail address:[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}'
)

allow=()
if [ -f tools/privacy-allowlist.txt ]; then
  while IFS= read -r l; do [ -n "$l" ] && [ "${l#\#}" = "$l" ] && allow+=("$l"); done < tools/privacy-allowlist.txt
fi

is_allowed() { # $1 = matched text
  local m=$1 a
  # trim the boundary characters the IPv4/.local patterns capture
  m=${m#[^A-Za-z0-9/]}; m=${m%[^A-Za-z0-9/]}
  for a in "${allow[@]}"; do [[ $m =~ ^($a)$ ]] && return 0; done
  return 1
}

status=0
report() { printf '%s:%s: %s: %s\n' "$1" "$2" "$3" "$4"; status=1; }

for entry in "${generic[@]}"; do
  name=${entry%%:*}; re=${entry#*:}
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    file=${hit%%:*}; rest=${hit#*:}; line=${rest%%:*}; text=${rest#*:}
    while IFS= read -r m; do
      case "$name" in
        *'home path')
          who=${m#/}; who=${who#var/}; who=${who#home/}; who=${who#Users/}; who=${who%%/*}
          [[ $who =~ $placeholder ]] && continue ;;
      esac
      is_allowed "$m" && continue
      case "$name:$m" in 'e-mail address:'*@users.noreply.github.com) continue ;; esac
      report "$file" "$line" "$name" "$m"
    done < <(printf '%s\n' "$text" | grep -oE "$re")
  done < <(grep -InE "$re" -- "${filtered[@]}" 2>/dev/null)
done

# Personal patterns from untracked files (never printed in CI: the files do not exist there).
extra_files=("$(git rev-parse --git-path info/privacy-patterns)" "${XDG_CONFIG_HOME:-$HOME/.config}/desktop-themes-rs/privacy-patterns")
for pf in "${extra_files[@]}"; do
  [ -f "$pf" ] || continue
  pats=$(grep -vE '^[[:space:]]*(#|$)' "$pf" || true)
  [ -n "$pats" ] || continue
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    report "${hit%%:*}" "$(echo "${hit#*:}" | cut -d: -f1)" "personal pattern ($(basename "$(dirname "$pf")")/$(basename "$pf"))" "(line hidden)"
  done < <(grep -InEi -f <(printf '%s\n' "$pats") -- "${filtered[@]}" 2>/dev/null)
done

if [ $status -ne 0 ]; then
  echo "privacy-check: findings above. Replace with placeholders (/home/user, user@example.org) or, for a" >&2
  echo "public third-party string, add a regex to tools/privacy-allowlist.txt." >&2
fi
exit $status
