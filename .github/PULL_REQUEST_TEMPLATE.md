## What and why

<!-- What changes, and why. Link the issue if there is one. -->

## Surfaces

Tick what applies, write "n/a" for the rest (see AGENTS.md, "Hit every surface").

- [ ] Skins: macos / windows11 / default
- [ ] Entry points: CLI, window, quick-settings toggle, D-Bus
- [ ] Strings in Ukrainian and English
- [ ] Accessibility (labels, keyboard path, contrast)
- [ ] Docs: `docs/`, README, `CHANGELOG.md` under **Unreleased**; ADR if a decision changed
- [ ] Reverse states: revert / rollback exists and is tested
- [ ] INHERITS chains and module mixing still work

## Proof

<!-- Pick what fits: a test that fails without the change, a before/after screenshot for
     visual changes, or a transcript from the ISOLATED headless session. -->

## Checklist

- [ ] `just check-changed` is green (CI runs the rest); for the bridge also `npm run typecheck && npm run lint`
- [ ] tested in the isolated headless session (`tools/headless/`) or a VM, **not** on my live session
- [ ] `tools/perf/run.sh` summary attached (only for changes to deskd, the clipboard path or the bridge)
- [ ] no personal data (`just privacy`), no third-party or Apple/Microsoft assets; new third-party material is in `CREDITS.md`
- [ ] every new file has an SPDX header (GPL-3.0-or-later)
- [ ] no `HANDOFF*.md` files; agent context files did not grow (or the `agent-context-increase` label is justified)
