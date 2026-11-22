# Product principles

These principles were agreed with the project owner and bind every feature spec. When a spec seems
to contradict one, the principle wins and the spec is wrong.

## 1. One settings hub

All settings of desktop-themes-rs live in one place: the window «Робочий стіл» (Desktop; today named
«Теми»), reachable from quick settings, the launcher and `themes gui`. Every setting is declared
once in the settings registry and stored in GSettings, so the window, `gsettings`, deskd and the shell
bridge see the same values. See [settings-hub](features/settings-hub.md).

* No feature ships its own preferences dialog, its own config file format, or a second copy of a
  setting.
* Features appear on a page of the hub with their [origin badge](glossary.md#origin-badge).

## 2. No legacy duplicates

When we add a feature the system or a third-party extension already provides, the old one is
retired in the bundled themes, not kept beside the new one: two clipboard indicators, two launchers or
two shortcut owners on the same key are bugs (the installed Clipboard Indicator is no longer enabled
by any bundled theme; the planned search engine replaces the third-party launcher).

* A key binding has exactly one owner per theme. The behaviour tables ([behavior](features/behavior.md))
  are the registry of owners; a new shortcut must not collide with an existing one.
* Migration scripts retire old ids (`scripts/migrate-from-legacy.sh`).

## 3. Architecture: engine, thin bridge, skins

```
 deskd (Rust)          the logic and the state: history, search index, schedules, OCR, ...
 desk-bridge (TS)      the thin shell side: capture events, draw skinned UI, send keys
 skin (per theme)      the look of the UI: default | macos | windows11 (a dconf key of the theme)
```

* Logic, state and anything heavy go in **deskd** (Rust, small, no polling). Anything that only the
  compositor process can do (see the clipboard, grab the keyboard, draw over the desktop, synthesise
  input) goes in the **bridge**, kept as thin as possible and written in TypeScript.
* The bridge never decides; it asks deskd. deskd never draws.
* A skin changes layout, shortcuts and wording, never the feature's rules (limits, privacy, data).
* Everything is reachable without the shell UI (`gdbus`, `gsettings`, the CLI) so it can be tested
  headlessly.

## 4. Order of work: shared first, then vendor-specific, "as the other vendor would do it"

1. Features that Windows 11 **and** macOS both have come first. They are built once and skinned
   twice.
2. Then features only one of them has. The other skin gets an adaptation **designed as that vendor
   would have designed it**, with that vendor's shortcut conventions, wording and visual language,
   not a copy of the original's look. The badge says so ("From Windows, adapted for macOS style").
3. The default (Fedora) skin gets the plain, GNOME-native form of every feature.
4. We describe vendor behaviour; we never copy vendor assets (logos, fonts, icons, sounds,
   wallpapers, trademarks) and never use a vendor product name as our own feature name in the UI.

## 5. Open source first

Before writing code, look for a maintained open-source project that does it.

* **Integrate** it when it can be installed and configured at user level and its licence allows
  (as the themes do with Dash to Dock or Blur my Shell).
* **Port with attribution** when it must live in our code (as the clipboard capture was adapted
  from Clipboard Indicator): keep the licence notice in the source header, name the project in
  [CREDITS.md](../../CREDITS.md), and make the licence compatible with GPL-3.0-or-later.
* **Upstream fixes** that we write (patches for third-party extensions are kept as diffs under
  `patches/`, never vendored) are offered to the upstream project.
* No code with an incompatible or unknown licence; no copying from closed-source products.

## 6. Respect the system

Our UI follows what the user already told the system:

* **Dark mode:** `org.gnome.desktop.interface color-scheme`. Skins ship light and dark variants and
  switch live.
* **Accent colour:** `org.gnome.desktop.interface accent-color` (GNOME 47+) is the accent of our
  controls, selection and focus ring.
* **Reduced motion:** when `enable-animations` is off (or the toolkit reports reduced motion) every
  transition becomes instant; no information may be conveyed by motion alone.
* **Accessibility:** everything is operable by keyboard alone, has an accessible name and role,
  and meets WCAG 2.2 AA contrast (4.5:1 text, 3:1 non-text) in every skin and scheme; text scaling
  (`text-scaling-factor`) and large-text and high-contrast settings are honoured. Focus is visible and
  never lost; closing a panel returns focus to where it was.
* **Input methods and layouts:** shortcuts use keysyms where the user's layout would otherwise
  break them; typing in our fields works with input methods.

## 7. Privacy: local only, no telemetry

* Nothing leaves the machine: no network access from deskd or the bridge, no analytics, no crash
  upload, no update ping. A feature that needs the network (for example weather-based sunset) must
  say so in its spec, be off by default, and have a local alternative.
* Sensitive data is minimised: what is stored is listed in each spec, stored under `~/.local/share`
  (0600/0700) or in `$XDG_RUNTIME_DIR` (session-only), never in world-readable places, never in
  logs. Password-manager content is never read.
* Every store can be cleared from the hub, and clearing is real deletion.

## 8. Safety: never break the session

* **Baseline:** the user's own settings are captured before any theme touches them; `themes reset`
  returns to them ([themes](features/themes.md)).
* **Rollback:** extensions that fail after a live change are switched off automatically.
* **Doctor and crash protection:** `themes doctor` explains and repairs; after a shell crash the manager
  tells the user which theme and extensions are the likely cause and never re-enables extensions
  without consent ([doctor](features/doctor.md)).
* A bridge failure (deskd missing, schema missing, exception) must degrade to "feature unavailable"
  with a clear message, never to a frozen or crashed shell. The bridge never blocks the shell main
  loop on D-Bus or disk; all calls are asynchronous with timeouts.
* Features that can destabilise the shell are **Experimental**, off by default, marked in the UI and
  the CLI, and need confirmation to enable.
* Everything is undoable: each feature's settings reset to the schema defaults from the hub.

## Performance budgets

The numbers below are release-build budgets; the measurable list lives in
`tools/perf/budgets.toml` (stage 1.5) and is enforced by `tools/perf/compare.py`.

| Budget | Limit |
|---|---|
| A panel of ours becomes visible after its shortcut | <= 16 ms (one 60 Hz frame), p95 |
| Panel filled with items (D-Bus round trip + render) | <= 50 ms, p95 (soft) |
| deskd resident memory, idle and after 200 clipboard pushes | <= 20 MB |
| deskd idle CPU | 0% (<= 0.1% measured) |
| deskd idle wakeups | about 0 per minute (<= 2); no polling, no periodic timers; use signals, inotify, and one-shot timers armed for the next real event |
| `themes list` | <= 50 ms wall time |
| Design-token generation | <= 50 ms |
| `themes apply X --no-verify` | <= 3 s (includes the deliberate ~1 s pause between dconf and extensions) |
| gnome-shell memory cost of the bridge | <= 10 MB |

Every new feature spec states which of these it touches and adds its own budgets there. A feature that
cannot meet a budget is not merged; the budget is not raised silently.

## Internationalisation

* Two languages ship: **Ukrainian** and **English**. Source strings are English gettext ids; Ukrainian
  lives in `po/uk.po`.
* Default: follow the session locale. An in-app **language override** (`system | uk | en`, key
  `io.github.OlehDzhumyk.Desk language`) applies to the window, the `themes` command, deskd messages
  and the bridge, without a logout.
* No string is built by concatenation; plurals use gettext plural forms; dates, times and numbers use
  the locale; layouts must survive strings 40% longer than English.
* Search and sorting use Unicode-aware case folding (Cyrillic included).

## Conventions every spec uses

* Shortcuts are written `Super+V`; the `windows11` skin calls the key Win in prose only when quoting
  the reference behaviour; the `macos` skin maps Command to Super and Option to Alt.
* "Panel" means a shell-drawn surface of the bridge; "window" means the GTK application.
* A feature that has a shell UI declares its behaviour for the three skins: `macos`, `windows11` and
  `default`.
