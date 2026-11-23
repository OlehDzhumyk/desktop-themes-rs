# 0010. The live wallpaper palette is a recorded overlay in the generated gtk.css copy

* Status: accepted
* Date: 2026-11-23

## Context

The live "accent from wallpaper" mode ([spec](../spec/features/theme-from-wallpaper.md)) must give
token themes the full palette of the wallpaper's seed, not only `accent-color`, and must never edit
the theme's files. libadwaita apps read one file, `~/.config/gtk-4.0/gtk.css`, which for a token
theme is already a recorded copy of the generated stylesheet ([ADR 0004](0004-design-tokens-generate-libadwaita-css.md)).
deskd owns the live mode but does not link themes-core; themes-core owns `state.json`, the
manager's lock and every revert path (theme switch, `themes reset`, rollback, `themes doctor`).

## Decision

The overlay is one more token layer, the seed's tonal palette (`[colors.light]` and
`[colors.dark]` only), put above the theme's `INHERITS` chain. themes-core renders it to
`~/.cache/desktop-themes/wallpaper-overlay/<theme>/gtk.css` and writes that content into the
existing gtk.css copy, under the manager's lock.

* deskd runs `themes tokens overlay SEED` and `themes tokens overlay --remove` as child processes,
  in order, on one worker thread.
* `state.json` records the overlay (`wallpaper_overlay`: theme, seed, copy, the theme's own
  generated file) before the copy changes; the copy's recorded hash becomes the overlay's.
* `--remove` writes the theme's own generated file back (regenerating it if the cache was cleaned)
  and deletes the record and the overlay cache. `gtklinks::unlink_all` (theme switch, reset,
  rollback) removes the copy as usual and then drops the record and the cache.
* A theme switch drops the overlay until the next wallpaper change, like the accent (decision of
  the stage 2 pull request: a theme or user change wins until then).
* Only a generated copy is ever replaced: hand-made stylesheets (WhiteSur, Fluent) keep
  accent-only; a copy the user edited is left alone.
* The shell layer, the Ptyxis palette and the dconf keys of the theme are not changed.

## Consequences

* Good: one file to revert, already covered by the copy bookkeeping; Flatpak apps see the
  palette; contrast is AA by construction and the merged tokens are validated before writing.
* Good: a malformed record or cache is visible in `themes doctor` and fixed by `--fix`.
* Bad: a rollback of a failed apply does not bring the overlay back; deskd adds it again at the
  next wallpaper or colour-scheme change, or at its next start.
* Bad: a child process per accepted seed (a few milliseconds; idempotent).

## Alternatives considered

* Writing gtk.css from deskd: a second writer of the manager's state without its lock.
* A separate CSS file imported by the theme's gtk.css: the generated file would change for every
  theme, and Flatpak apps would need a second override path.
* Only the accent in CSS: the theme's other colours would not belong to the wallpaper, and an
  accent replaced alone can break the theme's accent text contrast.
