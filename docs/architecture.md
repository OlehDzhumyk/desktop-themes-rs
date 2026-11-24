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
| `crates/desk-capture` | library | the screenshot core, pure Rust with no desktop dependency (ADR 0011): PNG checks with a 100 MP cap, removal of text/time metadata, collision-safe atomic saving, and destructive redaction, crop and annotations rendered into RGBA |
| `crates/deskd` | binary `deskd` | desktop features daemon: hosts engines (clipboard history, automatic light/dark appearance, emoji and symbol picker, screenshots, focus modes, volume mixer over PipeWire; cargo feature `pipewire`, on by default) |
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
| `apply.rs` | execute a plan: write dconf, link GTK files, flatpak overrides, hooks, toggle extensions live, post-apply cleanup, auto-rollback; refuses an experimental theme without `Options::confirmed` |
| `txn.rs` | intent journal of `apply` (`state.json` `pending`): rollback on error, recovery after a crash |
| `dconf.rs` | `dconf dump` text format parser/writer, wrappers around the `dconf` tool, batched resets through the dconf writer; `Backend` (the tool, or an in-memory database in unit tests) |
| `gtklinks.rs` | symlinks of hand-made GTK files into `~/.config/gtk-4.0` / `gtk-3.0`; generated `gtk.css` is copied instead (Flatpak apps cannot follow links into `~/.cache`); backups of displaced user files |
| `flatpak.rs` | records exactly which `flatpak override --user` arguments a theme added, removes only those |
| `hooks.rs` | `apps/hooks/apply` and `revert` executables (time limit, bounded output) |
| `shell.rs` | D-Bus talk to GNOME Shell (extension states, enable/disable), notifications |
| `doctor.rs` | `themes doctor [--fix]`: broken extensions, journal errors, missing themes/icons/files, GNOME version change, GSettings schemas not compiled |
| `snapshot.rs` | "save current look as a theme": lays the current settings out by module |
| `state.rs`, `lock.rs` | `state.json` (what was applied) and the `.lock` file shared by CLI, GUI and service |
| `crash.rs` | GNOME's crash protection (`disable-user-extensions`), last real shell crashes, culprit theme/extensions |
| `themes/src/service.rs` | D-Bus service `io.github.OlehDzhumyk.Themes` (blocking work on a thread pool; exits after 90 s idle or when the bus closes) |
| `themes/src/gui/` | the window: theme cards, the mix page, «Функції» (feature settings, credits from CREDITS.md), crash banner |

**Long-term direction.** The layers above are meant to stay DE-agnostic in the core, with desktop
specifics behind a backend boundary and thin compositor adapters (GNOME Shell today, a native Rust
`wlr-layer-shell` client on KDE and wlroots later). The target design, and what it does not change
yet, is in [ADR 0011](adr/0011-wayland-agnostic-rust-core.md).

## Files and directories

Everything lives under the **theme root**, `~/.local/share/desktop-themes` (override with
`DESKTOP_THEMES_ROOT`):

```
themes/<id>/        the themes (copied from this repository's themes/, plus your own snapshots)
baseline -> baseline-<timestamp>/full.dconf   `dconf dump /` taken before any theme was applied
state.json          which theme each module comes from, keys written, broken extensions, GNOME version,
                    and while an apply runs its journal (`pending`)
current             the theme that provides the "look" module
apply.log           journal
backups/            files of yours that a theme's GTK links or copies displaced
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
   module. An experimental theme is refused unless the caller passes the user's confirmation
   (ADR 0006). An earlier apply that was interrupted is finished first (see below).
2. Compute a plan (`plan.rs`): for each affected module the **managed keys** (keys any theme sets in
   that module plus keys written last time), their desired values (the selected theme's value, else
   the baseline value, else the schema default), and the extension list.
3. Record the intent in `state.json` (`pending`: the selection to commit, the value of every key
   that may change, the steps begun) and save it before each step: token links, dconf keys,
   extension list, GTK files, flatpak overrides, hooks. A GTK backup is recorded before the user's
   file is moved, and every link or copy before it is made.
4. Write the dconf keys first. Then wait until GNOME Shell has processed them, and only then
   change `enabled-extensions` (the shell loads and unloads extensions live). See "GNOME Shell 51.1
   workaround" below.
5. GTK files are linked or copied, flatpak overrides added, hooks run. Then the new selection, its
   managed keys and the theme's extensions are committed with one save and `pending` is removed.
6. After ~5 s verify the extensions; any in `ERROR` state is disabled and recorded as broken, with a
   notification (`--no-verify` skips this).
7. Cleanup: restore managed keys that disabled extensions rewrote while shutting down, reset keys
   that are outside the baseline and equal to their default.

A full `themes reset` additionally removes the settings of theme extensions that are now disabled
(`--keep-extension-settings` keeps them).

**Errors and crashes.** When a step fails, the steps begun so far are undone in reverse order
(flatpak overrides, GTK files with the previous look relinked, dconf values, token links) and the
old selection stays; the error says what could not be undone. When the process dies half way,
`pending` stays in `state.json`: the next `apply` and `themes doctor --fix` (which runs after
login) finish it, and `themes doctor` reports it. Steps before the hooks are rolled back; once the
hooks began, the apply is completed instead, because a hook's effects cannot be undone.

**Hooks** get a time limit (`DESKTOP_THEMES_HOOK_TIMEOUT` seconds, 30 by default; then their
process group is killed) and only the first 64 KiB of their output is kept. A failing hook is a
warning, not an error.

**GNOME Shell 51.1 workaround.** GNOME Shell 51.1 crashed (SIGSEGV in gnome-desktop's
`update_clock`) when a change notification of `org.gnome.desktop.interface` arrived while an
extension with a clock was being disabled. So the apply subscribes to the dconf writer's `Notify`
signal before it writes, waits for the notification that covers its keys (the bus has then queued
it for the shell as well), and makes one call that the shell answers from its main loop, after
that notification. Only then is the extension list changed. The wait has an upper bound of 3 s;
`apply.log` records `shell-settle=ok` or `shell-settle=timeout`. Extension states are then read on
`ExtensionStateChanged` (no polling), and before the cleanup a shell call plus an empty change to
the dconf writer make sure the writes of extensions that shut down have reached dconf. No upstream
report is linked yet. The wait can be removed when the oldest supported GNOME Shell no longer
crashes without it: drop it and run `just e2e -k many_times` repeatedly on that shell.

## D-Bus API

Service `io.github.OlehDzhumyk.Themes`, object `/io/github/OlehDzhumyk/Themes`:

| Member | Description |
|---|---|
| `List() -> a(sssas)` | id, name, description, modules |
| `Experimental() -> a{ss}` | experimental themes: id -> warning text |
| `PendingCommands(s theme, as modules) -> a(ss)` | unapproved dconf keys that run commands (S-5): `(key, value)` |
| `Current() -> a{ss}` | module -> theme id |
| `Apply(s theme, as modules) -> (b relogin, s message, as warnings)` | empty list = all modules; experimental themes and unapproved command keys are refused (`AccessDenied`) |
| `ApplyConfirmed(s theme, as modules) -> (b relogin, s message, as warnings)` | the same after the user confirmed an experimental theme (ADR 0006) and/or command keys (S-5) |
| `Doctor(b fix) -> (b ok, as messages, s json)` | system check |
| signal `Changed(a{ss})` | the selection changed (also after CLI or GUI changes, seen by a file monitor on `state.json`) |

Interface XML: `data/dbus/io.github.OlehDzhumyk.Themes.xml` (a unit test checks the service against
it; the bridge loads the same file).

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

Each feature is an engine (`crates/deskd/src/engine/`): it is made before the bus exists, adds its
interfaces to the connection builder (`register`), connects its watchers once the name is owned
(`start`), and writes its pending state when the main loop stops (`shutdown`, in reverse start
order). Engines with wall-clock schedules add a callback to one shared watcher of resume and time
zone changes. `main.rs` only owns the bus name, the main loop and the process signals.

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

Only GNOME Shell (the owner of `org.gnome.Shell`) and our own binaries, outside any sandbox, may
call the content methods (all but the properties): others get `AccessDenied`, `gdbus`/`busctl`
included. The bus name is not replaceable, and the bridge talks only to a deskd it verified.
[ADR 0008](adr/0008-deskd-serves-content-to-trusted-callers.md).

`/io/github/OlehDzhumyk/Desk/Emoji`, interface `io.github.OlehDzhumyk.Desk.Emoji` (offline emoji, Unicode symbols and text faces with English/Ukrainian search, Fitzpatrick skin tones and 0600 recents in `~/.local/share/deskd/emoji.json`):

| Member | Description |
|---|---|
| `Search(s query, s lang, u limit) -> a(sssssb)` | ranked search across English and Ukrainian names/keywords: `(id, glyph, name, category, keywords, supports_tone)` |
| `Category(s category, u limit) -> a(sssssb)` | items in a category (`smileys`, `people`, `animals`, `food`, `travel`, `activities`, `objects`, `symbols`, `flags`, `arrows`, `math`, `currency`, `punctuation`, `text-faces`) |
| `Recents() -> a(sssssb)` | last 36 picked items, newest first |
| `Pick(s id, s tone) -> s` | returns the glyph with the Fitzpatrick skin-tone modifier applied and records it in recents |
| `ClearRecents()` | clears recents and removes `emoji.json` |

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

`/io/github/OlehDzhumyk/Desk/Appearance`, interface `io.github.OlehDzhumyk.Desk.Appearance`:

| Member | Description |
|---|---|
| `Status() -> (s mode, s scheme, s next_transition, s location_label, b polar, b overridden)` | current mode, effective scheme (`light`/`dark`), next transition (`HH:MM` or empty), resolved location, polar day/night flag, manual override state |
| `SetScheme(s scheme)` | manually switch `light`/`dark`/`default`, respecting `override-until-next` in auto modes |
| `Recompute()` | force immediate schedule recomputation and timer re-arm |
| `WallpaperAccent() -> (b enabled, s seed, s accent, b monochrome, s uri, s error, s palette)` | live "accent from wallpaper": on/off, last seed and named accent, near-monochrome flag, source wallpaper URI, last decoding error, the token theme whose libadwaita palette follows (`""` = accent only) |
| signal `Changed()` | emitted when mode, scheme, schedule, override or wallpaper-accent state changes |

The engine (`crates/deskd/src/appearance/`) computes local sunrise/sunset offline using the NOAA
solar algorithm, shares the one-shot wake scheduler (`crates/deskd/src/schedule.rs`) with
suspend/resume and timezone watchers, and writes `org.gnome.desktop.interface color-scheme` only
when the effective scheme changes. `appearance/wallpaper.rs` follows the wallpaper keys while
`accent-from-wallpaper` is on (GSettings signals, one 500 ms debounce), runs
`themes tokens seed --json` as a child process (an untrusted image never runs inside deskd), and
writes `accent-color`; the accent to restore is kept in `~/.cache/desktop-themes/wallpaper-accent.json`,
which is deleted when the setting is turned off. For token themes the seed's palette also goes into
the copied `gtk.css` through `themes tokens overlay` children (one worker thread, in order); the
overlay and its revert belong to themes-core (`overlay.rs`, [ADR 0010](adr/0010-live-wallpaper-palette-overlay.md)).

`/io/github/OlehDzhumyk/Desk/Focus`, interface `io.github.OlehDzhumyk.Desk.Focus`:

| Member | Description |
|---|---|
| `Status() -> (s mode, s name, s icon, s source, x until, x break_until, s next_mode, x next_start, as allowed_apps)` | active mode (empty: none), `manual`/`schedule`/`session`, end in Unix seconds (0: until turned off), session break end, next scheduled start, the active mode's allowed apps |
| `ListModes() -> s` | the modes as a JSON array (stored fields plus translated `label` and `builtin`) |
| `Start(s mode, x until)`, `StartSession(u minutes)`, `Stop()` | start by hand (0: until turned off), start a focus session (0: `session-length`), end |
| `SaveMode(s json)`, `DeleteMode(s id)` | add or replace a mode; delete a custom one |
| `Recompute()`; signals `Changed()`, `SessionEnded()` | re-evaluate schedules now |

The engine (`crates/deskd/src/focus/`) is a pure state machine with an injected clock and effects:
it stores the value of `show-banners`/`event-sounds` it overrides before writing, restores it only
while the key still holds deskd's value, and reconciles at start-up
([ADR 0009](adr/0009-deskd-writes-do-not-disturb-keys.md)).

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
  The clipboard end-to-end test lives in the container harness (`tests/e2e`, `just e2e`).
  Never call `toolbox` from inside `run.sh`: podman would see a different runtime dir.
