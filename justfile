# SPDX-License-Identifier: GPL-3.0-or-later
# Developer command surface. Run inside the Fedora toolbox (see CONTRIBUTING.md). `just --list`.
set shell := ["bash", "-euo", "pipefail", "-c"]

# list recipes
default:
    @just --list

# everything CI checks, locally
check: fmt-check lint test xtask-check bridge shellcheck privacy ast-grep

# fast checks for what you changed vs origin/main
check-changed:
    tools/check-changed.sh

# tests: cargo-nextest when installed (failure-only output), else cargo test
test:
    if command -v cargo-nextest >/dev/null; then \
      cargo nextest run --workspace --locked --status-level fail --final-status-level fail && cargo test --workspace --locked --doc; \
    else cargo test --workspace --locked; fi

# format all Rust
fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

# clippy plus the other linters
lint: clippy bridge-lint

clippy:
    cargo clippy --workspace --locked --all-targets -- -D warnings

bridge-lint:
    cd bridge && npm run lint

# bridge typecheck + lint
bridge:
    cd bridge && npm run typecheck && npm run lint

shellcheck:
    git ls-files -z | xargs -0 grep -IlE '^#!.*(ba)?sh' | xargs shellcheck --severity=warning

# personal/machine-specific strings in tracked files (generic + your untracked patterns)
privacy:
    tools/privacy-check.sh --all

# generated files (GSettings schema, accessors, docs/settings.md) match the settings registry
xtask-check:
    cargo xtask check

# regenerate them after editing crates/desk-settings/src/registry.rs
gen:
    cargo xtask gen

# translation template + merge into po/*.po, then the i18n lint
i18n:
    cargo xtask pot
    cargo test -p desk-settings --locked --test i18n_lint

# structural lint rules (tools/ast-grep) and their own tests
ast-grep:
    ast-grep scan
    ast-grep test --skip-snapshot-tests

# workflow lint: actionlint (and zizmor when installed)
workflows:
    actionlint
    if command -v zizmor >/dev/null; then zizmor .github/workflows; fi

# optional hygiene tools, skipped when not installed
hygiene:
    if command -v typos >/dev/null; then typos; else echo "typos not installed, skipped"; fi
    if command -v cargo-machete >/dev/null; then cargo machete; else echo "cargo-machete not installed, skipped"; fi

# bytes of agent context vs the recorded budget
context-budget:
    @tools/agent-context-bytes.sh

# GNOME Shell end-to-end tests in a podman container (tests/e2e, ADR 0007): just e2e -k clipboard
[positional-arguments]
e2e *args:
    tools/e2e.sh "$@"

# enable the repository's git hooks
install-hooks:
    git config core.hooksPath .githooks
    @echo "hooks enabled (.githooks/pre-commit)"

# install pinned cargo-nextest and ast-grep into ~/.local/bin
tools:
    tools/install-dev-tools.sh
