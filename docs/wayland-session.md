<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Windows 11 session on labwc (Wayland)

An optional second login session: the [labwc](https://labwc.github.io/) compositor with the
Windows 11 taskbar and Start menu of this project. GNOME stays installed and stays the default.
You pick the session on the login screen (gear button) and go back to GNOME the same way.

## How the parts work together

| Part | Role |
| --- | --- |
| `labwc` | Wayland compositor: windows, decorations, key bindings, snapping (`data/labwc/windows11/rc.xml`, `themerc`). |
| `desk-shell` | Taskbar, Start menu, quick settings and clipboard history, a GTK 4 layer-shell client (`gtk4-layer-shell`). It follows the `skin` setting and the light/dark changes of `deskd` live; the GNOME `default` skin draws as Windows 11. labwc calls `desk-shell --toggle-start` on the Windows key, `--toggle-quick-settings` on Win+A and `--toggle-clipboard` on Win+V; these calls go to the running shell over D-Bus at once. |
| `deskd` | The session daemon of this project (clipboard history, search, ...), on the session D-Bus, the same as under GNOME. `desk-shell` sends calls only to the `deskd` next to it or `/usr/bin/deskd`, of the same user and outside a sandbox ([ADR 0008](adr/0008-deskd-serves-content-to-trusted-callers.md)). Without `deskd`, search uses the built-in catalog (no files, clipboard or emoji). |
| `swaybg`, `swaylock`, `swayidle` | Wallpaper (the dark "bloom", else `#0b192c`) and lock screen (Win+L). `swayidle` locks the screen for `loginctl lock-session` (Lock in the Start menu) and before suspend. |
| `desk-win11-session` | Launcher: copies the config, installs the `desk-win11` labwc theme, sets the environment, runs labwc. |

Start order: the display manager runs `desk-win11-session`, which runs `labwc -C ~/.config/desk-labwc`.
labwc runs `autostart` from that directory: it imports `WAYLAND_DISPLAY` and
`XDG_CURRENT_DESKTOP` into the systemd user manager, then starts `swaybg`, `swayidle`, `deskd`
(`systemctl --user start deskd.service` when the systemd user manager runs) and `desk-shell`.
When labwc exits, `desk-win11-session` removes these variables from the user manager again.

## Clipboard history

Under GNOME the bridge reads the clipboard. labwc has no bridge, so `deskd` reads the clipboard
itself through the `ext-data-control-v1` protocol (or `wlr-data-control-unstable-v1` when the
compositor has only that one). The same capture works on other compositors with one of these
protocols: sway, niri, Hyprland, KWin and COSMIC. No package is necessary.

- `deskd` reads each new clipboard selection once: text first, then PNG, JPEG, GIF or WebP when
  the `images` setting is on. SVG is not read.
- `deskd` does not read a selection that has a password-manager type (for example
  `x-kde-passwordManagerHint`). It does not read in private mode or when the history is off.
- A read stops after 3 seconds or at the size limit (256 KiB of text, `image-max-bytes` for an
  image), so an app that does not answer cannot stop `deskd`.
- Wayland does not tell which app owns the clipboard, so `excluded-apps` has no effect here.
- Under GNOME Shell (Mutter) the compositor has neither protocol. The watcher stops at once and
  writes no message; the bridge captures the clipboard.

## 1. Layer the packages (Fedora Silverblue)

```sh
rpm-ostree install labwc gtk4-layer-shell swaybg swaylock swayidle grim slurp wl-clipboard
systemctl reboot
```

`grim`, `slurp` and `wl-clipboard` are optional (Win+Shift+S screenshot).
Layering does not remove GNOME. To undo it, boot the previous deployment:

```sh
rpm-ostree rollback   # or: rpm-ostree uninstall labwc gtk4-layer-shell swaybg swaylock swayidle ...
systemctl reboot
```

## 2. Install the session entry

Install this project first (`scripts/bootstrap.sh`, or `data/install.sh` after
`cargo build --release`). The install puts these files in your home directory:

- `deskd`, `desk-shell` (when it is built) and `desk-win11-session` in `~/.local/bin`.
- The bundled config `data/labwc/windows11` in `~/.local/share/desktop-themes/labwc/windows11`.

Then install the session entry. This step needs `sudo`:

```sh
sudo install -Dm644 data/labwc/desk-win11.desktop /usr/local/share/wayland-sessions/desk-win11.desktop
```

The entry runs `desk-win11-session`. A display manager does not always put `~/.local/bin` on
`PATH`. If the session does not start, also install the launcher system-wide:

```sh
sudo install -Dm755 data/labwc/desk-win11-session /usr/local/bin/desk-win11-session
```

On Silverblue `/usr/local` is writable (it is `/var/usrlocal`). The display manager reads
sessions from `/usr/local/share/wayland-sessions`; it does not read your home directory.

Check the setup without logging out:

```sh
desk-win11-session --dry-run   # copies the config, prints the labwc command
```

Log out, select **Desk (Windows 11 / labwc)** on the login screen, and log in.

## 3. Your own changes

The launcher copies the bundled config into `~/.config/desk-labwc` on each login.

- `rc.local.xml`: a complete labwc `rc.xml` of your own. When it exists, labwc uses it instead
  of the bundled `rc.xml`. Start from a copy of `rc.xml`.
- `autostart.user`: an executable script that `autostart` runs at the end.

Other files in that directory are replaced on the next login.

## Key bindings

| Keys | Action |
| --- | --- |
| Win (tap), Win+S | Start menu and search (`desk-shell --toggle-start`) |
| Win+A | Quick settings: dark mode, do not disturb, private clipboard, screenshot, settings hub, volume and volume mixer (`desk-shell --toggle-quick-settings`) |
| Win+V | Clipboard history (`desk-shell --toggle-clipboard`) |
| Alt+Tab, Alt+Shift+Tab | Next window, previous window |
| Win+D | Show the desktop, press again to restore |
| Win+E | Files |
| Win+L | Lock the screen |
| Win+Left, Win+Right | Snap to the left or right half |
| Win+Up, Win+Down | Maximize or restore, minimize |
| Win+Shift+S, Print | Region screenshot, then the capture editor |
| Ctrl+Shift+Esc | System monitor |
| Alt+F4 | Close the window |

## Tests

`just e2e-wayland` (`tools/e2e-wayland.sh`) runs the end-to-end tests in `tests/e2e-wayland/`.
They need podman or Docker; no systemd and no GPU. The steps:

1. The script builds `deskd` and `desk-shell` (`--no-build` uses `target/debug` as it is).
2. A Fedora 45 container gets the binaries, `data/` and the tests, all read-only.
3. In the container, labwc starts with `WLR_BACKENDS=headless` and `WLR_RENDERER=pixman`, with
   the bundled `rc.xml`. Then `deskd` and `desk-shell --skin windows11` start, on a private
   session bus and in their own `XDG_RUNTIME_DIR` and `XDG_CONFIG_HOME`.
4. The tests check these items:
   - `desk-shell` loads `libgtk4-layer-shell.so.0`, and the taskbar is a layer surface with an
     exclusive zone.
   - `deskd` owns `io.github.OlehDzhumyk.Desk` and `desk-shell` owns `io.github.OlehDzhumyk.DeskShell`.
   - `--toggle-start`, `--toggle-quick-settings` and `--toggle-clipboard` open and close their
     flyouts in the running shell, and exit 0.
   - Both processes stay alive, and their logs have no critical errors and no panics.

`desk-shell` runs with `WAYLAND_DEBUG=client`, so the tests see the layer surfaces in its log.
The logs of labwc, deskd and desk-shell go to `target/e2e-wayland`. CI runs the suite in
`.github/workflows/e2e-wayland.yml` when desk-shell, deskd or `data/labwc` change.

## Remove

```sh
sudo rm -f /usr/local/bin/desk-win11-session /usr/local/share/wayland-sessions/desk-win11.desktop
rm -r ~/.config/desk-labwc ~/.local/share/themes/desk-win11
```

`scripts/uninstall.sh` removes `desk-shell`, `desk-win11-session` and
`~/.local/share/desktop-themes/labwc`.

Then remove the layered packages (step 1).
