# Testing

| Layer | Where | Run | In CI |
|---|---|---|---|
| Unit and integration tests (Rust) | `crates/*/src`, `crates/*/tests` | `just test` | `ci.yml` |
| Snapshot tests of generated CSS and palettes | `crates/theme-tokens/tests` | `just test` | `ci.yml` |
| Bridge type check and lint | `bridge/` | `just bridge` | `ci.yml` |
| **End-to-end in GNOME Shell** | `tests/e2e/` | `just e2e` | `e2e.yml` |
| Interactive headless shell, screenshots with your installed assets | `tools/headless/` | see CONTRIBUTING.md | `headless.yml` |
| Performance budgets | `tools/perf/` | `tools/perf/run.sh` | `headless.yml` (manual) |

Never run tests against your live session: unit tests must not write dconf
([ADR 0005](adr/0005-headless-testing-is-isolated.md)), shell tests run in a container
([ADR 0007](adr/0007-e2e-in-containers.md)).

## End-to-end tests (`tests/e2e`)

`tools/e2e.sh` (`just e2e`) builds `deskd`, `themes` and the bridge, builds the test image
(`tests/e2e/Containerfile`, FROM ddterm's Fedora 45 GNOME Shell image, tagged by the hash of the
Containerfile), starts a container with systemd as PID 1 and runs pytest inside it. The repository
is mounted read-only at `/src`; results go to `target/e2e/` (`--out DIR`).

```sh
just e2e                          # everything
just e2e -k "clipboard and macos" # pytest selection
tools/e2e.sh --no-build -m smoke  # the smoke subset, without rebuilding
E2E_KEEP_LOGS=1 just e2e          # keep the shell log of passing tests too
```

### What one test gets

`make_shell(skin=..., extensions=..., dconf=...)` (`conftest.py`) starts a fresh session:

* its own HOME, `XDG_*` dirs, dconf database and private `dbus-daemon`;
* the built bridge and `lockscreen-layout` copied into `~/.local/share/gnome-shell/extensions`
  (schemas compiled), `deskd` and `themes` linked into `~/.local/bin` with D-Bus activation files,
  the repository's themes as the theme root and a baseline like `scripts/bootstrap.sh` makes;
* the skin's `desk.dconf` from the matching theme (`macos`, `windows11`, `fedora` for `default`);
* `gnome-shell --headless --unsafe-mode --wayland --virtual-monitor 1920x1080`, then the test waits
  for the shell's objects, the end of the start-up animation, the bridge's ACTIVE state and one
  redraw + idle.

When the test ends the shell is shut down with `global.context.terminate()` (the same path as a
logout), and the test **fails on any JS error or critical whose message or stack names the bridge**.
A failing test leaves `<test>-failure.png` and `<test>.shell.log` in the output directory.

### Driving the shell

`org.gnome.Shell.Eval` works only because the shell runs with `--unsafe-mode`, which happens only
inside the container. `Session.hook(name, *args)` loads `harness/hook.js` into the shell and calls
one of its exports:

| Hook | What it does |
|---|---|
| `keys('Shift+Super+v')` | presses a combination on a fresh Clutter virtual keyboard |
| `screenshot(path)` | full-screen PNG through `Shell.Screenshot` |
| `bridgeState()` | skin, panel class, whether the panel is open, keybinding, language |
| `waitPanel(open)` / `waitBridge(field, value)` | resolve on the panel's `notify::visible` / after a settings change |
| `panelItems()` | refreshes the open panel from deskd and returns its texts |
| `settle()` | one redraw, then idle (needed after enabling an extension at run time) |
| `startTracking()` / `leaks()` / `stopTracking()` | records objects, signal handlers and GLib sources the bridge creates; reports what survives `disable()` |

Rules: no `sleep` in tests, wait on a signal, a promise or a line of output (timeouts only bound a
hang); kill only processes the test spawned (`procs` fixture); keep the bridge free of test hooks.

### The tests

| File | Covers |
|---|---|
| `test_bridge.py` | enables without errors on `macos`, `windows11`, `default`; live skin switch |
| `test_clipboard.py` | per skin: `wl-copy` → deskd history → theme shortcut opens the panel → Down + Enter pastes into a GTK window |
| `test_shortcuts.py` | `themes shortcut set` reaches the bridge live; a taken accelerator is refused and nothing is written |
| `test_themes.py` | `themes apply macos/windows11/fedora` live (no rollback, skin follows), `themes doctor` has no errors; a synthetic experimental theme refused without `--yes` |
| `test_lifecycle.py` | nothing left after disable (handlers on shell objects, sources, actors, keybinding); logout with the bridge enabled or after disable leaves no GC warning or error from the bridge |

Third-party extensions of the themes are not installed in the image, so `themes apply` and
`themes doctor` warn that they are missing; that is expected there. The experimental theme is never
applied (only its refusal is tested).

### Shutdown warning "call back into JSAPI during the sweeping phase of GC"

GJS prints it when an actor that was never destroyed is finalised by the garbage collector while a
JS `destroy` handler is still connected; the line after it names the type ("The offending signal
was destroy on ..."). On the development machine the types were GNOME Shell's own search result
lists and dash-to-panel's taskbar items, never a desk-bridge type, and the harness does not reproduce
it with the bridge. `test_lifecycle.py` keeps it that way: `leaks()` reports bridge-created actors
that are off the stage, never destroyed and carry destroy handlers, and the logout tests fail on a
GC warning that names the bridge.

### Updating the image

Bump the tag and digest in `tests/e2e/Containerfile` together (`podman pull` prints the digest).
The image tag of the runner changes with the Containerfile's hash, so the next run rebuilds it.
