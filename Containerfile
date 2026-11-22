# SPDX-License-Identifier: GPL-3.0-or-later
# Development image matching CI (Fedora 45). Use it as a toolbox:
#   podman build -t desktop-themes-dev -f Containerfile .
#   toolbox create --image localhost/desktop-themes-dev themes-dev
FROM registry.fedoraproject.org/fedora-toolbox:45

RUN dnf install -y --setopt=install_weak_deps=False \
        git gcc pkgconf-pkg-config glib2-devel gtk4-devel libadwaita-devel pipewire-devel clang-devel \
        rust cargo rustfmt clippy cargo-deny \
        nodejs npm ShellCheck just jq gitleaks desktop-file-utils libxml2 \
        dbus-daemon unzip pipewire wireplumber pipewire-utils \
    && dnf clean all

# cargo-nextest and ast-grep, version- and checksum-pinned in the script (also used by CI).
COPY tools/install-dev-tools.sh /tmp/install-dev-tools.sh
RUN /tmp/install-dev-tools.sh /usr/local/bin && rm /tmp/install-dev-tools.sh
