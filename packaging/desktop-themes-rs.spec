# SPDX-License-Identifier: GPL-3.0-or-later
# Fedora RPM spec for desktop-themes-rs (Fedora 44 / 45 / rawhide).
# See docs/packaging.md for local mock, COPR, and Silverblue rpm-ostree workflows.

# Offline builds (mock / COPR without network): pass a vendored Cargo archive as Source1
# (`cargo vendor`) and prebuilt bridge output (or build with `--without gnome_bridge`).
%bcond vendor 0
%bcond gnome_bridge 1

%global app_id      io.github.OlehDzhumyk.Themes
%global desk_id     io.github.OlehDzhumyk.Desk
%global bridge_uuid desk-bridge@desktop-themes-rs

Name:           desktop-themes-rs
Version:        0.3.0
Release:        1%{?dist}
Summary:        Desktop theme and features manager for GNOME and Wayland

License:        GPL-3.0-or-later
URL:            https://github.com/OlehDzhumyk/desktop-themes-rs
Source0:        %{url}/archive/v%{version}/%{name}-%{version}.tar.gz
%if %{with vendor}
Source1:        %{name}-%{version}-vendor.tar.xz
%endif

ExclusiveArch:  %{rust_arches}

BuildRequires:  appstream
BuildRequires:  cargo >= 1.100
BuildRequires:  clang-devel
BuildRequires:  desktop-file-utils
BuildRequires:  gcc
BuildRequires:  gettext
BuildRequires:  glib2-devel
BuildRequires:  gtk4-devel
BuildRequires:  libadwaita-devel
BuildRequires:  pipewire-devel
BuildRequires:  pkgconf-pkg-config
BuildRequires:  rust >= 1.100
BuildRequires:  systemd-rpm-macros
%if %{with gnome_bridge}
BuildRequires:  nodejs-npm
%endif

Requires:       dconf
Requires:       glib2
Requires:       gtk4
Requires:       libadwaita
Requires:       pipewire-libs

%description
desktop-themes-rs is a desktop theme and features manager for GNOME and Wayland
desktops. It switches the look, behaviour, sounds and per-app settings between
Fedora, macOS, Lavender and Windows 11 themes with module mixing, baseline
snapshots, doctor diagnostics and rollback. It also provides deskd (clipboard
history, search, mixer, OCR, colour picker) and desk-shell (a GTK 4 layer-shell
taskbar and flyouts).

%package labwc-session
Summary:        Windows 11 Wayland session on labwc for %{name}
BuildArch:      noarch
Requires:       %{name} = %{version}-%{release}
Requires:       grim
Requires:       gtk4-layer-shell
Requires:       labwc
Requires:       slurp
Requires:       swaybg
Requires:       swayidle
Requires:       swaylock
Requires:       wl-clipboard
Recommends:     brightnessctl
Recommends:     lxqt-policykit
Recommends:     mako

%description labwc-session
An optional Wayland login session built on labwc with the Windows 11 taskbar,
Start menu, quick settings and clipboard history provided by desktop-themes-rs.
GNOME stays installed and remains available on the login screen.

%if %{with gnome_bridge}
%package gnome-bridge
Summary:        GNOME Shell bridge extension for %{name}
BuildArch:      noarch
Requires:       %{name} = %{version}-%{release}
Requires:       gnome-shell

%description gnome-bridge
The desk-bridge@desktop-themes-rs GNOME Shell extension providing the
quick-settings theme toggle, clipboard capture and skinned shell UI for deskd.
%endif

%prep
%autosetup -n %{name}-%{version}
%if %{with vendor}
tar -xf %{SOURCE1}
mkdir -p .cargo
cat > .cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF
%endif

%build
export CARGO_INCREMENTAL=0
%if %{with vendor}
cargo build --release --locked --offline \
    -p themes -p deskd -p desk-shell
%else
cargo build --release --locked \
    -p themes -p deskd -p desk-shell
%endif

%if %{with gnome_bridge}
if [ ! -f "bridge/build/%{bridge_uuid}/extension.js" ]; then
    pushd bridge
    npm ci --no-audit --no-fund
    npm run build
    popd
fi
%endif

%install
# Core binaries and CLI aliases
install -Dm0755 target/release/themes %{buildroot}%{_bindir}/themes
ln -s themes %{buildroot}%{_bindir}/theme
ln -s themes %{buildroot}%{_bindir}/themes-gui
install -Dm0755 target/release/deskd %{buildroot}%{_bindir}/deskd
install -Dm0755 target/release/desk-shell %{buildroot}%{_bindir}/desk-shell

# GSettings schema
install -Dm0644 data/%{desk_id}.gschema.xml \
    %{buildroot}%{_datadir}/glib-2.0/schemas/%{desk_id}.gschema.xml

# Translations (.mo catalogues)
for po in po/*.po; do
    lang="$(basename "${po}" .po)"
    install -dm0755 "%{buildroot}%{_datadir}/locale/${lang}/LC_MESSAGES"
    msgfmt --check -o "%{buildroot}%{_datadir}/locale/${lang}/LC_MESSAGES/%{name}.mo" "${po}"
done

# Translated desktop entry and AppStream metainfo
install -dm0755 %{buildroot}%{_datadir}/applications
msgfmt --desktop -d po --template data/%{app_id}.desktop.in -o "%{_builddir}/%{app_id}.desktop"
sed "s|@BIN@|%{_bindir}|g" "%{_builddir}/%{app_id}.desktop" \
    > %{buildroot}%{_datadir}/applications/%{app_id}.desktop
chmod 0644 %{buildroot}%{_datadir}/applications/%{app_id}.desktop

install -dm0755 %{buildroot}%{_metainfodir}
msgfmt --xml -d po --template data/%{app_id}.metainfo.xml.in \
    -o %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml
chmod 0644 %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml

# D-Bus activation services
install -dm0755 %{buildroot}%{_datadir}/dbus-1/services
sed "s|@BIN@|%{_bindir}|g" data/%{app_id}.service \
    > %{buildroot}%{_datadir}/dbus-1/services/%{app_id}.service
sed "s|@BIN@|%{_bindir}|g" data/%{desk_id}.service \
    > %{buildroot}%{_datadir}/dbus-1/services/%{desk_id}.service
chmod 0644 %{buildroot}%{_datadir}/dbus-1/services/%{app_id}.service \
    %{buildroot}%{_datadir}/dbus-1/services/%{desk_id}.service

# Systemd user units
install -dm0755 %{buildroot}%{_userunitdir}
sed "s|@BIN@|%{_bindir}|g" data/deskd.service \
    > %{buildroot}%{_userunitdir}/deskd.service
sed "s|@BIN@|%{_bindir}|g" data/themes-doctor.service \
    > %{buildroot}%{_userunitdir}/themes-doctor.service
chmod 0644 %{buildroot}%{_userunitdir}/deskd.service \
    %{buildroot}%{_userunitdir}/themes-doctor.service

# Bundled themes
install -dm0755 %{buildroot}%{_datadir}/desktop-themes/themes
cp -a themes/. %{buildroot}%{_datadir}/desktop-themes/themes/

# Windows 11 labwc session subpackage files
install -Dm0755 data/labwc/desk-win11-session \
    %{buildroot}%{_bindir}/desk-win11-session
install -Dm0644 data/labwc/desk-win11.desktop \
    %{buildroot}%{_datadir}/wayland-sessions/desk-win11.desktop
install -dm0755 %{buildroot}%{_datadir}/desktop-themes/labwc/windows11
cp -a data/labwc/windows11/. %{buildroot}%{_datadir}/desktop-themes/labwc/windows11/

%if %{with gnome_bridge}
# GNOME Shell bridge extension subpackage files
install -dm0755 %{buildroot}%{_datadir}/gnome-shell/extensions/%{bridge_uuid}
cp -a "bridge/build/%{bridge_uuid}/." \
    %{buildroot}%{_datadir}/gnome-shell/extensions/%{bridge_uuid}/
if ls %{buildroot}%{_datadir}/gnome-shell/extensions/%{bridge_uuid}/schemas/*.gschema.xml >/dev/null 2>&1; then
    glib-compile-schemas %{buildroot}%{_datadir}/gnome-shell/extensions/%{bridge_uuid}/schemas
fi
%endif

%find_lang %{name}

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{app_id}.desktop
appstreamcli validate --no-net --explain %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml

%post
%systemd_user_post deskd.service themes-doctor.service

%preun
%systemd_user_preun deskd.service themes-doctor.service

%postun
%systemd_user_postun_with_restart deskd.service

%files -f %{name}.lang
%license LICENSE
%doc README.md CHANGELOG.md CREDITS.md
%{_bindir}/themes
%{_bindir}/theme
%{_bindir}/themes-gui
%{_bindir}/deskd
%{_bindir}/desk-shell
%{_datadir}/applications/%{app_id}.desktop
%{_datadir}/dbus-1/services/%{app_id}.service
%{_datadir}/dbus-1/services/%{desk_id}.service
%{_datadir}/desktop-themes/themes/
%{_datadir}/glib-2.0/schemas/%{desk_id}.gschema.xml
%{_metainfodir}/%{app_id}.metainfo.xml
%{_userunitdir}/deskd.service
%{_userunitdir}/themes-doctor.service

%files labwc-session
%license LICENSE
%{_bindir}/desk-win11-session
%{_datadir}/desktop-themes/labwc/windows11/
%{_datadir}/wayland-sessions/desk-win11.desktop

%if %{with gnome_bridge}
%files gnome-bridge
%license LICENSE
%{_datadir}/gnome-shell/extensions/%{bridge_uuid}/
%endif

%changelog
* Tue Nov 24 2026 OlehDzhumyk <93152974+OlehDzhumyk@users.noreply.github.com> - 0.3.0-1
- Initial Fedora RPM packaging with labwc-session and gnome-bridge subpackages.
