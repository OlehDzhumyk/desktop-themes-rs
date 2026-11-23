# Architecture

## Components

```
  themes CLI ----+                                  +--> dconf (via the `dconf` tool)
  themes gui ----+--> themes-core (library) --------+--> GNOME Shell (org.gnome.Shell.Extensions)
  themes service-+    plan -> apply -> verify       +--> ~/.config/gtk-{3,4}.0, flatpak overrides
  (D-Bus, on demand)
        ^                                         deskd (session daemon, D-Bus activated)
        | io.github.OlehDzhumyk.Themes            io.github.OlehDzhumyk.Desk[.Clipboard]
        |                                               ^   history, pins, privacy, storage
  +-----+-----------------------------------------------+------+
  | desk-bridge@desktop-themes-rs (GNOME Shell, TypeScript)     |
  |  quick settings «Тема» · clipboard capture (Meta.Selection) |
  |  skinned panels (default / macos / windows11) · paste       |
  +-------------------------------------------------------------+
        GSettings io.github.OlehDzhumyk.Desk (skin, clipboard settings) — one place for all
```

The repository is a Cargo workspace:

| Crate | Kind | Role |
|---|---|---|
| `crates/themes-core` | library | the theme manager: theme contract (incl. inheritance), modules, plan/apply, dconf, doctor, crash detection |
| `crates/themes` | binary `themes` | CLI (`clap`), GTK 4 / libadwaita window («Теми»: themes, mix, «Функції»), theme D-Bus service (`--dbus-service`) |
| `crates/desk-proto` | library | shared D-Bus names, interface XML (`data/dbus/`), the `ClipItem` type and a zbus client proxy |
| `crates/desk-settings` | library | the settings registry (one source for the GSettings schema, typed accessors, the window's preference pages and docs/settings.md; `cargo xtask gen`) and the gettext helpers (`tr!`) |
| `crates/deskd` | binary `deskd` | desktop features daemon: hosts engines (clipboard history, volume mixer over PipeWire; cargo feature `pipewire`, on by default) |
| `bridge/` | npm project | the GNOME Shell extension `desk-bridge@desktop-themes-rs`, TypeScript compiled with `tsc` (TypeScript 7, native) to ES2025 modules for gjs 1.90 / SpiderMonkey 140, strict flags incl. `noUncheckedIndexedAccess`, linted with typescript-eslint `strictTypeChecked` (types from `@girs/gnome-shell` 51 / ts-for-gir) |

`themes` with no arguments opens the window; `themes --dbus-service` is what the bus activates;
`theme` and `themes-gui` are symlinks to the same binary.

**Where the theme service lives.** The theme D-Bus service stays in the `themes` binary for now: it
shares the lock, `state.json` and the apply code with the CLI and the window, it runs only on demand
and exits after 90 s idle, whereas deskd is a long-running session daemon that must stay small and
never block on a theme switch (which can take seconds and talks to the shell). The bridge already
speaks to both over D-Bus, so moving the service into deskd later changes no client.

**Logic in deskd, UI in the shell.** GNOME's mutter has no `wlr-data-control`, so only the shell can
watch the clipboard. The bridge listens to `Meta.Selection::owner-changed`, reads the content with
`St.Clipboard` and hands it to deskd (`Push`); deskd decides what to keep (privacy rules, sizes,
duplicates) and stores it. Panels ask deskd for items (`List`) and paste through `Activate` + a
synthesized Shift+Insert. Everything the user configures is in GSettings and is read by deskd,
the bridge and the window alike: there is no second settings dialog.

**Settings schema.** `data/io.github.OlehDzhumyk.Desk.gschema.xml` is compiled into
`~/.local/share/glib-2.0/schemas` (GLib searches the user data dir, so deskd, the window and
`gsettings` find it without environment variables); the bridge ships a compiled copy in its own
`schemas/` and prefers it, so the shell never depends on the user schema dir being current.

| Source file (themes-core) | Role |
|---|---|
| `theme.rs`, `modules.rs` | read a theme directory, split it into the four modules, expand `~`/`$HOME`/`${HOME}` and the Ptyxis profile placeholder |
| `plan.rs` | pure computation: managed keys per module, desired dconf values, extension set (unit tested) |
| `apply.rs` | execute a plan: write dconf, link GTK files, flatpak overrides, hooks, toggle extensions live, post-apply cleanup, auto-rollback |
| `dconf.rs` | `dconf dump` text format parser/writer and thin wrappers around the `dconf` tool |
| `gtklinks.rs` | symlinks into `~/.config/gtk-4.0` / `gtk-3.0`, backups of displaced user files |
| `flatpak.rs` | records exactly which `flatpak override --user` arguments a theme added, removes only those |
| `hooks.rs` | `apps/hooks/apply` and `revert` executables |
| `shell.rs` | D-Bus talk to GNOME Shell (extension states, enable/disable), notifications |
| `doctor.rs` | `themes doctor [--fix]`: broken extensions, journal errors, missing themes/icons/files, GNOME version change, GSettings schemas not compiled |
| `snapshot.rs` | "save current look as a theme": lays the current settings out by module |
| `state.rs`, `lock.rs` | `state.json` (what was applied) and the `.lock` file shared by CLI, GUI and service |
| `crash.rs` | GNOME's crash protection (`disable-user-extensions`), last real shell crashes, culprit theme/extensions |
| `themes/src/service.rs` | D-Bus service `io.github.OlehDzhumyk.Themes` (exits after 90 s idle) |
| `themes/src/gui/` | the window: theme cards, the mix page, «Функції» (feature settings, credits from CREDITS.md), crash banner |

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

Installed by `data/install.sh`: `~/.local/bin/themes` (+ `theme`, `themes-gui`) and
`~/.local/bin/deskd`, the `.desktop` file, the D-Bus activation files
`~/.local/share/dbus-1/services/io.github.OlehDzhumyk.{Themes,Desk}.service`, the systemd user units
`themes-doctor.service` (runs `themes doctor --fix` after login) and `deskd.service`, the GSettings
schema of deskd, the extensions from `extensions/` and the built bridge
(`bridge/build/desk-bridge@desktop-themes-rs`).

deskd keeps the optional per-application volumes of the mixer in `~/.local/share/deskd/mixer/apps.json`
(0600) and the clipboard history in `~/.local/share/deskd/clipboard/` (`history.json`, `images/`,
mode 0600/0700; text only unless «persist images» is on) and session-only images in
`$XDG_RUNTIME_DIR/deskd/clipboard/`. These are personal and never part of the repository.

Upgrading from an older private build with other ids (`io.github.<name>.Themes`,
`themes-quick@<host>.local`, `lockscreen-layout@<host>.local`): `scripts/migrate-from-legacy.sh
[--dry-run] [--reapply]` (idempotent; keeps state, baseline and backups; see the script header).

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

### deskd

Service `io.github.OlehDzhumyk.Desk` (activation file → systemd user unit `deskd.service`).
Interface XML: `data/dbus/` (a unit test checks the zbus implementation against it; the bridge
loads the same files at runtime).

`/io/github/OlehDzhumyk/Desk`, interface `io.github.OlehDzhumyk.Desk`: properties `Version s`,
`Features as`.

`/io/github/OlehDzhumyk/Desk/Clipboard`, interface `io.github.OlehDzhumyk.Desk.Clipboard`:

| Member | Description |
|---|---|
| `Push(s mime, ay data, as offered, s source_app) -> t id` | new clipboard content from the shell; 0 = not recorded |
| `List(s query, u limit) -> a(tssssbxt)` | (id, kind, text, mime, image path, pinned, time, size), newest first |
| `Get(t id) -> (s mime, ay data)` / `Activate(t id)` | content; `Activate` also makes it the newest |
| `Pin(t id, b pinned)`, `Delete(t id)`, `Clear(b keep_pinned)` | |
| property `Private b` (rw), `Enabled b`, `Count u`; signal `Changed()` | |

```sh
gdbus call --session -d io.github.OlehDzhumyk.Desk -o /io/github/OlehDzhumyk/Desk/Clipboard \
  -m io.github.OlehDzhumyk.Desk.Clipboard.List "" 5
```

`/io/github/OlehDzhumyk/Desk/Mixer`, interface `io.github.OlehDzhumyk.Desk.Mixer` (volumes on the cubic
slider scale, 1.0 = 100 %):

| Member | Description |
|---|---|
| `ListDevices(s kind) -> a(ussssdbb)` | `output` or `input`: (id, node name, description, icon, bus, volume, muted, default) |
| `ListStreams() -> a(ussssdbbu)` / `ListApps(b include_silent) -> a(ssssdbbuau)` | playback streams; one row per application (its streams merged) |
| `SetVolume(u id, d v)`, `SetMute(u id, b)` | a device or a stream |
| `SetAppVolume(s app, d v)`, `SetAppMute(s app, b)`, `MoveApp(s app, u device)`, `MoveStream(u, u)` | every stream of an application; device 0 = the default |
| `SetDefault(s kind, u id)` | default output/input; streams moved by hand follow it |
| property `Version u` (1), `Available b`, `DefaultOutput u`, `DefaultInput u`; signal `Changed(as what)` | coalesced: `devices`, `apps`, `defaults`, `available` |

The engine (`crates/deskd/src/mixer/`) keeps a PipeWire connection on its own thread and changes
nothing until asked; `model.rs` holds every rule and is tested with mocked graphs,
`tests/mixer_pipewire.rs` runs deskd against an isolated PipeWire and WirePlumber.

## Crash protection

When GNOME Shell crashes soon after start (or within 60 s of enabling extensions), GNOME sets
`org.gnome.shell disable-user-extensions=true`; `enabled-extensions` stays as it was but nothing
runs. `themes apply` reports this instead of "applied live", `themes doctor` shows it as an error
together with the last real (non-headless) shell crash from `coredumpctl`, the theme that was applied
then and the extensions that theme had just added (from `apply.log`), and records the crash in
`state.json` (`last_shell_crash`). The window shows a banner «GNOME вимкнув розширення після збою
оболонки» whose button offers to switch to the safe theme first or to re-enable extensions; nothing
re-enables them without the user's confirmation (`themes doctor --enable-extensions` on the CLI).

## Build environment

Silverblue has no compilers on the host. `scripts/lib.sh` runs build steps inside a toolbox
(`desktop-themes-build`, override with `DT_TOOLBOX`) that `scripts/bootstrap.sh` creates with these
packages: `git curl unzip sassc glib2-devel gtk-update-icon-cache gcc pkgconf-pkg-config gtk4-devel
libadwaita-devel pipewire-devel clang-devel rust cargo python3-numpy python3-scipy python3-soundfile python3-pillow nodejs npm gettext`
(npm builds the TypeScript bridge; its dev dependencies are pinned in `bridge/package-lock.json`).
The toolbox shares `$HOME`, so artifacts land in the real home. Rust 1.100 or newer is required
(`rust-version` in `Cargo.toml`: the release Fedora 45 ships); `bootstrap.sh --rustup` installs a toolchain with rustup if the
distribution's is older.

## Testing

* `cargo test`: unit tests (plan computation, dconf parsing, module split, path expansion, state).
* `tools/integration-test.sh`: applies a temporary theme with all modules, mixes macos/windows11
  and checks dconf against the baseline after reset. **It writes the real dconf**; use a VM or a
  spare session.
* `bridge/`: `npm run typecheck` (tsc --noEmit against the GNOME Shell 51 types), `npm run lint`. Two compilers on purpose: TypeScript 7 builds and type-checks, TypeScript 6 (package `typescript`) is only the compiler API typescript-eslint still needs; see the note in `bridge/package.json`.
* `tools/headless/`: a fully isolated headless GNOME Shell (see `env.sh`): own `dbus-daemon`, own
  Wayland socket, own `XDG_RUNTIME_DIR` subdirectory (so the shell's crash-protection file never
  lands in the real one), own `XDG_CONFIG_HOME`/dconf database, own `XDG_DATA_HOME` overlay (symlinks
  to the real data plus the extension under test, the schema and a deskd-only service directory).
  `start.sh --theme ID --ext DIR` loads a theme's dconf into the private database (never
  `themes apply`); `input.py` sends keys/clicks (Mutter RemoteDesktop) and takes screenshots (Mutter
  ScreenCast → PipeWire → gst); `stop.sh` kills only its own processes.
  `clipboard-test.sh THEME SHORTCUT OUTDIR` is the end-to-end clipboard test (copy three samples
  with wl-copy, open the panel, paste the 2nd into a GTK 4 text field, verify).
  Never call `toolbox` from inside `run.sh`: podman would see a different runtime dir.
