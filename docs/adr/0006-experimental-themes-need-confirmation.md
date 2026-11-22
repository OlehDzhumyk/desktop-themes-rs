# 0006. Experimental themes require explicit confirmation

* Status: accepted (the confirmation prompt in `themes apply` and the window is tracked separately;
  the agent hook and docs already follow this decision)
* Date: 2026-11-22

## Context

`macos-glass` (macOS plus the Liquid Glass shader extension) crashed GNOME Shell 51.1 with SIGSEGV
in `St` about 20 seconds after applying, again at the next start, and then GNOME disabled every
extension. The theme is still useful to some users, so removing it is not the answer
([docs/themes.md](../themes.md)).

## Decision

* A theme can mark itself experimental in `theme.conf` (NAME/DESCRIPTION say so).
* Applying such a theme requires an explicit confirmation from the user: a dialog in the window,
  `--yes` on the CLI. It is never applied silently, by default, by mixing, or by an agent on its own
  initiative.
* Agents apply an experimental theme only after the user said yes in the conversation, only in the
  isolated harness, and pass `--yes` explicitly (`.claude/hooks/guard.sh` blocks it otherwise).
* Every apply keeps its rollback; `themes doctor` names the culprit after a crash.

## Consequences

* Good: risky themes stay available without surprising anyone.
* Bad: one more flag and dialog to maintain; the marker is currently a text convention.

## Alternatives considered

* Remove the theme: loses a feature people asked for.
* Warn but proceed: the warning is read after the crash.
