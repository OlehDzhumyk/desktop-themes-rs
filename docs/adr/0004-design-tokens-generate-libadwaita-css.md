# 0004. Design tokens generate libadwaita CSS, which is copied, not linked

* Status: accepted
* Date: 2026-11-21

## Context

libadwaita apps ignore GTK themes; they are styled through `~/.config/gtk-4.0/gtk.css`. Hand-written
CSS per theme drifts from libadwaita's versions. Flatpak apps see `~/.config/gtk-4.0` only (with an
override), not `~/.cache`. See [docs/design-tokens.md](../design-tokens.md).

## Decision

* A theme may describe its look in `tokens.toml` (schema 1, validated incl. WCAG contrast). The
  `theme-tokens` crate generates the libadwaita stylesheet, a GNOME Shell layer, a Ptyxis palette and
  dconf keys at apply time.
* Generated files go to `~/.cache/desktop-themes/<id>/` and are never written into the repository or
  the theme directory.
* The generated `gtk.css` is **copied** into `~/.config/gtk-4.0/`, recorded in the state so reverts
  remove exactly that copy. It is never symlinked into `~/.cache`.
* Hand-made files win over derived ones (a theme's own `*.dconf` or `gtk4/` directory switches
  generation off); only keys that are set are emitted.
* Generators are covered by snapshot tests, and output is deterministic.

## Consequences

* Good: one source of truth per theme, upgrades follow libadwaita, Flatpak apps are themed.
* Bad: a stale copy is possible if state is lost; `themes doctor` checks it and apply refreshes it.

## Alternatives considered

* Symlink into `~/.cache`: broken for Flatpak apps (found on real hardware).
* Patching WhiteSur/Fluent output with regexes: breaks on each upstream release.
