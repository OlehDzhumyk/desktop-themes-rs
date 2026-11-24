# Functional specifications

This tree says what every feature of desktop-themes-rs **does** and **exactly how it must behave**.
It is the contract between the user, the implementer and the tests: a behaviour that is not in a
spec is not guaranteed, and a behaviour that contradicts a spec is a bug in the code or in the spec
(open an issue or fix whichever is wrong).

* [principles.md](principles.md): the product principles every spec follows.
* [glossary.md](glossary.md): module, skin, engine, bridge, baseline, managed keys, origin badge, ...
* [features/](features/): one file per feature (map below).

Related documents (how it is built, not how it behaves): [architecture](../architecture.md),
[theme contract](../theme-contract.md), [module system](../module-system.md),
[bundled themes](../themes.md), [desktop features overview](../features.md).

## How to read a spec

Every feature spec has the same sections, in this order:

| Section | Content |
|---|---|
| Summary | one paragraph: what the user gets |
| Status | one of the labels below, plus the stage and what is missing |
| Reference behaviour | how Windows 11 and macOS do it (descriptive; no vendor assets are used) |
| Our behaviour | exact behaviour, per skin where skins differ: shortcuts, layout, interactions, keyboard navigation, focus, animation (and the reduced-motion variant), empty and error states |
| Settings | GSettings keys with defaults and ranges, read from the schema or the code (planned keys are marked *proposed*) |
| Privacy & security | what is stored, where, who can read it, what is never collected |
| Edge cases | the awkward inputs and environments, each with the required behaviour |
| Accessibility | keyboard, screen reader, contrast, motion |
| Performance | the budgets that apply (see [principles](principles.md#performance-budgets)) |
| Acceptance criteria | testable lines `AC-<feature>-NN: Given ..., When ..., Then ...`; they become headless end-to-end tests (`tools/headless/`) |
| Open questions | decisions the user or the implementer still has to make |

Requirement words: **must** and **must not** are binding; **should** is the default that may be
overridden with a reason written in the spec; **may** is optional.

Specs of Implemented features describe what the code does today and mark every difference from the
intended behaviour as a **Gap** (with the file that causes it). Specs of Planned features are written
ahead of the implementation: the implementer must follow them, and changes go through the spec first.

UI strings are quoted in English; the shipped texts are Ukrainian and English (see
[principles](principles.md#internationalisation)). Paths use `~` for the home directory.

## Status legend

| Status | Meaning |
|---|---|
| **Implemented** | on `main`, behaves as specified (listed gaps are small) |
| **Partial** | on `main`, but a part of the specified behaviour is missing |
| **Planned** | not written yet; the spec is the brief for the implementer |
| **Experimental** | on `main` but known to be unsafe or unfinished; off by default or clearly marked |
| **Removed** | was shipped, then taken out; the spec stays as the record |

## Stages

| Stage | Content | State |
|---|---|---|
| 0 | theme manager (CLI, window, service), modules, inheritance, doctor | done |
| 1 | `deskd` + `desk-bridge`, skins, clipboard history | done |
| 1.5 | foundations: design tokens, settings registry, i18n and accessibility, CI and performance budgets, these specs | done (follow-ups: see below) |
| 2 | settings hub, emoji and symbols, volume mixer, focus modes, automatic light/dark, theme from wallpaper | implemented |
| 3 | capture family: screenshots, annotation, recording, OCR, colour picker | implemented |
| 4 | own search engine, Quick Look, shortcut guide, window layouts | implemented |
| 5 | phone integration | proposed |

Stage 5 is a proposal (see the open questions in its spec).

## Origin badges

Every feature carries one badge, shown on its settings page ([glossary](glossary.md#origin-badge)):

* *In Windows 11 and macOS*: both systems have it; we build it once and skin it.
* *From Windows, adapted for macOS style*: Windows has it (natively or in PowerToys), macOS does not; the macOS skin gets it "as Apple would have done it".
* *From macOS, adapted for Windows 11 style*: the reverse.
* *Based on `<project>` (`<licence>`)*: code or design taken from an open-source project, with attribution in [CREDITS.md](../../CREDITS.md).
* *Original*: nothing to borrow.

## Feature map

| Feature | Status | Stage | Origin badge | Spec |
|---|---|---|---|---|
| Theme switching, modules, mixing, inheritance, reset | Implemented | 0 | Original | [themes](features/themes.md) |
| Token-only showcase theme `lavender` | Implemented | 1.5 | Original | [themes](features/themes.md) |
| Doctor and crash protection | Implemented | 0 | Original | [doctor](features/doctor.md) |
| Quick settings theme menu | Implemented | 1 | Original | [quick-settings-theme](features/quick-settings-theme.md) |
| Per-theme behaviour and keybindings | Implemented | 0 | In Windows 11 and macOS (each theme follows its system) | [behavior](features/behavior.md) |
| Sounds | Partial | 0 | Original (generated, CC0) | [sounds](features/sounds.md) |
| Lock screen layouts | Implemented | 0 | In Windows 11 and macOS | [lock-screen](features/lock-screen.md) |
| Clipboard history | Implemented | 1 | In Windows 11 and macOS; based on Clipboard Indicator (MIT) | [clipboard](features/clipboard.md) |
| Spotlight-style launcher (third-party today) | Partial | 0 | Based on Spotlight by itsnin (GPL-3.0-or-later) | [search](features/search.md) |
| Own search engine and search box | Partial | 4 | In Windows 11 and macOS | [search](features/search.md) |
| Liquid Glass variant (`macos-glass`) | Removed | 0 | Based on Liquid Glass extension | [liquid-glass](features/liquid-glass.md) |
| Volume mixer and audio device switching | Implemented | 2 | From Windows, adapted for macOS | [mixer](features/mixer.md) |
| Emoji and symbol picker | Implemented | 2 | In Windows 11 and macOS | [emoji](features/emoji.md) |
| Focus modes and do-not-disturb schedules | Implemented | 2 | In Windows 11 and macOS | [focus](features/focus.md) |
| Automatic light/dark by sunset | Implemented | 2 | From macOS, adapted for Windows 11 style | [auto-dark](features/auto-dark.md) |
| Theme from wallpaper | Implemented | 1.5 / 2 | From Windows, adapted for macOS style | [theme-from-wallpaper](features/theme-from-wallpaper.md) |
| Settings hub "Desktop" | Implemented | 2 | Original | [settings-hub](features/settings-hub.md) |
| Screenshots, annotation, screen recording | Partial | 3 | In Windows 11 and macOS | [screenshots](features/screenshots.md) |
| OCR text extractor | Implemented | 3 | In Windows 11 and macOS | [ocr](features/ocr.md) |
| Colour picker | Implemented | 3 | In Windows 11 (PowerToys) and macOS | [color-picker](features/color-picker.md) |
| On-screen displays (volume, brightness, mute, keyboard backlight) | Implemented | 3 | In Windows 11 and macOS | [osd](features/osd.md) |
| Quick Look | Partial | 4 | From macOS, adapted for Windows 11 style | [quick-look](features/quick-look.md) |
| Snap layouts and always-on-top | Implemented | 4 | In Windows 11 and macOS (tiling); always-on-top from Windows | [window-layouts](features/window-layouts.md) |
| Shortcut guide (hold Super) | Implemented | 4 | From Windows, adapted for macOS style | [shortcut-guide](features/shortcut-guide.md) |
| Phone integration | Planned | 5 | In Windows 11 and macOS; based on GSConnect (GPL-2.0-or-later) | [phone](features/phone.md) |

The licence of every "Based on" project is re-checked when code is actually taken; the table says
what is intended.

## Keeping specs honest

* A change of behaviour edits the spec in the same pull request.
* Acceptance criteria are numbered and never renumbered; a removed one is marked *(withdrawn)*.
* Stage 1.5 documents this tree refers to: [design tokens](../design-tokens.md) and the
  [theme contract](../theme-contract.md); the settings registry ([settings](../settings.md), generated
  by `just gen`); [translations](../i18n.md); [testing](../testing.md); performance budgets
  ([`tools/perf/budgets.toml`](../../tools/perf/budgets.toml), [harness](../../tools/perf/README.md)).
* Stage 1.5 follow-ups, not blocking: the headless performance job is manual and reports only
  (shared CI runners are too noisy to gate on); accessible names are set by hand, with no automated
  AT-SPI audit yet; dconf is still read and written through the `dconf` tool (see
  [architecture](../architecture.md)).
