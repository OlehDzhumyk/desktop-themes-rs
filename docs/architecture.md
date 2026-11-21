# Architecture

## Components

```
                  +-------------------------------------------+
  themes CLI ---->|                                           |
  themes gui ---->|   libthemes (src/lib.rs, pure Rust)       |----> dconf (via the `dconf` tool)
  D-Bus service ->|   plan -> apply -> verify -> cleanup      |----> GNOME Shell (org.gnome.Shell.Extensions)
                  +-------------------------------------------+----> ~/.config/gtk-{3,4}.0 (symlinks)
                          ^                                    ----> flatpak overrides, notifications
                          |
   quick-settings extension (extensions/themes-quick@...) --- D-Bus ---> service
```

One binary (`themes`) contains the library, the CLI (`clap`), the GTK 4 / libadwaita window and the
D-Bus service. `themes` with no arguments opens the window; `themes --dbus-service` is what the bus
activates; `theme` and `themes-gui` are symlinks to the same binary.

| Source file | Role |
|---|---|
| `theme.rs`, `modules.rs` | read a theme directory, split it into the four modules, expand `~`/`$HOME`/`${HOME}` and the Ptyxis profile placeholder |
| `plan.rs` | pure computation: managed keys per module, desired dconf values, extension set (unit tested) |
| `apply.rs` | execute a plan: write dconf, link GTK files, flatpak overrides, hooks, toggle extensions live, post-apply cleanup, auto-rollback |
| `dconf.rs` | `dconf dump` text format parser/writer and thin wrappers around the `dconf` tool |
| `gtklinks.rs` | symlinks into `~/.config/gtk-4.0` / `gtk-3.0`, backups of displaced user files |
| `flatpak.rs` | records exactly which `flatpak override --user` arguments a theme added, removes only those |
| `hooks.rs` | `apps/hooks/apply` and `revert` executables |
| `shell.rs` | D-Bus talk to GNOME Shell (extension states, enable/disable), notifications |
| `doctor.rs` | `themes doctor [--fix]`: broken extensions, journal errors, missing themes/icons/files, GNOME version change |
| `snapshot.rs` | "save current look as a theme": lays the current settings out by module |
| `state.rs`, `lock.rs` | `state.json` (what was applied) and the `.lock` file shared by CLI, GUI and service |
| `service.rs` | D-Bus service `io.github.OlehDzhumyk.Themes` (exits after 90 s idle) |
| `gui/` | the window: theme cards, the mix page, menus, dialogs |

## Files and directories

Everything lives under the **theme root**, `~/.local/share/desktop-themes` (override with
`DESKTOP_THEMES_ROOT`):

```
themes/<id>/        the themes (copied from this repository's themes/, plus your own snapshots)
baseline -> baseline-<timestamp>/full.dconf   `dconf dump /` taken before any theme was applied
state.json          which theme each module comes from, keys written, broken extensions, GNOME version
current             the theme that provides the "look" module
apply.log           journal
backups/            files of yours that a theme's GTK symlinks displaced
trash/              deleted themes
.lock               flock shared by all front ends
```

`baseline/`, `state.json`, `apply.log`, `backups/` and `trash/` are personal and are never part of
this repository (`.gitignore` excludes them).

Installed by `data/install.sh`: `~/.local/bin/themes` (+ `theme`, `themes-gui`), the `.desktop` file,
the D-Bus activation file `~/.local/share/dbus-1/services/io.github.OlehDzhumyk.Themes.service`, the
systemd user unit `themes-doctor.service` (runs `themes doctor --fix` after login) and the
extensions from `extensions/`.

## Applying a theme

1. Read the baseline, the current dconf dump, the saved state and the selected source for every
   module.
2. Compute a plan (`plan.rs`): for each affected module the **managed keys** (keys any theme sets in
   that module plus keys written last time), their desired values (the selected theme's value, else
   the baseline value, else the schema default), and the extension list.
3. Write the dconf keys first; GTK files are linked, flatpak overrides and hooks run.
4. About a second later, change `enabled-extensions` (GNOME Shell watches it and loads/unloads
   extensions live). Changing `org.gnome.desktop.interface` keys at the same moment as an extension
   being disabled was observed to crash GNOME Shell 51.1, hence the delay.
5. After ~5 s verify the extensions; any in `ERROR` state is disabled and recorded as broken, with a
   notification (`--no-verify` skips this).
6. Cleanup: restore managed keys that disabled extensions rewrote while shutting down, reset keys
   that are outside the baseline and equal to their default.

A full `themes reset` additionally removes the settings of theme extensions that are now disabled
(`--keep-extension-settings` keeps them).

## D-Bus API

Service `io.github.OlehDzhumyk.Themes`, object `/io/github/OlehDzhumyk/Themes`:

| Member | Description |
|---|---|
| `List() -> a(sssas)` | id, name, description, modules |
| `Current() -> a{ss}` | module -> theme id |
| `Apply(s theme, as modules) -> (b relogin, s message, as warnings)` | empty list = all modules |
| `Doctor(b fix) -> (b ok, as messages, s json)` | system check |
| signal `Changed(a{ss})` | the selection changed (also after CLI or GUI changes) |

```sh
gdbus call --session -d io.github.OlehDzhumyk.Themes -o /io/github/OlehDzhumyk/Themes \
  -m io.github.OlehDzhumyk.Themes.Apply windows11 '["behavior"]'
```

The window additionally exposes a small `...Themes.Window` interface (used to raise an existing
window instead of starting a second one).

## Build environment

Silverblue has no compilers on the host. `scripts/lib.sh` runs build steps inside a toolbox
(`desktop-themes-build`, override with `DT_TOOLBOX`) that `scripts/bootstrap.sh` creates with these
packages: `git curl unzip sassc glib2-devel gtk-update-icon-cache gcc pkgconf-pkg-config gtk4-devel
libadwaita-devel rust cargo python3-numpy python3-scipy python3-soundfile python3-pillow`.
The toolbox shares `$HOME`, so artifacts land in the real home. Rust 1.88 or newer is required
(edition 2024, let chains); `bootstrap.sh --rustup` installs a toolchain with rustup if the
distribution's is older.

## Testing

* `cargo test`: unit tests (plan computation, dconf parsing, module split, path expansion, state).
* `tools/integration-test.sh`: applies a temporary theme with all modules, mixes macos/windows11
  and checks dconf against the baseline after reset. **It writes the real dconf**; use a VM or a
  spare session.
* `tools/headless/`: a private headless GNOME Shell (own `dbus-daemon`, own Wayland socket) with
  `screenshot.py` (Mutter ScreenCast) and `click.py` (Mutter RemoteDesktop) so extension and theme
  changes can be tried and photographed without touching the real session.
