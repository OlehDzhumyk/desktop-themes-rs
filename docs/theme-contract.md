# Theme contract

A theme is a directory `themes/<id>/` under the theme root. `<id>` uses lower-case Latin letters,
digits, `-`, `_` or `.`, starts with a letter or digit, and is at most 64 characters. Files the
manager does not know (README, tools/, ...) are ignored.

| File | Module | Purpose |
|---|---|---|
| `theme.conf` | - | `NAME="..."`, `DESCRIPTION="..."`, `PREVIEW="path"`, optional `PREVIEW_DARK="path"` |
| `settings.dconf`, `extensions.dconf`, any other `*.dconf` not listed below | look | keys in `dconf dump` format |
| `extensions.txt` | look | UUIDs of extensions the theme enables, one per line (`#` comments) |
| `gtk4/`, `gtk3/` | look | files linked into `~/.config/gtk-4.0/` and `~/.config/gtk-3.0/` (`gtk3/bookmarks` is never touched) |
| `flatpak.conf` | look | one `flatpak override --user` argument set per line |
| `behavior.dconf` | behavior | shortcuts, Alt+Tab, touchpad, hot corners, mutter, font rendering |
| `sounds.dconf` | sounds | `org/gnome/desktop/sound` |
| `apps.dconf` | apps | per-application settings (Ptyxis, ...) |
| `apps/hooks/apply`, `apps/hooks/revert` | apps | executables for changes that are not dconf keys |

Which module a `*.dconf` file belongs to is decided by its file name (see `Module::for_dconf_file`
in `src/modules.rs`).

## dconf files

Standard `dconf dump` text: `[path/relative/to/root]` section headers, `key=GVariant text`.
Lines starting with `#` or `;` are comments. The list keys `enabled-extensions` and
`disabled-extensions` are ignored here: extensions are driven only by `extensions.txt`.

## Portable paths

Themes must not contain absolute home directories. The manager expands, at load time:

* `PREVIEW` and `PREVIEW_DARK` in `theme.conf`: a leading `~`, and `$HOME` / `${HOME}` anywhere.
* dconf string values: `$HOME` / `${HOME}` anywhere, and a leading `~/` inside the quotes.
  In `file://` URIs the home directory is percent-encoded; in plain strings quotes and
  backslashes are escaped.

```ini
picture-uri='file://${HOME}/.local/share/backgrounds/macos/WhiteSur-light.jpg'
```

```sh
PREVIEW="${HOME}/.local/share/backgrounds/macos/WhiteSur-light.jpg"
```

## Ptyxis profile placeholder

Ptyxis stores per-profile settings under `/org/gnome/Ptyxis/Profiles/<uuid>/`, and the UUID differs
per machine. In any `*.dconf` file write:

```ini
[org/gnome/Ptyxis/Profiles/@DEFAULT_PTYXIS_PROFILE@]
palette='Pro'
```

`@DEFAULT_PTYXIS_PROFILE@` is replaced by the value of `/org/gnome/Ptyxis/default-profile-uuid`.
If that key does not exist yet (Ptyxis never ran), those keys are skipped and the rest of the
module still applies.

## Hooks

`apps/hooks/apply` runs when the apps module switches to this theme, `apps/hooks/revert` when it
switches away. Environment: `THEME_ID`, `THEME_DIR`, `PREV_THEME_ID` (for `revert` also
`NEXT_THEME_ID`). Hooks must be executable.

## Aliases between themes

`macos-glass` reuses most of `macos` through relative symlinks (`apps.dconf -> ../macos/apps.dconf`,
...) and owns only `theme.conf`, `glass.dconf`, `extensions.txt`. `themes/macos-glass/sync-glass.sh`
re-creates the links and rebuilds `extensions.txt` after `macos` changes. Its `gtk4` entry points at
`../macos/gtk4`, which `scripts/install-assets.sh` generates.

## Generated, not committed

`themes/macos/gtk4/` and `themes/windows11/gtk4/` (libadwaita CSS and assets derived from WhiteSur
and Fluent) are produced by `scripts/install-assets.sh` inside the installed theme root and are
git-ignored.
