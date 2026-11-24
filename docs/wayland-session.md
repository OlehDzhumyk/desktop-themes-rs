<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Windows 11 session on labwc (Wayland)

An optional second login session: the [labwc](https://labwc.github.io/) compositor with the
Windows 11 taskbar and Start menu of this project. GNOME stays installed and stays the default.
You pick the session on the login screen (gear button) and go back to GNOME the same way.

## How the parts work together

| Part | Role |
| --- | --- |
| `labwc` | Wayland compositor: windows, decorations, key bindings, snapping (`data/labwc/windows11/rc.xml`, `themerc`). |
| `desk-shell` | Taskbar, Start menu, quick settings and clipboard history, a GTK 4 layer-shell client (`gtk4-layer-shell`), started with `--skin windows11`. labwc calls `desk-shell --toggle-start` on the Windows key, `--toggle-quick-settings` on Win+A and `--toggle-clipboard` on Win+V. |
| `deskd` | The session daemon of this project (clipboard history, search, ...), on the session D-Bus, the same as under GNOME. |
| `swaybg`, `swaylock` | Wallpaper (the dark "bloom", else `#0b192c`) and lock screen (Win+L). |
| `desk-win11-session` | Launcher: copies the config, installs the `desk-win11` labwc theme, sets the environment, runs labwc. |

Start order: the display manager runs `desk-win11-session`, which runs `labwc -C ~/.config/desk-labwc`.
labwc runs `autostart` from that directory: it starts `swaybg`, then `deskd`
(`systemctl --user start deskd.service` when the systemd user manager runs), then `desk-shell`.

## 1. Layer the packages (Fedora Silverblue)

```sh
rpm-ostree install labwc gtk4-layer-shell swaybg swaylock grim slurp wl-clipboard
systemctl reboot
```

`grim`, `slurp` and `wl-clipboard` are optional (Win+Shift+S screenshot).
Layering does not remove GNOME. To undo it, boot the previous deployment:

```sh
rpm-ostree rollback   # or: rpm-ostree uninstall labwc gtk4-layer-shell swaybg swaylock ...
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

## Remove

```sh
sudo rm -f /usr/local/bin/desk-win11-session /usr/local/share/wayland-sessions/desk-win11.desktop
rm -r ~/.config/desk-labwc ~/.local/share/themes/desk-win11
```

`scripts/uninstall.sh` removes `desk-shell`, `desk-win11-session` and
`~/.local/share/desktop-themes/labwc`.

Then remove the layered packages (step 1).
