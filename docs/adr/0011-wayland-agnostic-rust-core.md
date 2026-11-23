# 0011. A Wayland-agnostic Rust core, a desktop backend boundary and thin compositor adapters

* Status: accepted
* Date: 2026-11-23

## Context

The product goal is a faithful, cohesive Windows 11 and macOS experience on Linux/Wayland, built
from clean open-source assets: one Fluent-style settings app with the tweaks Windows hides, desktop
features that are built in (clipboard history, emoji, mixer, focus, screenshots), and the same for
macOS ([principles](../spec/principles.md)).

Today the project targets GNOME 51 on Fedora Silverblue. The shell bridge is TypeScript
([ADR 0001](0001-typescript-bridge-not-wasm.md)), logic lives in `deskd`
([ADR 0002](0002-one-daemon-thin-bridge.md)), and the window is GTK 4 / libadwaita. We want three
things from the next stages:

* Maximum reliability and speed in Rust, and as little TypeScript as the platform allows.
* A path to KDE Plasma (KWin) and to wlroots/Smithay compositors without rewriting business logic.
* A settings app that can look like Windows 11 or macOS. libadwaita enforces GNOME's HIG, and a
  Fluent or Aqua layout fights it.

Facts that shape the design:

* Mutter does not implement `wlr-layer-shell`, so on GNOME panels, Control Center, OSD and the lock
  screen can only be drawn by code inside GNOME Shell. KWin, wlroots compositors and COSMIC do
  implement it, so a normal Wayland client can draw those surfaces there.
* Mutter has no `wlr-data-control`, so only the shell can watch the clipboard (ADR 0001). Other
  compositors offer a data-control protocol to ordinary clients.
* Many system services already have a standard D-Bus API, and XDG Desktop Portals cover screenshots,
  screen capture and global shortcuts across desktops.

## Decision

Four layers. Dependencies point downward only; a lower layer never knows which upper layer calls it.

1. **DE-agnostic Rust core: `deskd`, `desk-proto`, `theme-tokens`.**
   * Every feature is an `Engine`: one module with a small trait (start, stop, status, `Changed`),
     its own D-Bus object and its own tests. A new feature adds an engine, not a special case.
   * D-Bus contracts are versioned. The XML in `data/dbus/` is the contract, and an interface only
     gains members within a version.
   * Platform services come from standard interfaces first: XDG Desktop Portals through `ashpd`
     (Screenshot, ScreenCast, GlobalShortcuts), and system daemons (PipeWire, NetworkManager, BlueZ,
     UPower, systemd-logind, timedated). The core calls no GNOME-only API.
2. **`DesktopBackend` boundary: `themes-core` and `desk-settings`.** Everything that writes
   desktop-specific configuration sits behind one trait: read, write, snapshot, revert.
   * GNOME backend (today): dconf/GSettings, GNOME Shell extension control, and Flatpak-safe GTK
     copies ([ADR 0004](0004-design-tokens-generate-libadwaita-css.md)).
   * Later backends: KDE (`kwriteconfig6`, Plasma Look-and-Feel) and wlroots-style desktops.
   * Plan, managed keys, baseline, rollback and `themes doctor` stay above the boundary and work
     the same for every backend. The existing `dconf::Backend` is the first step.
3. **A settings app that does not depend on libadwaita.** The registry (`desk-settings`) and the
   backends carry no widget code. The app renders pages from the registry, so a skin can draw a
   Fluent or an Aqua layout.
   * The toolkit is `gtk4-rs` without libadwaita, or a pure-Rust toolkit such as `iced`. A short
     spike and a follow-up ADR choose between them; this ADR does not.
   * Until then the libadwaita window stays and gets no new logic. Moving a rule out of the window
     into the registry or the core is always allowed.
4. **Thin compositor adapters.** An adapter draws and forwards; it never decides.
   * GNOME: `desk-bridge` stays strict TypeScript with zero business logic (ADR 0001, ADR 0002).
     It is the only way to draw shell surfaces and read the clipboard there.
   * KDE, wlroots, COSMIC: a native Rust client uses `wlr-layer-shell` (and a data-control
     protocol for the clipboard) and talks to the same `deskd` over `desk-proto`.
   * Both adapters use the same D-Bus contracts, so a skin and a feature behave the same on both.

**Reuse and budgets.** For low-level plumbing (protocols, codecs, parsers) we adapt audited,
compatible-licence open-source code before we write our own. Each port follows
[ADR 0003](0003-gpl-and-attribution-policy.md): attribution, an entry in [CREDITS.md](../../CREDITS.md),
a pinned revision, and a licence that `cargo deny` accepts. New code must pass the deterministic
checks in CI and the performance budgets in `tools/perf/budgets.toml`. A change that breaks a
budget needs a reason in its pull request.

## Consequences

* Good: business logic exists once, in Rust, and is testable without a compositor. A second
  desktop needs a backend and an adapter, not a rewrite.
* Good: the TypeScript stays small and strict, and only where GNOME Shell forces it.
* Good: portals and standard D-Bus services work in Flatpak and on every desktop that ships them.
* Good: the settings app can follow Windows 11 and macOS layouts faithfully, with one registry.
* Bad / cost: the `DesktopBackend` trait can leak GNOME ideas if it is drawn from one backend
  only. We keep it small and revise it when the second backend lands.
* Bad / cost: portal support differs by desktop (GlobalShortcuts especially). Some features need a
  fallback or are marked unavailable on a desktop.
* Bad / cost: a Rust shell client for KDE and wlroots is a second shell implementation to
  maintain. It starts only when a user needs it.
* Bad / cost: a settings app without libadwaita loses its adaptive widgets, theming and
  accessibility defaults. We must rebuild or verify labels, keyboard paths and contrast.
* Not done by this ADR: no code moves. `Engine`, `DesktopBackend`, `ashpd` and the native shell
  client are targets; each lands in its own small pull request.

## Alternatives considered

* Drop the GNOME Shell extension now: impossible. Mutter gives no layer-shell and no clipboard
  watch, so panels, OSD and clipboard capture would be lost on GNOME.
* Write everything in JavaScript or QML: logic would run inside the shell or in a UI process. A bug
  can then crash the desktop, tests need a compositor, and ADR 0002 already rejected this.
* Target GNOME only: the fastest path today, but every feature would depend on GNOME APIs and a
  second desktop would mean a rewrite.
* Keep libadwaita for the settings app: native on GNOME, but it cannot give an authentic Fluent
  or Aqua layout, and a patched libadwaita breaks on each release.
* A new toolkit for everything, including shell surfaces: GNOME Shell cannot host it. It would
  not remove the extension.
* Build our own low-level plumbing for portals, PipeWire or D-Bus: slower, riskier and less
  tested than the maintained crates and daemons we can reuse.
