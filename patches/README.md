# Patches for third-party extensions

This directory holds **only unified diffs**, never a copy of a third-party extension.
`scripts/install-extensions.sh` downloads the pinned upstream build from extensions.gnome.org and
applies the diffs with `git apply`; a diff that no longer applies (for example because upstream fixed
the problem) is skipped with a warning.

## spotlight@nin

Upstream: <https://github.com/itsnin/spotlight> by itsnin, licence GPL-3.0-or-later. Pinned build:
extensions.gnome.org version 15 (`2026.15`). These diffs are derivative works and are provided under
the same licence. All credit for the extension goes to its author.

| Patch | Why |
|---|---|
| `0001-wait-for-startup-complete-before-stealing-overview-search.patch` | Stealing the overview search widgets while GNOME Shell 51 is still in its startup animation crashed the shell; wait for `startup-complete` first. |
| `0002-use-orientation-instead-of-vertical-for-st-boxlayout.patch` | `St.BoxLayout`'s `vertical` property no longer works on Shell 51; use `orientation`. |

Apply by hand:

```sh
cd ~/.local/share/gnome-shell/extensions/spotlight@nin
for p in /path/to/desktop-themes-rs/patches/spotlight@nin/*.patch; do git apply -p1 "$p"; done
```
