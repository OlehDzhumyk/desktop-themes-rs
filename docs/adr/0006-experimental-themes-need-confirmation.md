# 0006. Experimental themes require explicit confirmation

* Status: accepted, implemented (CLI `--yes`/`-y` with a y/N prompt, window and quick-settings
  dialogs; enforced in `themes_core::apply` and the D-Bus service; `.claude/hooks/guard.sh` for
  agents)
* Date: 2026-11-22

## Context

`macos-glass` (macOS plus the Liquid Glass shader extension) crashed GNOME Shell 51.1 with SIGSEGV
in `St` about 20 seconds after applying, again at the next start, and then GNOME disabled every
extension. The theme is still useful to some users, so removing it is not the answer
([docs/themes.md](../themes.md)).

## Decision

* A theme marks itself experimental in `theme.conf` with `EXPERIMENTAL=true` and an optional
  `WARNING=` text shown in the confirmation.
* Applying such a theme requires an explicit confirmation from the user: a dialog in the window,
  `--yes` on the CLI. It is never applied silently, by default, by mixing, or by an agent on its own
  initiative.
* Agents apply an experimental theme only after the user said yes in the conversation, only in the
  isolated harness, and pass `--yes` explicitly (`.claude/hooks/guard.sh` blocks it otherwise).
* The rule is enforced below the front ends: `themes_core::apply::apply` refuses an experimental
  theme unless `Options::confirmed` is set (a dry run is allowed). The CLI sets it after `--yes`
  or the prompt, the window after its dialog. The D-Bus method `Apply` never sets it and answers
  `AccessDenied`; `ApplyConfirmed` sets it and is called by the quick-settings toggle only after its
  dialog. A front end that cannot tell whether a theme is experimental (for example `Experimental()`
  fails) applies nothing: fail closed.
* Every apply keeps its rollback; `themes doctor` names the culprit after a crash.

## Consequences

* Good: risky themes stay available without surprising anyone.
* Good: a script or an agent that calls the library or `gdbus` directly cannot skip the
  confirmation by accident: it has to choose the confirmed variant.
* Bad: one more flag, one more D-Bus method and three dialogs (CLI, window, quick settings) to keep
  in sync.

## Alternatives considered

* Remove the theme: loses a feature people asked for.
* Warn but proceed: the warning is read after the crash.
