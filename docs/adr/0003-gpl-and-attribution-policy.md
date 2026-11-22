# 0003. GPL-3.0-or-later; port with attribution; never redistribute third-party assets

* Status: accepted
* Date: 2026-11-21

## Context

The project builds on and next to much existing work (WhiteSur, Fluent, Clipboard Indicator, GNOME
extensions, libadwaita defaults). It is public and must be clean to package, fork and review.

## Decision

* All own code is **GPL-3.0-or-later**; every file carries an SPDX header. `cargo deny` fails the
  build on dependencies whose licence is not compatible.
* We may port code from permissively or compatibly licensed projects. A port keeps the upstream
  copyright notice, names the origin in the file header and is listed in [CREDITS.md](../../CREDITS.md)
  with licence and pinned commit. Facts (such as default colour values) may be listed, expressive
  text is not copied.
* Third-party themes, icons, cursors, fonts, wallpapers and extensions are **not committed**;
  `scripts/` downloads them from upstream at pinned revisions onto the user's machine.
* No Apple or Microsoft logos, fonts, wallpapers or sounds. Own sounds and wallpapers are generated
  and released under CC0.

## Consequences

* Good: licence status of every file is traceable; the repository stays small and redistributable.
* Bad: installation needs network access and pinned downloads must be kept current
  (`scripts/versions.sh`).

## Alternatives considered

* Vendoring the upstream themes: simpler install, but mixes licences and redistributes trademarks.
* A permissive licence: incompatible with GPL-only upstream pieces we build on.
