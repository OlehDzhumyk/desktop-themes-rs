## What and why

<!-- What changes, and why. Link the issue if there is one. -->

## How was it tested

- [ ] `cargo test --workspace --locked`, and for the bridge `npm run typecheck && npm run lint`
- [ ] in the isolated headless session (`tools/headless/`) or a VM, **not** on my live session
- [ ] `tools/perf/run.sh` summary attached (only for changes to deskd, the clipboard path or the bridge)

## Checklist

- [ ] `CHANGELOG.md` has an entry under **Unreleased** (user-visible changes)
- [ ] docs updated (`README.md`, `docs/`) where behaviour or files changed
- [ ] no personal data, no third-party or Apple/Microsoft assets; new third-party material is in `CREDITS.md`
- [ ] every new file has an SPDX header (GPL-3.0-or-later)
