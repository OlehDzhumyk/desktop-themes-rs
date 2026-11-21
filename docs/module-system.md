# Module system

A theme is the union of four independent modules (`look`, `behavior`, `sounds`, `apps`). The state
records, for each module, which theme it currently comes from; `themes current` prints it. A theme
card in the window shows "active" only when all modules come from it.

```sh
themes apply macos                       # all four modules from macos
themes apply windows11 --only behavior   # only the behavior module from windows11
themes apply macos --module look --module sounds
```

## Managed keys

For a module, the managed keys are the union of

* every dconf key that **any** installed theme sets in that module,
* every key this module wrote the last time it was applied (recorded in `state.json`).

Applying a module touches only its managed keys. For each of them the desired value is:

1. the value in the selected theme's files for that module, else
2. the value in the baseline (`baseline/full.dconf`), else
3. reset to the schema default.

So switching from a theme that sets a key to one that does not leaves no stale values. A module
whose selected theme has no files for it is shown as "(default)" and returns its keys to the
baseline.

## Overlapping keys

If two modules set the same key, the later module wins, in the order look, behavior, sounds, apps.
(Example: `toggle-overview` is cleared by the macOS look through the Spotlight fragment and also by
the macOS behavior module, otherwise both Spotlight and the overview would react to Super+Space.)

## Extensions

The enabled set is: (baseline extensions + extensions you enabled manually) minus extensions of all
themes, plus `extensions.txt` of the theme providing **look**, plus the permanent
`themes-quick@desktop-themes-rs`, minus extensions disabled as broken. Extensions are switched
about one second after the settings; see docs/architecture.md.

## Safety nets

* `baseline`: your own settings, captured once; `themes reset` returns to it.
* Auto-rollback: ~5 s after a live apply, extensions in `ERROR` state are disabled and a
  notification is shown.
* `themes doctor --fix` (also run by the systemd user unit after each login) disables broken
  extensions and re-enables repaired ones when the extension or the GNOME version changes.
* GTK symlinks: files of yours that are displaced go to `backups/` and come back when the theme is
  switched off. `gtk3/bookmarks` is never touched.
* A shared lock prevents the CLI, the window and the D-Bus service from applying concurrently.
