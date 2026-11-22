# desktop-themes-rs: notes for AI agents

Canonical, short on purpose. Read the linked docs instead of asking for a recap.
`CLAUDE.md` only imports this file. Conventions adapted from T3 Code (MIT), see `CREDITS.md`.

## What this is

A GNOME desktop theme and features manager, GPL-3.0-or-later, developed on Fedora Silverblue.
Rust workspace: `themes-core` (library: plan, apply, dconf), `themes` (CLI plus GTK 4 / libadwaita
window), `desk-proto` and `deskd` (session daemon over zbus), `theme-tokens` (design tokens and
generators). `bridge/` is the TypeScript GNOME Shell extension. `themes/` holds the bundled themes,
`docs/spec/` the product specs ([README](docs/spec/README.md), [glossary](docs/spec/glossary.md)),
`docs/adr/` the decisions. Start with [docs/architecture.md](docs/architecture.md).

## Glossary

- **look, behavior, sounds, apps**: the four *modules* of a theme; each can come from a different
  theme ("mixing"). [docs/module-system.md](docs/module-system.md)
- **skin**: how the bridge draws its panels: `macos`, `windows11`, `default` (GSettings key `skin`).
- **baseline**: the `dconf dump /` taken before the first apply; `themes reset` goes back to it.
- **managed keys**: the dconf keys a theme may write; only these are touched and reverted.
- **tokens**: `tokens.toml`, the design description a theme can use instead of hand-made CSS.
  [docs/design-tokens.md](docs/design-tokens.md)
- **bridge**: the shell extension `desk-bridge@desktop-themes-rs`. Thin: UI and clipboard capture only.
- **deskd**: the Rust session daemon; owns logic and storage, talks D-Bus.
- **INHERITS**: `INHERITS=<id>` in `theme.conf`; a child theme layers over a parent.
- **experimental theme**: one whose `theme.conf` says so (e.g. `macos-glass`); it has crashed the
  shell on real hardware. Applying it needs explicit user confirmation ([ADR 0006](docs/adr/0006-experimental-themes-need-confirmation.md)).

## Ways to hurt yourself

1. **Killing by pattern.** Never `pkill`, `killall`, `pgrep | kill`, or kill a PID found by
   matching a name or path. Kill only PIDs you spawned and recorded.
2. **Writing to the live session.** No `dconf write|reset`, `gsettings set|reset`,
   `themes apply|reset`, `gio::Settings::set_*`, or restarting gnome-shell against the real bus.
   Tests use an isolated `XDG_CONFIG_HOME`, `DCONF_PROFILE` and their own bus (`tools/headless/`,
   `tools/headless/run.sh`). Never run `tools/integration-test.sh` outside a VM.
3. **Leaking private data.** The repo is public. No user names, host names, home paths
   (`/var/home/<name>`, `/Users/...`), LAN IPs, SSIDs, personal e-mail (use the GitHub noreply
   address), and nothing about any private, local-only theme. Use placeholders (`/home/user`).
   `tools/privacy-check.sh` enforces generic patterns; put your own words in the untracked
   `.git/info/privacy-patterns`.
4. **Applying an experimental theme** without the user saying yes in this conversation.
5. **Linking generated GTK files.** Generated `gtk.css` is *copied* into `~/.config/gtk-4.0`;
   Flatpak apps cannot read `~/.cache` ([ADR 0004](docs/adr/0004-design-tokens-generate-libadwaita-css.md)).
6. **Installing over your working copy.** `scripts/bootstrap.sh` replaces the installed binaries;
   run `target/debug/...` instead.
7. **Committing generated or third-party assets** (`themes/*/gtk4`, icons, fonts, Apple/Microsoft
   material). They are downloaded by `scripts/` at pinned revisions.

A project hook (`.claude/hooks/guard.sh`) blocks the mechanical cases of 1, 2, 4 and `git push`.
It is a seatbelt, not permission to be careless.

## Hit every surface

The common defect is a change that works on the path you tested and nowhere else. Before calling
work done, say which of these applied:

- **Skins**: `macos`, `windows11`, `default`, in the bridge and in `docs/features.md`.
- **Entry points**: CLI (`crates/themes/src/cli.rs`), GUI window, quick-settings toggle, D-Bus service.
  A feature in one is usually wanted in all.
- **Strings**: English gettext ids in code, Ukrainian in `po/uk.po`; run `just i18n`
  ([docs/i18n.md](docs/i18n.md)). New settings go in the registry (`crates/desk-settings`), then
  `just gen`; never hand-edit the generated schema, accessors or `docs/settings.md`.
- **Accessibility**: labels, keyboard path, focus order, contrast (tokens are validated for WCAG).
- **Docs**: `docs/` pages (`theme-contract.md`, `module-system.md`, ...), README, CHANGELOG
  `Unreleased`; a new decision gets an ADR.
- **Reverse states**: every apply has a revert and a rollback path; a way in needs a way out and
  a way to see the state (`themes doctor`).
- **INHERITS**: child themes must still work when the parent changes; test the chain.
- **Modules**: the change works when only one module comes from the theme (mixing).

## Verifying

- Iterate with `just check-changed` (only what you touched vs `origin/main`). CI runs the full suite;
  do not run workspace-wide builds repeatedly. `just --list` shows the rest.
- Run builds and tests inside the Fedora toolbox (`toolbox run -c <name> ...`; the host has no
  compiler). `Containerfile` builds a matching image ([CONTRIBUTING.md](CONTRIBUTING.md)).
- Test what can break: apply planning, key managing, parsers, rollback, token validation. Skip tests
  for what types already guarantee, copy text, and wiring.
- Extend an existing test or table before adding a new one. Test at the lowest layer that fails
  without your fix; do not repeat the case in wrappers.
- No `sleep`, timeouts, or polling loops in tests; wait on an event, a channel, or a returned value.
  Time-based assertions are bugs.
- Headless GNOME Shell tests only through the isolated harness (`just headless`,
  [ADR 0005](docs/adr/0005-headless-testing-is-isolated.md)). Visual-only fixes: before/after screenshot, no test.
- Generated output (CSS, palettes) is covered by snapshot tests in `crates/theme-tokens/tests`.
  After an intended change, review the diff (`cargo insta review` or `UPDATE_SNAPSHOTS=1`).
- Do not mass-format or fix unrelated clippy findings inside a feature change.
- Style: SPDX header on every file, minimal diffs, no new dependency without a reason in the PR.

## Commits and PRs

- Commit as the GitHub noreply identity (`<id>+<name>@users.noreply.github.com`); small logical commits.
- **Agents never push.** Hand the branch to the maintainer, who reviews and pushes.
- Delete `HANDOFF*.md` notes before the final commit; the tree must not carry them.
- PR description follows `.github/PULL_REQUEST_TEMPLATE.md` (surface checklist plus proof: test,
  before/after screenshot, or headless transcript).
- `AGENTS.md`, `CLAUDE.md` and `.claude/**/*.md` have a size budget (`.github/agent-context-budget.json`).
  Cut text before adding; growth needs the `agent-context-increase` label.

## Optional tools

`just`, `cargo-nextest`, `ast-grep` (`tools/install-dev-tools.sh`), `gitleaks`, `typos`,
`cargo-machete`, `zizmor`. Opt-in MCP servers that help navigation (not configured here, no keys
needed locally): Serena (symbol navigation), context7 (library docs; queries leave the machine).
