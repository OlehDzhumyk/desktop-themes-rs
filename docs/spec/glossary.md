# Glossary

**Accent.** The colour `org.gnome.desktop.interface accent-color` that our controls use. A theme may
set it; the user's explicit choice in the hub is kept by [automatic modes](features/auto-dark.md).

**Baseline.** A snapshot (`dconf dump /`) of the user's own settings taken once before the first
theme is applied, stored in `~/.local/share/desktop-themes/baseline`. It is the value a managed key
returns to when the selected theme does not set it, and what `themes reset` restores.

**Bridge.** The GNOME Shell extension `desk-bridge@desktop-themes-rs` (TypeScript, `bridge/`). It runs
inside the shell process, draws skinned panels, captures events only the compositor can see, sends
synthetic keys, and talks to deskd and the theme service over D-Bus. It holds no state of its own.
See [principles](principles.md#3-architecture-engine-thin-bridge-skins).

**Broken extension.** An extension that reached the `ERROR` or `OUT_OF_DATE` state after a live change
or after a GNOME update. The manager disables it, records it in `state.json`, and re-enables it only
when the extension or GNOME version changes and it loads cleanly ([doctor](features/doctor.md)).

**Crash protection.** GNOME's own safety net: after the shell crashes soon after start it sets
`org.gnome.shell disable-user-extensions=true` and no extension runs. The manager detects it and
offers a consented way back ([doctor](features/doctor.md)).

**deskd.** The session daemon (Rust, `crates/deskd`, bus name `io.github.OlehDzhumyk.Desk`) that
hosts the engines of desktop features. Started by D-Bus activation through a systemd user unit.

**Desk-bridge skin.** See *Skin*.

**Doctor.** `themes doctor [--fix]`: the system check, also run after login by `themes-doctor.service`.

**Engine.** A self-contained feature core inside deskd (today: clipboard history), pure logic with
unit tests, no D-Bus or GSettings inside; the service layer adapts it.

**Experimental.** A status for features that can destabilise the session. See the
[status legend](README.md#status-legend) and [liquid-glass](features/liquid-glass.md).

**Feature.** A user-visible capability with a spec in `docs/spec/features/`, a page in the settings
hub, an engine in deskd (if it has state) and a skin in each theme.

**Headless test.** An end-to-end test run in the isolated headless GNOME Shell of `tools/headless/`,
which never touches the real session. Acceptance criteria are written to become such tests.

**Hub.** The settings window «Робочий стіл» (today «Теми»); see [settings-hub](features/settings-hub.md).

**Inheritance.** `INHERITS=<id>` in `theme.conf`: a theme takes every file it does not have from its
parent ([theme-contract](../theme-contract.md)). Example: a snapshot with `INHERITS=macos`.

**Look / behaviour / sounds / apps.** The four [modules](#module).

**Managed keys.** For a module: every dconf key that any installed theme sets in that module plus
every key the module wrote last time. Applying a module touches only its managed keys; each one gets
the selected theme's value, else the baseline value, else the schema default
([module-system](../module-system.md)).

**Mixing.** Taking modules from different themes at the same time (macOS look with Windows 11
behaviour).

<a id="module"></a>**Module.** One of four independently switchable parts of a theme: `look` (GTK and shell themes, icons,
cursors, fonts, wallpapers, extensions, skin), `behavior` (shortcuts, Alt+Tab, touchpad, hot corners,
window placement), `sounds` (sound theme), `apps` (per-app settings and hooks).

<a id="origin-badge"></a>**Origin badge.** The one-line statement of where a feature comes from ("In Windows 11 and macOS",
"From Windows, adapted for macOS style", "Based on <project> (<licence>)", "Original"), shown on the
feature's page in the hub and listed in the [feature map](README.md#feature-map).

**Overlapping keys.** A key set by two modules: the later module wins in the order look, behaviour,
sounds, apps.

**Panel.** A surface drawn by the bridge (clipboard flyout, emoji picker, shortcut guide, ...).

**Permanent extension.** `desk-bridge@desktop-themes-rs`: enabled in every theme; a theme cannot
switch it off.

**Reduced motion.** The user preference `enable-animations=false` (and toolkit equivalents): every
transition becomes instant.

**Registry (settings).** The single declaration of every setting (stage 1.5) from which the GSettings
schema, the typed accessors, `docs/settings.md` and the window pages are generated.

**Safe theme.** `fedora`, the built-in theme that cannot be deleted; switching to it returns every
module to the baseline.

**Skin.** One of three UI variants of the bridge's panels, `default`, `macos`, `windows11`, chosen by
the dconf key `io.github.OlehDzhumyk.Desk skin` that a theme sets in its look module (`desk.dconf`).
A skin changes layout, shortcuts, wording and styling, not the feature's rules.

**Snapshot (theme).** "Save current look as a theme": the current settings laid out by module as a new
theme directory.

**State.** `~/.local/share/desktop-themes/state.json`: which theme each module comes from, keys
written, broken extensions, last GNOME version, last shell crash.

**Theme.** A directory under the theme root following the [theme contract](../theme-contract.md).

**Theme root.** `~/.local/share/desktop-themes` (override: `DESKTOP_THEMES_ROOT`).

**Theme service.** The D-Bus service `io.github.OlehDzhumyk.Themes` inside the `themes` binary,
started on demand, exits after 90 s idle.

**Tokens (design tokens).** The declarative description of a theme's colours, fonts and shapes
(`tokens.toml`) from which libadwaita and shell CSS, terminal palettes and so on are generated
(design-tokens (`docs/design-tokens.md`, lands with its stage 1.5 pull request)). Planned in stage 1.5.
