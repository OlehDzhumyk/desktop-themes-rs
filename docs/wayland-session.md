<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Windows 11 session on labwc (Wayland)

An optional second login session: the [labwc](https://labwc.github.io/) compositor with the
Windows 11 taskbar and Start menu of this project. GNOME stays installed and stays the default.
You pick the session on the login screen (gear button) and go back to GNOME the same way.

## How the parts work together

| Part | Role |
| --- | --- |
| `labwc` | Wayland compositor: windows, decorations, key bindings, snapping (`data/labwc/windows11/rc.xml`, `themerc`). |
| `desk-shell` | Taskbar and Start menu, a GTK 4 layer-shell client (`gtk4-layer-shell`), started with `--skin windows11`. labwc calls `desk-shell --toggle-start` on the Windows key. |
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

Install this project first (`scripts/bootstrap.sh`), so that `deskd` and `desk-shell` are in
`~/.local/bin`. Then, from the repository:

```sh
mkdir -p ~/.local/share/desktop-themes/labwc
cp -r data/labwc/windows11 ~/.local/share/desktop-themes/labwc/
sudo install -Dm755 data/labwc/desk-win11-session /usr/local/bin/desk-win11-session
sudo install -Dm644 data/labwc/desk-win11.desktop /usr/local/share/wayland-sessions/desk-win11.desktop
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
| Win (tap), Win+S, Win+A | Start menu (`desk-shell --toggle-start`) |
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
sudo rm /usr/local/bin/desk-win11-session /usr/local/share/wayland-sessions/desk-win11.desktop
rm -r ~/.config/desk-labwc ~/.local/share/themes/desk-win11 ~/.local/share/desktop-themes/labwc
```

Then remove the layered packages (step 1).
