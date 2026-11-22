# Theme switching, modules, mixing, inheritance, reset

## Summary

The user switches the whole look of the desktop between the bundled themes (`fedora`, `macos`,
`macos-glass`, `windows11`) and their own, or takes single **modules** (`look`, `behavior`, `sounds`,
`apps`) from different themes. Everything is done at user level (dconf, GTK symlinks, flatpak
overrides, extensions switched live); nothing needs sudo, and everything can be undone.

## Status

**Implemented** (stage 0). Differences from the intended behaviour are listed as gaps below.
Origin badge: *Original* (the themes use WhiteSur and Fluent assets installed from upstream, see
[CREDITS.md](../../../CREDITS.md)).

## Reference behaviour

Neither Windows 11 nor macOS has a "theme manager"; both have a light/dark switch, an accent colour
and a wallpaper. The bundled themes recreate the *whole* desktop idiom of each system (shell, dock or
taskbar, icons, cursors, fonts, sounds, keyboard conventions) in a GNOME session. See
[behavior](behavior.md), [sounds](sounds.md), [lock-screen](lock-screen.md).

## Our behaviour

### Themes

* A theme is a directory `<theme root>/themes/<id>/` following the [theme contract](../../theme-contract.md).
  `<id>` is `[a-z0-9][a-z0-9._-]{0,63}`. Files the manager does not know are ignored.
* Bundled: `fedora` (built-in, stock look, cannot be deleted, is the **safe theme**), `macos`,
  `macos-glass` (Experimental, inherits `macos`, see [liquid-glass](liquid-glass.md)), `windows11`.
  `lavender` is a token-only showcase theme (just `theme.conf` + `tokens.toml`, see design-tokens (`docs/design-tokens.md`, lands with its stage 1.5 pull request)); **Partial** until its generated output is complete (stage 1.5, PR in review). Further themes are the user's own (snapshots, hand-made).
* `themes list` shows id, name, modules the theme provides, which of its modules are active, and the
  description; `*` marks the theme that provides the **look** module. `--json` gives
  `id, name, description, preview, preview_dark, active, active_modules, modules, builtin, extensions, path`.
* `themes current` shows which theme each module comes from, or "all modules from <theme>".

### Modules and mixing

* The four modules are independent: `themes apply macos --only look,sounds`, `--module behavior`
  (repeatable). With no module option all four apply.
* The state records, per module, which theme it comes from. A theme card is "active" only when all
  four modules come from it; otherwise the window says «Змішано: ...» (mixed) and the quick-settings
  toggle appends «(змішано)».
* Per module, the **managed keys** are: every key any installed theme sets in that module, plus every
  key the module wrote last time. For each managed key the desired value is the selected theme's
  value, else the baseline value, else the schema default (key reset). Applying a module touches only
  its managed keys.
* A key set by two modules is written by the later module in the order look, behaviour, sounds, apps.
* Mixing keeps the look's skin: the skin and the clipboard shortcut live in the look module's
  `desk.dconf`, so a macOS look with Windows 11 behaviour shows the macOS clipboard panel on
  Shift+Super+V (the shortcut is the look's, the rest of the keyboard is the behaviour's).
* A module whose selected theme has no files for it is shown as "(default)" and returns its keys to
  the baseline.

### Applying (order is binding)

1. Take the lock (`.lock`, shared by CLI, window and service); a second apply waits up to 60 s for it, then fails
   with "another process holds the lock"; two applies never run concurrently.
2. Read baseline, current dconf dump, state, the selected source per module.
3. Compute the plan (pure function): managed keys, desired values, extension set.
4. Write dconf keys; link GTK files; run flatpak overrides and `apps/hooks`.
5. **About one second later** write `enabled-extensions` (the shell loads/unloads live). The pause is
   deliberate: changing `org.gnome.desktop.interface` at the moment an extension is disabled crashed
   GNOME Shell 51.1.
6. About 5 s later verify extensions; any in `ERROR` is disabled, recorded as broken, and a
   notification is shown (`--no-verify` skips).
7. Cleanup: restore managed keys that disabled extensions rewrote while shutting down; reset keys
   outside the baseline that equal their default.
8. Append one line to `apply.log`; update `state.json` and `current`.

The extension set is: (baseline extensions + extensions the user enabled by hand) minus every
extension any theme lists, plus the `extensions.txt` of the theme providing look, plus the permanent
`desk-bridge@desktop-themes-rs`, minus extensions recorded as broken.

If GNOME's crash protection is active (`disable-user-extensions=true`), apply reports "applied, but
extensions are disabled by GNOME" instead of "applied live" ([doctor](doctor.md)).

### Inheritance

`INHERITS=<id>` in `theme.conf` makes a child that takes every file it lacks from the parent,
recursively up to 8 levels. Rules: `*.dconf` by file name (a child's `settings.dconf` replaces the
parent's whole `settings.dconf`); `extensions.txt` lines `+uuid` add and `-uuid` remove, any plain
`uuid` line makes the child list everything itself; `gtk4/`, `gtk3/`, `flatpak.conf`, `apps/hooks/*`
come from the first theme in the chain that has them (directories are not merged); `PREVIEW`
inherited, `NAME`/`DESCRIPTION`/`INHERITS` not. Cycles, missing parents and chains longer than 8 are
reported by `themes doctor` and the child then stands alone. Example: `macos-glass`.

### Reset

`themes reset` is `apply fedora`: every module returns to the baseline; additionally the settings of
theme extensions that are now disabled are removed (`--keep-extension-settings` keeps them), so the
look really returns to the baseline. `reset --only <modules>` resets only those modules. The window has
the same action («Скинути до стандартного вигляду»), with a confirmation.

### Snapshot and delete

* `themes snapshot <id> [--name N] [--force]` saves the current look as a theme laid out by module;
  refuses to overwrite an existing id without `--force`; warns about things it could not save.
* `themes delete <id>` moves the directory to `trash/`; `fedora` and any theme that is the source of
  an active module cannot be deleted. The window asks for confirmation.

### Safety nets

Baseline (taken once, before the first apply), auto-rollback of failing extensions, GTK-symlink backups
(a user's own file displaced by a theme goes to `backups/` and returns when the theme is switched
off; `gtk3/bookmarks` is never touched), flatpak overrides recorded exactly and removed exactly,
`themes doctor`. See [principles](../principles.md#8-safety-never-break-the-session).

### Surfaces

| Surface | Behaviour |
|---|---|
| CLI | `list`, `current`, `apply [--only/--module/--dry-run/--no-verify]`, `reset`, `snapshot`, `delete`, `doctor`, `gui`; `--json` on all; errors as `Error: ...` on stderr (JSON: `{"ok":false,"error":...}`); exit 0 success, 1 error, 2 usage |
| Window | pages «Теми» (cards), «Змішати» (a module picker per theme, applies immediately), «Функції» (feature pages); the card menu applies a single module or deletes; toast with «Вийти з сесії» when a relogin is needed |
| D-Bus | `io.github.OlehDzhumyk.Themes`: `List`, `Current`, `Apply(theme, modules)`, `Doctor(fix)`, signal `Changed` |
| `--dry-run` | prints the keys that would be written and reset, the extension set and counts; **must not** change anything or write any file under the theme root |

## Settings

None in GSettings: the theme state is `state.json`. The skin and the clipboard shortcut are
managed keys of the look module (`io.github.OlehDzhumyk.Desk skin`, `.../clipboard toggle-shortcut`).
Environment: `DESKTOP_THEMES_ROOT`.

## Privacy & security

* `baseline/`, `state.json`, `apply.log`, `backups/`, `trash/` are personal; they never go in the
  repository and are not uploaded anywhere. No network access at apply time.
* Themes are data plus optional executable hooks (`apps/hooks/apply|revert`). **A hook is arbitrary
  code with the user's rights**; the window must show a hook's presence before a third-party theme is
  applied, and the manager never downloads themes.
* Third-party assets are fetched by the install scripts from pinned upstream revisions only.

## Edge cases

* Applying the theme that is already active re-applies it (repairs drift) and is not an error.
* Applying while GNOME Shell is not running (TTY, early login): dconf and files are written, the
  extension step is skipped and reported; doctor finishes the job at next login.
* An extension listed by a theme but not installed: apply continues, `doctor` warns.
* An extension installed after login cannot be loaded live: report "relogin required" (the toast
  offers «Вийти з сесії»), do not error.
* Two front ends apply simultaneously: the second waits for the lock, then applies on top.
* `PREVIEW` missing or unreadable: the card shows a placeholder, nothing fails.
* `@DEFAULT_PTYXIS_PROFILE@` unresolved (Ptyxis never ran): those keys are skipped, the rest of the
  module applies.
* Hook not executable or failing: warning, apply continues.
* Light/dark: `fedora` sets `color-scheme='prefer-dark'` through its `settings.dconf`; bundled
  themes do not otherwise force a scheme, and `macos` ships a GTK theme with `gtk-dark.css` so
  GTK apps follow the toggle (the shell theme is always dark).

## Accessibility

CLI output is plain text with `--json` alternative. The window is built from libadwaita widgets
(keyboard and screen-reader accessible); theme cards have accessible names that include the active
modules. Switching never relies on colour alone ("активна" badge text).

## Performance

`themes list` <= 50 ms; `themes apply X --no-verify` <= 3 s; the window opens without blocking on
`doctor` or on the shell.

## Gaps found

* **G-THEMES-1** Saving a snapshot does not include the skin and the clipboard shortcut:
  `SNAPSHOT_PREFIXES` in `crates/themes-core/src/snapshot.rs` has no `/io/github/OlehDzhumyk/Desk/`,
  so a snapshot theme falls back to the default skin. Intended: include the `Desk` keys in the look
  module's `desk.dconf`.
* **G-THEMES-2** There is no `EXPERIMENTAL`/`WARNING` marker: `macos-glass` is flagged only by words in its
  description, and the CLI, the window and quick settings apply it with one click. Intended: mandatory
  confirmation, see [liquid-glass](liquid-glass.md).
* **G-THEMES-3** *(resolved)* `themes/macos/spotlight.dconf` had a stale comment about `behavior.dconf`.
* **G-THEMES-4** `themes apply` exits 0 even when the auto-rollback disabled an extension; only the
  warning and the JSON field `rolled_back` tell. Scripts must read `rolled_back`; consider a distinct
  exit code.
* **G-THEMES-5** `themes doctor` (no `--fix`) writes `state.json` (`gnome_version`, `last_doctor`,
  `last_shell_crash`); a "check" that mutates state surprises users who diff it.

## Acceptance criteria

* AC-THEMES-01: Given the baseline exists, When `themes apply macos` and then `themes reset` run, Then
  every managed key equals its baseline value (or is unset if it was unset) and `themes current`
  reports all modules from `fedora`.
* AC-THEMES-02: Given `macos` is active, When `themes apply windows11 --only behavior` runs, Then
  `themes current` reports look and sounds and apps from `macos` and behavior from `windows11`, and
  the skin is still `macos`.
* AC-THEMES-03: Given any installed theme, When `themes apply <id> --dry-run` runs, Then the exit code
  is 0 and dconf, `state.json`, GTK links and the enabled extensions are byte-identical afterwards.
* AC-THEMES-04: Given `macos-glass`, When its files are listed, Then only `theme.conf`, `glass.dconf`
  and `extensions.txt` exist and every other module file resolves from `macos`.
* AC-THEMES-05: Given a child with `extensions.txt` containing `-spotlight@nin`, When it is applied,
  Then the parent's list minus that uuid plus `desk-bridge@desktop-themes-rs` is enabled.
* AC-THEMES-06: Given a theme `INHERITS` itself or a missing id, When `themes doctor` runs, Then it
  reports an `inherits` warning and the theme still applies as a stand-alone theme.
* AC-THEMES-07: Given an extension that errors after enabling, When `themes apply` finishes, Then
  within ~6 s it is disabled, recorded as broken, a notification is shown, and the JSON `rolled_back`
  contains its uuid.
* AC-THEMES-08: Given two `themes apply` processes started at once, When both finish, Then the
  final state equals the later one's selection and `apply.log` has two complete lines.
* AC-THEMES-09: Given `fedora`, When `themes delete fedora` runs, Then it fails with an error and
  nothing moves.
* AC-THEMES-10: Given a snapshot `mine` taken while the Windows 11 skin is active, When `mine` is
  applied later, Then the skin is `windows11` and the clipboard shortcut is `Super+V` (fails today,
  G-THEMES-1).
* AC-THEMES-11: Given a theme file with `$HOME` in `picture-uri`, When it is applied, Then the stored
  value contains the percent-encoded real home directory and no `$HOME`.
* AC-THEMES-12: Given `themes list --json`, When parsed, Then every object has the fields listed above
  and `themes list` takes <= 50 ms (release build).

## Open questions

1. *(Decided)* Experimental themes (`EXPERIMENTAL=true`, `WARNING="..."` in `theme.conf`) stay bundled but need explicit confirmation before apply: CLI `y/N` (`--yes` skips), GUI dialog, quick-settings dialog ([liquid-glass](liquid-glass.md)).
2. Distinct exit code when rollback happened (G-THEMES-4)?
3. Should third-party themes with hooks need an explicit "trust" step stored in `state.json`?
4. Should `reset` offer to keep the user's wallpaper (a frequent ask)?
