# Theme contract

A theme is a directory `themes/<id>/` under the theme root. `<id>` uses lower-case Latin letters,
digits, `-`, `_` or `.`, starts with a letter or digit, and is at most 64 characters. Files the
manager does not know (README, tools/, ...) are ignored.

| File | Module | Purpose |
|---|---|---|
| `theme.conf` | - | `NAME="..."`, `DESCRIPTION="..."`, `PREVIEW="path"`, optional `PREVIEW_DARK="path"`, optional `INHERITS=<theme id>` |
| `desk.dconf` | look | skin of the shell bridge and the clipboard shortcut (see "Skin" below) |
| `settings.dconf`, `extensions.dconf`, any other `*.dconf` not listed below | look | keys in `dconf dump` format |
| `tokens.toml` | look (derived keys go to look, sounds, apps) | design tokens: colours, radii, fonts, terminal, sounds, ...; generates libadwaita CSS, a shell layer, a Ptyxis palette, dconf keys (see [design-tokens.md](design-tokens.md)) |
| `extensions.txt` | look | UUIDs of extensions the theme enables, one per line (`#` comments) |
| `gtk4/`, `gtk3/` | look | files linked into `~/.config/gtk-4.0/` and `~/.config/gtk-3.0/` (`gtk3/bookmarks` is never touched); a `gtk4/` generated from `tokens.toml` is copied instead |
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

## Skin (shell bridge UI variant)

The bridge extension `desk-bridge@desktop-themes-rs` draws the shell-side UI of desktop features
(clipboard history, later more) in one of three variants. A theme picks one with a dconf key of the
bridge's GSettings schema `io.github.OlehDzhumyk.Desk`, in its look module (`desk.dconf` by
convention):

```ini
[io/github/OlehDzhumyk/Desk]
skin='windows11'                       # 'default' | 'macos' | 'windows11'

[io/github/OlehDzhumyk/Desk/clipboard]
toggle-shortcut=['<Super>v']           # shortcut that opens the clipboard panel
```

| Skin | Clipboard UI |
|---|---|
| `default` | compact list popup at the pointer + «Буфер обміну» toggle in quick settings |
| `macos` | Spotlight-like centred panel: search field, list, preview, keyboard hints |
| `windows11` | Win+V flyout next to the text cursor: tabs, cards with pin and «…», «Очистити все» |

A key rather than a separate file was chosen because it rides on the existing machinery: it is a
managed key of the look module (switching to a theme without `desk.dconf` returns it to `default`),
it is inherited, mixing modules works, and the bridge reacts to the change live (`changed::skin`).
The stylesheet classes are `.desk-skin-<skin>` (bridge/static/stylesheet.css).

## Inheritance (`INHERITS`)

`INHERITS=<id>` in `theme.conf` makes a theme a child of another installed theme. Every file the
child does not have comes from the parent (recursively, up to 8 levels; cycles and missing parents
are reported by `themes doctor` and the child then stands alone):

* `*.dconf`: by file name: a child's `settings.dconf` replaces the parent's `settings.dconf` as a
  whole; files only the parent has are used as they are. This works per module (look, behavior,
  sounds, apps).
* `extensions.txt`: lines `+uuid` add to the parent's list, `-uuid` remove from it. If the child's
  file has any plain `uuid` line, the parent's list is not used (the child lists everything itself);
  `+`/`-` lines still apply on top. Without the file the parent's list is used.
* `tokens.toml`: deep-merged along the chain (a child's key replaces the parent's, tables merge,
  arrays are replaced).
* `gtk4/`, `gtk3/`, `flatpak.conf`, `apps/hooks/*`: the first theme in the chain that has them
  (directories are not merged). An inherited hook runs in its own theme's directory.
* `theme.conf`: `PREVIEW`/`PREVIEW_DARK` and unknown keys are inherited; `NAME`, `DESCRIPTION` and
  `INHERITS` are not.

`macos-glass` is `INHERITS=macos` plus `glass.dconf` and `+liquid-glass@...`.

## Generated, not committed

`themes/macos/gtk4/` and `themes/windows11/gtk4/` (libadwaita CSS and assets derived from WhiteSur
and Fluent) are produced by `scripts/install-assets.sh` inside the installed theme root and are
git-ignored. Files generated from `tokens.toml` live in `~/.cache/desktop-themes/<id>/` and are
never written into a theme directory. Hand-made files win over generated ones: a `gtk4/` directory
or a `user-theme name` key in the theme's dconf files switches the matching generator off.
