# Architecture decision records

Short records of decisions that shaped the project, so they are not argued again by accident.
Format: [MADR](https://adr.github.io/madr/)-style, one page each. To change a decision, add a new
ADR that supersedes the old one; do not rewrite history.

| ADR | Decision |
|---|---|
| [0001](0001-typescript-bridge-not-wasm.md) | The shell bridge is TypeScript, not WASM |
| [0002](0002-one-daemon-thin-bridge.md) | One Rust daemon (deskd) over zbus, plus a thin shell bridge |
| [0003](0003-gpl-and-attribution-policy.md) | GPL-3.0-or-later; port with attribution; never redistribute third-party assets |
| [0004](0004-design-tokens-generate-libadwaita-css.md) | Design tokens generate libadwaita CSS, which is copied, not linked |
| [0005](0005-headless-testing-is-isolated.md) | Headless testing must be fully isolated from the live session |
| [0006](0006-experimental-themes-need-confirmation.md) | Experimental themes require explicit confirmation |
| [0007](0007-e2e-in-containers.md) | End-to-end tests run GNOME Shell in a throw-away container, driven over Eval |
| [0008](0008-deskd-serves-content-to-trusted-callers.md) | deskd serves content only to GNOME Shell and its own binaries; its bus name is not replaceable |
| [0009](0009-deskd-writes-do-not-disturb-keys.md) | deskd writes the do-not-disturb keys for focus modes; the bridge only adds exceptions |
| [0010](0010-live-wallpaper-palette-overlay.md) | The live wallpaper palette is a recorded overlay in the generated gtk.css copy |
| [0011](0011-wayland-agnostic-rust-core.md) | A Wayland-agnostic Rust core, a desktop backend boundary and thin compositor adapters |

New ADR: copy `0000-template.md`, take the next number, add a row above.
