# Maintainer guide

What the person who keeps this project alive needs to know, in one place. It is short on purpose:
the details live in the linked pages. If you learn something the hard way, add it here.

* [Releasing](#releasing)
* [CI at a glance](#ci-at-a-glance)
* [Build environment (toolbox)](#build-environment-toolbox)
* [Updating pinned versions and digests](#updating-pinned-versions-and-digests)
* [Known workarounds](#known-workarounds)

## Releasing

A release is a signed tag; the workflow does the rest and leaves a **draft** for you to publish.

1. Make sure `main` is green (`CI`, `E2E`, `Hygiene`) and `CHANGELOG.md` `Unreleased` is complete.
2. `scripts/release.sh X.Y.Z` (add `--dry-run` first). It sets the version in `Cargo.toml`,
   `Cargo.lock`, `bridge/package.json` and `bridge/static/metadata.json` (also the integer
   `version` GNOME wants), and rolls the changelog. It edits files only.
3. Read the diff, then `git commit -am "Release X.Y.Z"`, `git tag -s vX.Y.Z -m X.Y.Z`,
   `git push origin main vX.Y.Z`.
4. [`release.yml`](../.github/workflows/release.yml) builds in the pinned Fedora 45 container, writes
   `SHA256SUMS`, attaches a build provenance attestation and creates a draft GitHub release with the
   notes taken from the changelog. It never publishes by itself.
5. Check the draft: download an archive, `sha256sum --check SHA256SUMS`,
   `gh attestation verify <file> --repo OlehDzhumyk/desktop-themes-rs`, try `./install.sh` in a VM.
   Then publish it.

A pre-release (`X.Y.Z-rc.1`) is marked as such automatically. A tag that does not match the
`Cargo.toml` version fails the workflow on purpose.

## CI at a glance

All jobs run on GitHub-hosted `ubuntu-latest`; there is no self-hosted runner. Rust jobs run in a
`registry.fedoraproject.org/fedora:45` container (the target platform: GTK 4, libadwaita and
GNOME Shell 51 from the distribution), pinned by digest.

| Workflow | When | What |
|---|---|---|
| [`ci.yml`](../.github/workflows/ci.yml) | push to `main`, PRs | rustfmt, clippy (`-D warnings`), orphan `.rs` check, tests (nextest), cargo-deny, bridge (tsc, eslint, build), shellcheck, schemas, desktop file, D-Bus XML, privacy check, ast-grep, actionlint, docs links |
| [`e2e.yml`](../.github/workflows/e2e.yml) | push to `main`, PRs | GNOME Shell 51 in a rootless podman container with systemd ([ADR 0007](adr/0007-e2e-in-containers.md), [testing](testing.md)); the build job is a Fedora container, the test job uses the runner's own podman |
| [`hygiene.yml`](../.github/workflows/hygiene.yml) | push to `main`, PRs | typos, cargo-machete, zizmor |
| [`headless.yml`](../.github/workflows/headless.yml) | weekly, manual, PRs that touch deskd, the bridge, themes or the harness | clipboard end to end per theme in a headless shell, performance budgets (manual, report only) |
| [`install-smoke.yml`](../.github/workflows/install-smoke.yml) | push to `main`, PRs that touch the installed files | installs the built tree like a user |
| [`agent-context-budget.yml`](../.github/workflows/agent-context-budget.yml) | PRs | size budget of `AGENTS.md` and friends |
| [`release.yml`](../.github/workflows/release.yml) | tag `v*` | see [Releasing](#releasing) |

Local equivalents: `just check` (everything but e2e), `just check-changed` (what you touched),
`just e2e`, `just workflows` (actionlint, and zizmor `--persona pedantic` when installed).

## Build environment (toolbox)

The reference platform is Fedora Silverblue; the host has no compiler, so builds run in a toolbox
created from the Fedora 45 toolbox image. [CONTRIBUTING.md](../CONTRIBUTING.md#setting-up) has the
`dnf install` line; the `Containerfile` builds the same environment with `just`, cargo-nextest,
ast-grep and gitleaks included:

```sh
podman build -t desktop-themes-dev -f Containerfile .
toolbox create --image localhost/desktop-themes-dev themes-dev
toolbox run -c themes-dev just check
```

* Rust: `rust-version` in `Cargo.toml` is the Rust that Fedora 45 ships, and equals the channel in
  `rust-toolchain.toml` (`tools/check-rust-version.sh`, part of `just check` and CI). It is not
  lower on purpose: the GTK bindings alone need a recent compiler, so an older number would be a
  promise nobody tests. When Fedora moves to a newer Rust, bump both together with the container
  digest.
* `cargo` lives in `~/.cargo/bin` when rustup is used; add it to `PATH` inside the toolbox.
* Never run `scripts/bootstrap.sh` from your working copy for tests (it replaces the installed
  binaries) and never test on the live session: see the ground rules in CONTRIBUTING.md.
* Dev tools Fedora does not package (cargo-nextest, ast-grep): `tools/install-dev-tools.sh`.

## Updating pinned versions and digests

Everything third-party is pinned and verified; nothing from other projects is stored here.

| What | Where | How to update |
|---|---|---|
| GitHub Actions | `.github/workflows/*.yml` (commit SHA + version comment) | Dependabot (`github-actions`, weekly, 7-day cooldown) |
| Cargo and npm dependencies | `Cargo.lock`, `bridge/package-lock.json` | Dependabot (`cargo`, `npm`); `cargo deny check` must stay green |
| Container images (Fedora 45, Fedora toolbox 45, actionlint) | `ci.yml`, `e2e.yml`, `headless.yml`, `install-smoke.yml`, `release.yml`, `Containerfile` as `name:tag@sha256:digest` | `tools/update-image-digests.sh` (needs skopeo; `--check` only reports). Dependabot (`docker`) may cover the Containerfiles; the script covers workflow `container:` keys and `docker run` lines. Review, then `just check` |
| E2E base image (ddterm GNOME Shell image) | `tests/e2e/Containerfile`, tag and digest together | `podman pull` prints the digest; Dependabot `docker` may propose it too |
| Rust toolchain | `rust-toolchain.toml`, `rust-version` in `Cargo.toml` | together, when Fedora moves; `tools/check-rust-version.sh` |
| GTK/shell themes, icons, cursors | `scripts/versions.sh` (`*_SHA`, commit of the upstream repo) | `git ls-remote <url> HEAD`, put the commit in, run `tools/headless` and `tools/integration-test.sh` in a VM |
| Fonts | `scripts/versions.sh` (`*_SHA256`) | download the new archive, `sha256sum` it |
| Shell extensions from extensions.gnome.org | `scripts/versions.sh` (`EGO_EXTENSIONS`: uuid, version tag, label, sha256 of the ZIP) | `tools/ego-pin.sh UUID [SHELL_MAJOR]` prints the new line; read the extension's changes first, it is code that runs in your shell. Patches in `patches/` must still apply |
| rustup-init (only `bootstrap.sh --rustup`) | `scripts/versions.sh` (`RUSTUP_INIT_URL`, `RUSTUP_INIT_SHA256`) | new version in the URL, the checksum from the `.sha256` file next to it |
| cargo-nextest, ast-grep | `tools/install-dev-tools.sh` | new version and checksum in the script |
| zizmor, typos, cargo-machete | `hygiene.yml` (version inputs and action SHAs) | Dependabot for the actions, edit the `version:` input by hand |

After any bump: `just check`, then `just e2e`; for theme or extension pins also the headless run.

## Known workarounds

Things that look odd in the code and why they are there. Remove each one when its condition is met.

| Where | Why | Remove when |
|---|---|---|
| `themes-core/src/apply.rs`: `sleep` of 1.2 s after the dconf write and 1.5 s after the extension changes; `verify_live` waits about 5 s | GNOME Shell 51.1 crashes (SIGSEGV in gnome-desktop `update_clock`) when the `org.gnome.desktop.interface` change notification arrives while the extension with the clock is being turned off; new extensions also read their keys on load. No upstream issue is filed yet: when you file one, link it here | the shell no longer crashes, or the apply waits for the shell's signals instead of a fixed time (audit R-4) |
| `patches/spotlight@nin/*.patch` (applied by `scripts/install-extensions.sh`) | the pinned build of [spotlight@nin](https://github.com/itsnin/spotlight) crashes the shell if it steals the overview search during startup, and uses `St.BoxLayout` `vertical`, which Shell 51 dropped; see [patches/README.md](../patches/README.md) | upstream releases a build with both fixes (a patch that no longer applies is skipped with a warning) |
| `bridge/package.json`: TypeScript 7 (`typescript-native`) builds, TypeScript 6 stays for eslint; Dependabot ignores the `typescript` major | [typescript-eslint](https://github.com/typescript-eslint/typescript-eslint) needs the TypeScript 6 compiler API, which TypeScript 7 no longer ships | typescript-eslint supports TypeScript 7 |
| Generated `gtk.css` is copied into `~/.config/gtk-4.0`, not linked | Flatpak apps see `~/.config/gtk-4.0` (with an override), not `~/.cache`; [ADR 0004](adr/0004-design-tokens-generate-libadwaita-css.md) | Flatpak portals offer a theme API for libadwaita |
| E2E runs systemd as PID 1 in a rootless podman container on the runner, not container in container | the shell needs a real cgroup tree; layout after [ddterm's gnome-shell-image](https://github.com/ddterm/gnome-shell-image) ([ADR 0007](adr/0007-e2e-in-containers.md)) | never, unless the harness changes |
| `panic = "unwind"` in `[profile.release]` | `deskd` must survive a panic in one handler or thread; Cargo cannot set it per package. Costs about 0.5 MB per binary. `crates/deskd/src/panic_policy.rs` tests both the setting and the behaviour | never |
| Paste into XWayland apps that bind Shift+Insert to PRIMARY may paste the wrong text | documented limitation of the clipboard feature ([spec](spec/features/clipboard.md)); `paste-on-select` off is the workaround | the app stops binding it |
| `clipboard.rs` session images are capped by `image-budget-mb` and by the free space of `$XDG_RUNTIME_DIR` | the runtime directory is a small tmpfs shared with PipeWire, Wayland and D-Bus sockets | never |
| Theme `flatpak.conf` is limited to an allowlist, theme keys that run commands need a yes | a theme must not weaken the sandbox or run code outside its hooks: [theme contract](theme-contract.md), [SECURITY.md](../SECURITY.md) | never |
