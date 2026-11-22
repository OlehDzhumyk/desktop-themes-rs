#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fast checks for what you changed relative to origin/main (CI still runs everything).
#   tools/check-changed.sh [BASE]      STRICT=1 makes clippy warnings fatal
set -eu
cd "$(git rev-parse --show-toplevel)"
base=$(git merge-base "${1:-origin/main}" HEAD)
mapfile -t changed < <({ git diff --name-only "$base"; git ls-files --others --exclude-standard; } | sort -u)
[ ${#changed[@]} -gt 0 ] || { echo "check-changed: nothing changed vs ${1:-origin/main}"; exit 0; }

crates=() workspace=0 bridge=0 sh=()
for f in "${changed[@]}"; do
  [ -e "$f" ] || continue
  case "$f" in
    Cargo.toml|Cargo.lock|rust-toolchain.toml|rustfmt.toml|data/*) workspace=1 ;;
    crates/*/*) c=${f#crates/}; c=${c%%/*}; [[ " ${crates[*]:-} " == *" $c "* ]] || crates+=("$c") ;;
    bridge/*) bridge=1 ;;
  esac
  case "$f" in *.sh|.githooks/*) sh+=("$f") ;; esac
done

step() { printf '\n== %s\n' "$*"; }
deny=(); [ "${STRICT:-0}" = 1 ] && deny=(-- -D warnings)

if [ $workspace = 1 ]; then pk=(--workspace); else pk=(); for c in "${crates[@]:-}"; do [ -n "$c" ] && pk+=(-p "$c"); done; fi
if [ ${#pk[@]} -gt 0 ]; then
  step "clippy ${pk[*]}"
  cargo clippy "${pk[@]}" --all-targets --locked "${deny[@]}"
  step "tests ${pk[*]}"
  if command -v cargo-nextest >/dev/null 2>&1; then cargo nextest run "${pk[@]}" --locked --status-level fail --final-status-level fail --no-tests=pass
  else cargo test "${pk[@]}" --locked; fi
  mapfile -t rs < <(printf '%s\n' "${changed[@]}" | grep -E '\.rs$' || true)
  existing=(); for f in "${rs[@]:-}"; do [ -n "$f" ] && [ -f "$f" ] && existing+=("$f"); done
  if [ ${#existing[@]} -gt 0 ]; then
    step "rustfmt (changed files; existing unformatted files report too)"
    rustfmt --check --edition 2024 "${existing[@]}" 2>&1 | grep -E '^Diff in' | sed 's/:[0-9]*:$//' | sort -u || true
  fi
fi
if [ $bridge = 1 ]; then
  step "bridge typecheck + lint"
  (cd bridge && npm run typecheck && npm run lint)
fi
if [ ${#sh[@]} -gt 0 ]; then
  step "shellcheck"
  shellcheck --severity=warning "${sh[@]}"
fi
step "privacy"
tools/privacy-check.sh --diff "$base"
step "ast-grep"
if command -v ast-grep >/dev/null 2>&1; then ast-grep scan; else echo "ast-grep not installed (tools/install-dev-tools.sh), skipped"; fi
echo; echo "check-changed: ok"
