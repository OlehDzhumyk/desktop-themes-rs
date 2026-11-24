# 0012. desk-shell: a Rust GTK 4 shell on wlr-layer-shell, with gtk4-layer-shell loaded at run time

* Status: accepted
* Date: 2026-11-24

## Context

[ADR 0011](0011-wayland-agnostic-rust-core.md) keeps logic in a Rust core and makes compositor
adapters thin. On GNOME the adapter is the TypeScript bridge, because Mutter has no
`wlr-layer-shell`. labwc, niri, sway, Hyprland, KWin and COSMIC implement the protocol, so an
ordinary Wayland client can draw a taskbar and a Start menu there. We want the Windows 11
experience on those compositors without GNOME Shell, GJS or TypeScript.

Constraints:

* The crates `deskd`, `desk-search`, `desk-layouts`, `desk-capture`, `theme-tokens` and
  `desk-settings` must be reused as they are.
* The build must not need new crates or C `-devel` packages. Fedora Silverblue and the CI image
  have GTK 4, but not always `gtk4-layer-shell-devel`.
* Developers must be able to see the shell on GNOME, and tests must run without a compositor.

## Decision

Add the binary crate `crates/desk-shell`: GTK 4 (no libadwaita) plus `wlr-layer-shell` through
`libgtk4-layer-shell.so.0`, opened with `dlopen` at run time.

* `layer_shell.rs` resolves eight functions (`gtk_layer_is_supported`, `init_for_window`,
  `set_layer`, `set_anchor`, `auto_exclusive_zone_enable`, `set_margin`, `set_keyboard_mode`,
  `set_namespace`). If the library is absent, a symbol is missing, the compositor does not support
  the protocol, or `--windowed` is given, every surface stays a normal GTK window.
* gtk4-layer-shell must come before `libwayland-client` in the lookup order. So the program
  re-executes itself once with the library in `LD_PRELOAD`. Launched applications get the user's
  original `LD_PRELOAD` back.
* The shell is an adapter: it draws and forwards. Search ranking comes from `desk-search` in
  process (no D-Bus round trip per key). Usage counts, clipboard history, the light/dark scheme
  and screenshots go through `deskd` and the XDG Desktop Portal. System actions use
  systemd-logind (`loginctl`, `systemctl`).
* `deskd` trusts the `desk-shell` binary next to it, as it trusts `themes`
  ([ADR 0008](0008-deskd-serves-content-to-trusted-callers.md)). The other checks (same user,
  same root, not sandboxed) stay.
* One instance per session: the GTK application `io.github.OlehDzhumyk.DeskShell`.
  `desk-shell --toggle-start` from a compositor key binding toggles the flyout of the running
  instance over D-Bus.
* Windows 11 comes first: a 48 px bottom taskbar (`Layer::Top`, exclusive zone) and a Start and
  Search flyout (`Layer::Overlay`, keyboard on demand, 12 px above the bar). The `macos` and
  `default` skins use the same layout with their own palette until they get their own layouts.

## Consequences

* Good: the Windows 11 shell runs on any wlr-layer-shell compositor, with zero TypeScript and
  the same search, clipboard and capture engines as on GNOME.
* Good: no build-time dependency is added. The binary starts, and can be tested, everywhere.
* Good: the pure parts (placement fallback, pinned apps, keys, activations) have unit tests.
* Bad / cost: the `dlopen` binding is `unsafe` and has to match the C signatures of
  gtk4-layer-shell 1.x by hand. A missing symbol disables the layer surfaces, so a mismatch
  cannot crash the shell.
* Bad / cost: the `LD_PRELOAD` re-exec adds one `exec` at start-up.
* Bad / cost: there is no window list yet (it needs `wlr-foreign-toplevel-management`), and no
  clipboard capture yet (it needs a data-control protocol). These come in later stages.

## Alternatives considered

* Link gtk4-layer-shell through a `-sys` crate: it adds a crate and a `-devel` build dependency,
  and the binary fails to start where the library is missing.
* Write the layer-shell protocol by hand with `wayland-client` and `smithay-client-toolkit`:
  new crates, and we would have to give up GTK widgets or embed them, which GTK 4 does not allow.
* Keep everything in the GNOME bridge: it cannot run on compositors without GNOME Shell.
