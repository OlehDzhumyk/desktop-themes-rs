# 0001. The shell bridge is TypeScript, not WASM

* Status: accepted
* Date: 2026-11-21

## Context

Clipboard capture, the Win+V / Spotlight-like panels and the quick-settings toggle must run inside
GNOME Shell: mutter has no `wlr-data-control`, so only the shell can watch the clipboard
(`Meta.Selection`), and only shell code can draw `St` / `Clutter` widgets. GNOME Shell loads
extensions as GJS ES modules. The rest of the project is Rust, so compiling the bridge from Rust to
WebAssembly was considered.

## Decision

`bridge/` is a TypeScript project compiled with `tsc` to the ES modules GNOME Shell loads, typed with
`@girs/gnome-shell` (ts-for-gir). It is as small as possible: UI and capture only. Logic stays in
Rust ([ADR 0002](0002-one-daemon-thin-bridge.md)).

## Consequences

* Good: direct access to `St`, `Clutter`, `Meta`, `Shell` and shell internals; no glue layer; types
  catch API drift when the shell version moves (`npm run typecheck` in CI).
* Good: the extension remains reviewable on extensions.gnome.org, which expects readable JS.
* Bad: a second language and toolchain (Node, eslint) in a Rust project; mitigated by keeping the
  bridge thin.

## Alternatives considered

* Rust compiled to WASM: the module would have no direct access to `St`/`Clutter`, so every UI call
  would still go through a JavaScript shim, and extensions.gnome.org review does not accept binary
  blobs.
* Plain JavaScript: loses type checking against the shell's API.
