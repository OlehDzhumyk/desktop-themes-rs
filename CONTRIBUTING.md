# Contributing to desktop-themes-rs

Thanks for helping. This project changes the look and behaviour of a person's desktop, so the
rules below are mostly about **not breaking anyone's session** while you work on it.

By contributing you agree that your work is released under **GPL-3.0-or-later**, like the rest of
the repository. Please read the [Code of Conduct](CODE_OF_CONDUCT.md). Security problems go through
[SECURITY.md](SECURITY.md), not public issues.

## Ground rules

1. **Never test on your live GNOME session.** Theme switching writes dconf, enables and disables
   shell extensions and has crashed GNOME Shell before. Use the isolated headless harness
   (`tools/headless/`, see below) or a virtual machine. `tools/integration-test.sh` writes your real
   dconf and must only run in a VM or a spare session.
2. **No personal data in commits**: no real names, e-mail addresses (use your GitHub noreply
   address), host names, paths with your user name, clipboard contents, screenshots of a real
   desktop, dconf dumps or `baseline/` files. `themes doctor --json` output in bug reports may
   contain paths: read it before you paste it.
3. **No third-party or proprietary assets.** Themes, icons, cursors, fonts, wallpapers and
   extensions from other projects are downloaded by `scripts/` at pinned revisions and are never
   committed. No Apple or Microsoft logos, fonts, wallpapers or sounds. Credit every project in
   [CREDITS.md](CREDITS.md).
4. **User level only.** Nothing may need sudo or `rpm-ostree` layering.
5. **Every file gets an SPDX header** (`# SPDX-License-Identifier: GPL-3.0-or-later`, or the
   comment form of your language).

## Setting up

The reference platform is Fedora Silverblue (GNOME Shell 51); the build runs in a toolbox because
the host has no compiler.

```sh
git clone https://github.com/OlehDzhumyk/desktop-themes-rs && cd desktop-themes-rs
toolbox create themes-dev
toolbox run -c themes-dev sudo dnf install -y git gcc pkgconf-pkg-config glib2-devel gtk4-devel \
    libadwaita-devel pipewire-devel clang-devel rust cargo rustfmt clippy cargo-deny ShellCheck nodejs npm desktop-file-utils
# Rust >= 1.100 is required (rust-version in Cargo.toml = Fedora 45). If the distribution's is older: scripts/bootstrap.sh --rustup
```

Build and check (inside the toolbox, or prefix with `toolbox run -c themes-dev`):

```sh
cargo build --workspace
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo deny check                         # licences must be GPL-3.0-or-later compatible, no advisories
(cd bridge && npm ci && npm run typecheck && npm run lint && npm run build)
shellcheck --severity=warning $(git ls-files '*.sh')
glib-compile-schemas --strict --dry-run data/
desktop-file-validate data/*.desktop
```

These are the same checks as `.github/workflows/ci.yml`; running them before you push saves a
round trip. (rustfmt and clippy are report-only in CI at the moment; new code should be clean
anyway.)

### Do not use `cargo build` for what you install

`scripts/bootstrap.sh` builds a release binary and installs it to `~/.local/bin`, replacing your
working copy of the tool. While you develop, run `target/debug/themes` and `target/debug/deskd`
directly, and read [docs/architecture.md](docs/architecture.md) first.

## End-to-end tests (`just e2e`)

`tests/e2e/` runs GNOME Shell 51 headless in a throw-away podman container and tests the freshly
built bridge, deskd and `themes` in it ([docs/testing.md](docs/testing.md),
[ADR 0007](docs/adr/0007-e2e-in-containers.md)). Only podman is needed on the host; the build runs
in the toolbox (`E2E_TOOLBOX`, default `themes-rs-build`):

```sh
just e2e                    # build, then the whole suite (about a minute after the first image pull)
just e2e -k clipboard       # pytest arguments pass through
tools/e2e.sh --no-build -m smoke
```

Screenshots, shell logs of failed tests and the container journal land in `target/e2e/`.

## Headless testing (the safe way)

`tools/headless/` starts a complete GNOME Shell with `--headless` on a **private** D-Bus bus,
Wayland display, `XDG_RUNTIME_DIR` subdirectory, dconf database and data directory, so nothing of
your real session is read for writing:

```sh
cargo build -p deskd
(cd bridge && npm ci && npm run build)
export HS_TOOLBOX=themes-dev        # toolbox that has dbus-daemon; HS_TOOLBOX=none inside a container
tools/headless/clipboard-test.sh windows11 Super+v /tmp/shots     # end-to-end clipboard test
tools/headless/start.sh --theme macos --ext bridge/build/desk-bridge@desktop-themes-rs
tools/headless/run.sh dconf dump /     # run any command inside the isolated session
tools/headless/stop.sh
```

Rules for changes to the harness and for anything you run in it:

* Always go through `run.sh` so the private bus and dconf are used; never export
  `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/…/bus` (that is your real bus).
* Never call `themes apply` against the real environment to "try it quickly". In the harness, give
  `themes` a fake `HOME` and its own `DESKTOP_THEMES_ROOT` (see `tools/perf/measure.py cli`).
* `stop.sh` kills only the processes it started or that carry this environment's bus address; never
  `pkill gnome-shell` or similar by name.
* Screenshots need PipeWire (`HS_NO_SHOTS=1` skips them, as CI does).

CI runs the same end-to-end test in a Fedora container (`.github/workflows/headless.yml`, manual,
weekly and on changes under `bridge/`, `crates/deskd/`, `themes/`, `tools/`).

## Performance

`tools/perf/run.sh` measures deskd memory/CPU/wakeups, clipboard panel latency, `themes list` and
`themes apply` times and the shell memory cost of the bridge in the headless session, and compares
them with `tools/perf/budgets.toml`. Run it on a **release** build when your change touches the
clipboard path, deskd's event loop or the bridge's hot paths, and include the summary in the pull
request.

```sh
cargo build --release -p deskd -p themes
tools/perf/run.sh --report-only
```

## Commit and pull request style

* Small, focused commits; one logical change each. Imperative, present-tense subject of at most
  72 characters, no trailing period, with an optional area prefix: `deskd: cap image history by
  size`, `bridge: fix panel focus after Escape`, `docs: explain theme inheritance`.
* The body says **why**, not only what, wraps at about 80 columns, and mentions anything that
  affects users (migration, changed key, new dependency).
* Do not mix formatting-only changes with functional ones.
* Branch from `main`; open the pull request against `main`. Fill in the template: what changed,
  how you tested it (headless? VM?), and whether `CHANGELOG.md` has an entry under **Unreleased**.
* New behaviour needs a test where one is practical (unit tests in the crates, the headless e2e
  test for shell-side behaviour).
* Third-party code or ideas: say so in the file header and in CREDITS.md, with the licence.

## Changelog and releases

User-visible changes get a line under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md)
([Keep a Changelog](https://keepachangelog.com) format). Maintainers cut releases with
`scripts/release.sh VERSION`, which updates the versions and the changelog; pushing the tag `vVERSION`
makes `.github/workflows/release.yml` build the artifacts and draft the release.

## Reporting bugs and asking for features

Use the issue templates. For a bug, attach the output of `themes doctor --json` (after removing
anything private) and say which GNOME Shell version you run. Feature requests ask which operating
system the idea comes from, so that the behaviour can be matched faithfully.

## Development environment, commands and hooks

A `Containerfile` builds the same Fedora 45 toolchain as CI, including `just`, `cargo-nextest`,
`ast-grep` and `gitleaks`:

```sh
podman build -t desktop-themes-dev -f Containerfile .
toolbox create --image localhost/desktop-themes-dev themes-dev
```

`rust-toolchain.toml` pins the Rust release Fedora 45 ships (rustup users get it automatically).
`just --list` shows the command surface; the useful ones:

```sh
just check-changed     # while iterating: only what you changed vs origin/main
just check             # everything CI checks (fmt and clippy have known findings for now)
just privacy           # personal or machine-specific strings in tracked files
just headless          # clipboard end-to-end test in the isolated headless shell
just install-hooks     # pre-commit: gitleaks, rustfmt of staged files, privacy check
tools/install-dev-tools.sh   # pinned cargo-nextest + ast-grep into ~/.local/bin
```

The privacy check ships only generic patterns. Put your own (user name, host names, SSIDs) in the
untracked `.git/info/privacy-patterns` (one regex per line) so they never reach the repository.

Decisions that shaped the project are in [docs/adr/](docs/adr/). AI coding agents: read
[AGENTS.md](AGENTS.md); the Claude Code hook in `.claude/` blocks the commands that can hurt a
live session.
