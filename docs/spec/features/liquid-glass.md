# Liquid Glass variant (macos-glass)

## Summary

`macos-glass` is the `macos` theme plus the **Liquid Glass** shell extension: refraction, highlights
and glass edges on the top panel, the dock, menus, quick settings, notifications and the OSD. It is
visually the closest thing to a recent macOS design that a GNOME session can get, and it is the only
bundled theme that is **Experimental**: it crashed GNOME Shell 51.1 on real hardware.

## Status

**Experimental** (stage 0). Origin badge: *Based on Liquid Glass extension (licence of the upstream
repository, to be recorded in CREDITS.md at integration)*; installed from its repository at a pinned
commit, not from extensions.gnome.org. The theme must not be the default of anything and must never be
applied without the user knowing.

## Reference behaviour

macOS 26 draws controls, the menu bar and the dock with a translucent, light-refracting material
("Liquid Glass"). GNOME Shell has only static blur (Blur my Shell); true refraction needs a shader that
redraws the background under each element.

## Our behaviour

### Composition

`macos-glass` owns only `theme.conf` (`INHERITS=macos`), `glass.dconf` and an `extensions.txt` with
`+liquid-glass@thinkingcoding1231.gmail.com`. Everything else (look, behaviour, sounds, apps, skin, lock
screen) is `macos`.

`glass.dconf` (`org/gnome/shell/extensions/liquid-glass`): `top-bar-style='pill'`; glass **on** for dock,
menus, quick settings, notifications, OSD; **off** for applications, launcher (Spotlight is the launcher), glass clock and desktop widgets;
menu animation off; `blur-my-shell-override=true` (Blur my Shell keeps working for the overview and
apps, and the extension disables its blur under glass elements).

### Known crash

On GNOME Shell 51.1 (real hardware) the shell crashed with SIGSEGV in
`st_theme_node_reduce_border_radius` from `st_theme_node_paint_borders` about 20 s after the theme was
applied, and again at the next start; GNOME then sets `disable-user-extensions=true` and no extension runs.
`themes doctor`, `themes apply` and the window detect this state and name the likely culprit
(`liquid-glass@...`) ([doctor](doctor.md)). The cause is believed to be an invalid border radius in a style
the extension injects, hit during painting (not confirmed).

### Required behaviour (target)

Because the failure mode is "the whole session loses its extensions", an Experimental theme must:

1. **Be marked** in `theme.conf` with `EXPERIMENTAL="reason text"` (new key), and shown with an
   "Experimental" badge in the window, `themes list` (a column/JSON field `experimental`), and the quick-settings
   menu.
2. **Need confirmation:** the window shows a dialog explaining the crash and the way back
   («Відкрити ризиковану тему?», buttons «Скасувати» (default) / «Застосувати»); the CLI requires `--yes`
   (without a TTY and without `--yes` it refuses); quick settings asks too.
3. **Be applied in the safest order**: `apply` first writes everything but the experimental extension, waits
   for the verification window, then enables it; on the first detection of a crash (the shell restarted
   within 60 s of enabling) the next login switches to `fedora` and notifies, *without* waiting for the user
   to read documentation. (Today GNOME disables extensions and waits for the user: see doctor.)
4. **Be skipped by `themes doctor --fix`'s re-enable logic** unless the GNOME version or the extension changed.
5. **Never be re-applied automatically** by any service.

### Performance notice

The refraction shader redraws the background under each glass element: noticeably higher GPU time than
blur. On battery prefer `macos`; the settings card says so. A "disable glass on battery" option is an open
question.

### Animation

The extension's own; `enable-menu-animation=false` in the theme. With `enable-animations=false` the glass
must not animate.

## Settings

The `liquid-glass` keys in `glass.dconf` (above). No hub page; the theme card carries the warning.

## Privacy & security

Local only; the extension is third-party code running inside the shell with full privileges: it is installed
from a pinned commit, and that pin is the trust boundary (review diffs when bumping it).

## Edge cases

* Applying `macos-glass` over `macos`: only the extra extension and its keys change.
* Applying it while GNOME has already disabled extensions: apply reports "extensions disabled by GNOME" and
  offers the safe path.
* Hot-plugging a monitor or a resolution change while the glass is active is untested.
* Fractional scaling: untested; a likely trigger for the border-radius crash class.

## Accessibility

Glass reduces contrast under text; the theme must keep text contrast >= 4.5:1 on the panel and menus (the
extension's blur-my-shell override keeps a darkening layer); when a high-contrast setting or
`reduce-transparency` becomes available, glass must turn itself off. Motion is reduced as above.

## Performance

Budgets that apply: shell memory delta of the bridge unaffected; GPU: no frame drops on the reference
laptop in the overview or in menu open (not measured today).

## Gaps found

* **G-GLASS-1** No `EXPERIMENTAL` marker exists in the code: `theme.conf` has only prose in
  `DESCRIPTION`; nothing in `crates/themes/src/cli.rs`, `gui/card.rs` or `bridge/src/quickSettings.ts`
  distinguishes it (grep for "експериментальна" finds only the theme file).
* **G-GLASS-2** There is no confirmation in any front end ([quick-settings-theme](quick-settings-theme.md)
  G-QS-1).
* **G-GLASS-3** Crash detection is reactive (after GNOME disables extensions); there is no proactive
  "crashed within 60 s of applying X" auto-switch to `fedora`.
* **G-GLASS-4** The licence and the pinned commit of the extension are not yet in CREDITS.md in a form
  this spec can cite; check before the next release.

## Acceptance criteria

* AC-GLASS-01: Given `macos-glass`, When the module files are resolved, Then the look module equals
  `macos`'s plus `glass.dconf` and one added extension.
* AC-GLASS-02: Given an Experimental theme, When `themes apply <id>` runs without `--yes` on a terminal,
  Then it asks for confirmation; without a TTY it exits non-zero and changes nothing (target).
* AC-GLASS-03: Given the window, When an Experimental theme's card is activated, Then the dialog appears and
  «Скасувати» leaves everything unchanged (target).
* AC-GLASS-04: Given `themes list --json`, Then `macos-glass` has `"experimental": true` (target).
* AC-GLASS-05: Given the shell crashed within 60 s of applying an Experimental theme, When the user next logs
  in, Then `fedora` is active and a notification explains (target).
* AC-GLASS-06: Given `disable-user-extensions=true` after such a crash, When `themes doctor` runs, Then it names
  `macos-glass` and `liquid-glass@...` (implemented).
* AC-GLASS-07: Given `macos-glass` is active and `themes apply macos` runs, Then `liquid-glass@...` is
  disabled and its keys are reset to the baseline.

## Open questions

1. Keep `macos-glass` in the repository's bundled list while it can crash the shell, or move it to an opt-in
   `themes-experimental/` directory the installer does not copy by default?
2. Is the crash reproducible in the headless shell (software rendering) so a regression test can exist?
3. Disable glass automatically on battery or in power-saver mode?
4. Report upstream with a stack trace (principle 5)?
