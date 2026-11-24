<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Packaging (Fedora RPM, COPR, Silverblue)

`desktop-themes-rs` supports both user-level installs (`data/install.sh`, binaries in
`~/.local/bin`) and system-wide Fedora RPM packages (`packaging/desktop-themes-rs.spec`,
binaries in `/usr/bin`).

## Subpackages

The spec file builds three packages for Fedora 44, Fedora 45 and Rawhide:

| Package | Arch | Contents |
| --- | --- | --- |
| `desktop-themes-rs` | native (`%{rust_arches}`) | `themes` (+ `theme`, `themes-gui` aliases), `deskd`, `desk-shell`, GSettings schema, gettext `.mo` catalogues, D-Bus activation files, systemd user units, bundled themes (`/usr/share/desktop-themes/themes/`), `.desktop` entry and AppStream metainfo |
| `desktop-themes-rs-labwc-session` | `noarch` | `/usr/bin/desk-win11-session`, `/usr/share/wayland-sessions/desk-win11.desktop`, `/usr/share/desktop-themes/labwc/windows11/`; pulls `labwc`, `gtk4-layer-shell`, `swaybg`, `swaylock`, `swayidle`, `grim`, `slurp`, `wl-clipboard` and recommends `mako`, `lxqt-policykit`, `brightnessctl` |
| `desktop-themes-rs-gnome-bridge` | `noarch` | `desk-bridge@desktop-themes-rs` under `/usr/share/gnome-shell/extensions/`; requires `gnome-shell` |

### System vs. user installs

Both layouts obey the trust boundary of [ADR 0008](adr/0008-deskd-serves-content-to-trusted-callers.md):

- `deskd` and `themes --dbus-service` trust sibling binaries installed next to their own
  executable (`/usr/bin/{deskd,themes,desk-shell}` for RPM installs, `~/.local/bin/...` for user
  installs).
- `desk-shell` verifies that the owner of `io.github.OlehDzhumyk.Desk` is either the `deskd`
  next to it or `/usr/bin/deskd`.
- `desk-bridge@desktop-themes-rs` reads `Exec=` from both user and system D-Bus service
  directories (`$XDG_DATA_HOME/dbus-1/services` and `/usr/share/dbus-1/services`).
- `data/deskd.service`, `data/themes-doctor.service`, `data/io.github.OlehDzhumyk.{Desk,Themes}.service`
  and `data/io.github.OlehDzhumyk.Themes.desktop.in` all use `@BIN@`, substituted with
  `%{_bindir}` by the RPM spec and with `~/.local/bin` (or `%h/.local/bin` in user units) by
  `data/install.sh`.

## Validate spec and AppStream metadata

```sh
# Spec syntax and macro expansion (requires rpm-build)
rpmspec -P packaging/desktop-themes-rs.spec > /dev/null

# Strict AppStream validation (requires gettext and appstream)
msgfmt --xml -d po \
  --template data/io.github.OlehDzhumyk.Themes.metainfo.xml.in \
  -o /tmp/io.github.OlehDzhumyk.Themes.metainfo.xml
appstreamcli validate --no-net --explain /tmp/io.github.OlehDzhumyk.Themes.metainfo.xml
```

CI runs `appstreamcli validate --no-net --explain` strictly in the `static-checks` job.

## Local build with `mock`

`mock` builds inside a clean, offline Fedora chroot. Because `mock` disables network access during
`%build`, vendor Cargo crates first (and prebuild `bridge/build` or pass `--without gnome_bridge`):

```sh
VERSION=0.3.0
git archive --format=tar.gz --prefix="desktop-themes-rs-${VERSION}/" -o "desktop-themes-rs-${VERSION}.tar.gz" HEAD

# Vendor Rust dependencies for offline builds
cargo vendor vendor
tar -cJf "desktop-themes-rs-${VERSION}-vendor.tar.xz" vendor
rm -rf vendor

# Build SRPM
rpmbuild -bs \
  --define "_sourcedir ${PWD}" \
  --define "_srcrpmdir ${PWD}" \
  --with vendor \
  packaging/desktop-themes-rs.spec

# Build RPMs in Fedora 45 mock (pass --without gnome_bridge if npm packages are not vendored)
mock -r fedora-45-x86_64 --with vendor --without gnome_bridge \
  "desktop-themes-rs-${VERSION}-1.fc45.src.rpm"
```

Replace `fedora-45-x86_64` with `fedora-44-x86_64`, `fedora-rawhide-x86_64`, or `aarch64` as
needed.

## Publishing on Fedora COPR

1. Create a COPR project targeting `fedora-44-x86_64`, `fedora-44-aarch64`, `fedora-45-x86_64`,
   `fedora-45-aarch64`, and `fedora-rawhide-*`.
2. Either upload the vendored SRPM directly:

```sh
copr-cli build <owner>/desktop-themes-rs "desktop-themes-rs-${VERSION}-1.fc45.src.rpm"
```

3. Or configure a COPR SCM / custom script build that checks out the tag, runs `cargo vendor`,
   builds `bridge/`, and invokes `rpmbuild -bs --with vendor packaging/desktop-themes-rs.spec`.

## Installing on Fedora Silverblue / Atomic Desktops

Once a COPR repository is published (or after copying locally built RPMs to the host):

```sh
# 1. Enable the COPR repository (replace <owner> and releasever as needed)
curl -fsSL \
  "https://copr.fedorainfracloud.org/coprs/<owner>/desktop-themes-rs/repo/fedora-$(rpm -E %fedora)/<owner>-desktop-themes-rs-fedora-$(rpm -E %fedora).repo" \
  | sudo tee /etc/yum.repos.d/_copr:desktop-themes-rs.repo > /dev/null

# 2. Layer the core package and whichever session/bridge subpackages you want
rpm-ostree install desktop-themes-rs desktop-themes-rs-labwc-session
# Optional: add desktop-themes-rs-gnome-bridge for GNOME Shell integration

# 3. Reboot into the new deployment
systemctl reboot
```

To install locally built RPMs without COPR:

```sh
rpm-ostree install ./desktop-themes-rs-*.rpm ./desktop-themes-rs-labwc-session-*.rpm
systemctl reboot
```

To revert to the previous deployment cleanly:

```sh
rpm-ostree rollback
systemctl reboot
```

See [docs/wayland-session.md](wayland-session.md) for using the **Desk (Windows 11 / labwc)**
session after installing `desktop-themes-rs-labwc-session`, and [docs/maintainer.md](maintainer.md)
for tagging releases.
