# Doctor and crash protection

## Summary

`themes doctor` checks that the desktop's theme layer is healthy (extensions, journal errors,
missing themes/icons/cursors/sounds/wallpapers, inheritance, GNOME version change, shell crashes) and,
with `--fix`, repairs what it can: it disables broken extensions and re-enables repaired ones. After a
GNOME Shell crash that makes GNOME switch every extension off, the manager explains the likely cause
and brings extensions back **only with the user's consent**.

## Status

**Implemented** (stage 0). Origin badge: *Original*. Gaps below (no schema check; doctor writes
state; a stale crash keeps the exit code at 3 for 48 h).

## Reference behaviour

Windows 11 has "Safe Mode" and automatic repair; macOS has Safe Mode and crash reports. GNOME's own
equivalent is `disable-user-extensions`, set by gnome-shell after repeated early crashes. Our doctor is
the explainer and the consented way back, not a replacement for it.

## Our behaviour

### Entry points

| Entry | Behaviour |
|---|---|
| `themes doctor` | read-only report (except the bookkeeping gap G-DOCTOR-3); text or `--json` |
| `themes doctor --fix` | report, then repair; sends a notification when something was fixed, restored, the GNOME version changed, or GNOME disabled extensions (`--no-notify` suppresses) |
| `themes doctor --enable-extensions` | sets `org.gnome.shell disable-user-extensions=false` if it is `true`; prints what it did; then runs the report. Only on explicit request |
| `themes doctor --wait-shell SEC` | wait up to SEC for GNOME Shell on the bus, then let it settle (<= 20 s) unless the screen is locked; used at login |
| `themes-doctor.service` | systemd user oneshot after `gnome-session.target`: `themes doctor --fix --wait-shell 90`, `SuccessExitStatus=3` |
| Window: «Перевірити систему» | same report in a dialog («Система в порядку» or «Результати перевірки», marks ✗ error, ⚠ warning, ℹ info); a «Виправити» button appears if any issue is fixable |
| D-Bus `Doctor(fix)` | `(ok, messages, json)` |

Exit codes: 0 all clear (only `info` issues, or after `--fix` every issue was fixable); 3 problems remain
(not a failure of the tool); 1 the tool itself failed.

### Checks

| Kind | Severity | Condition | Fixable |
|---|---|---|---|
| `interrupted-apply` | error | `state.json` holds the journal (`pending`) of an apply that did not finish (crash, kill, power loss) | yes: completed when its hooks had run, else rolled back |
| `extension` | error | enabled extension in state `ERROR` or `OUT_OF_DATE` (with the shell's error text; without a shell: `metadata.json` does not list the GNOME major version) | yes: disabled and recorded in `state.json` |
| `extension` | warning | enabled but not installed | no |
| `extension` | info | installed but the shell has not loaded it yet (needs a re-login) | no |
| `permanent` | warning | `desk-bridge@desktop-themes-rs` not installed, or installed but disabled (and not recorded broken) | disabled: yes |
| `journal` | warning | extension errors in the journal of the last 30 min for an enabled extension | no |
| `extensions-disabled` | error | `disable-user-extensions=true` | no (consent needed) |
| `shell-crash` | warning | a real (non-headless, non-nested) gnome-shell crash in the last 48 h (from `coredumpctl`) while extensions are not disabled | no |
| `missing-asset` | warning | GTK, icon, cursor, sound or shell theme named by dconf or by an active theme is not installed; the wallpaper file does not exist | no |
| `inherits` | warning | a theme's `INHERITS` is invalid, cyclic, too deep or missing | no |
| `gnome-version` | info | GNOME Shell version differs from the one recorded last time | no |
| `broken-recorded` | info | extension recorded as broken, with date and reason | no |

`--fix` order: (1) disable broken extensions (via the shell when it runs, else straight in dconf) and
record them; (2) re-enable recorded-broken extensions whose GNOME version or extension files changed
since they broke, are installed, and whose `metadata.json` supports this GNOME, **and verify** that
they reach `ACTIVE` within ~4 s (otherwise disable again and update the stamp); (3) re-enable the
permanent bridge extension. Then record `gnome_version`, `last_doctor`, and the last crash.

### Crash protection

When gnome-shell crashes soon after starting (or within ~60 s of extensions being enabled), GNOME sets
`disable-user-extensions=true`; `enabled-extensions` stays untouched but nothing runs.

* `themes apply` must say "applied, extensions are disabled by GNOME" rather than "applied live".
* The report shows the error `extensions-disabled` plus the last real crash (time, signal, top stack
  frame), the theme that was applied just before (from `apply.log`), and the extensions that theme had
  added relative to the previous one: the suspects.
* The window shows the banner «GNOME вимкнув розширення після збою оболонки» with «Увімкнути…». The
  dialog shows the same explanation and offers: «Скасувати» (default), «Перейти на «Fedora» й
  увімкнути» (only when the culprit theme is not `fedora`), «Увімкнути». Nothing re-enables extensions
  without one of the two latter answers.
* Headless test shells (`--headless`, `--nested`) never count as crashes.

## Settings

None. Constants: crash look-back 48 h; journal look-back 30 min; auto-rollback wait 5 s; login wait
90 s.

## Privacy & security

The report names local paths and extension ids only; nothing is uploaded; `coredumpctl info` output is
read, not stored (only the signal, time, pid and one stack frame are kept in `state.json`).
`--fix` never touches the user's own settings, only `enabled-extensions` and the bookkeeping in
`state.json`. Re-enabling after a crash is never automatic.

## Edge cases

* GNOME Shell not running (TTY, not yet started): checks that need the shell fall back to
  `metadata.json` and dconf; the report says "не запущено".
* Screen locked at login: do not wait for extensions to settle (they load after unlock).
* A crash inside the last 48 h with no apply log line: culprit unknown, message omits it.
* The same extension broken on two GNOME versions: the record's `shell_version` and `stamp` are updated,
  it is not retried until one of them changes.
* `coredumpctl` missing (no systemd-coredump): no crash data, no error.
* Two doctor runs at once (login service plus the window): the lock serialises the writes.

## Accessibility

The dialog uses plain text lines with a leading symbol **and** a severity word available to
screen readers (the symbol is not the only carrier: the text must say "error"/"warning" in its
accessible description); the banner is a libadwaita `Banner` (focusable button).

## Performance

`themes doctor` without `--wait-shell` should finish in about 1 s on a normal session (D-Bus calls to the
shell plus `journalctl` and `coredumpctl`); it must not run on the window's main thread (it does not).
The login run is off the critical path (after `gnome-session.target`).

## Gaps found

* **G-DOCTOR-1** *(deskd part resolved)* Doctor checks deskd: warning when its D-Bus service file is
  missing, warning when it owns its name but does not answer or runs another version than `themes`,
  info (not a problem) when it is merely not running yet (D-Bus activation starts it on first use).
  Still open: the GSettings schema check (`io.github.OlehDzhumyk.Desk*`): a missing schema makes the bridge
  fail on enable with "GSettings schema not found" (`bridge/src/extension.ts`). Intended: error `schema`.
* **G-DOCTOR-2** A shell crash stays a `warning` for 48 h, so `themes doctor` exits 3 even though nothing
  can be fixed and the tool already told the user. Intended: report once per crash (remember
  "acknowledged" in `state.json`) and drop to `info`.
* **G-DOCTOR-3** A read-only `themes doctor` writes `state.json` (`gnome_version`, `last_doctor`,
  `last_shell_crash`) in `crates/themes-core/src/doctor.rs` (the "запам'ятати версію" block). Intended:
  only `--fix` and the login run write it.
* **G-DOCTOR-4** `check_journal` and `shell-crash` rely on English phrases of `journalctl` and
  `coredumpctl` output; a localised system may defeat them.

## Acceptance criteria

* AC-DOCTOR-01: Given an enabled extension in `ERROR`, When `themes doctor --fix` runs, Then the
  extension is removed from `enabled-extensions`, recorded in `state.json.broken_extensions`, the
  output lists it under fixed, and a notification is shown.
* AC-DOCTOR-02: Given a recorded-broken extension and the extension's files changed, When
  `themes doctor --fix` runs and the extension loads `ACTIVE`, Then it is re-enabled and removed from
  the broken list; if it errors again it is disabled again and stays recorded.
* AC-DOCTOR-03: Given `disable-user-extensions=true`, When `themes doctor` runs without
  `--enable-extensions`, Then the key is still `true` afterwards and the report has the error
  `extensions-disabled`.
* AC-DOCTOR-04: Given `disable-user-extensions=true` and a recorded crash after `apply macos-glass`,
  When the report is shown, Then it names theme `macos-glass` and the extensions that theme added
  as suspects.
* AC-DOCTOR-05: Given the window with the banner, When the user chooses «Скасувати», Then the key is
  unchanged and no theme is applied.
* AC-DOCTOR-06: Given the banner dialog with a culprit theme other than `fedora`, When the user chooses
  the safe option, Then `fedora` is applied first and extensions are enabled after.
* AC-DOCTOR-07: Given only `info` issues, When `themes doctor` runs, Then the exit code is 0.
* AC-DOCTOR-08: Given a headless test shell crashed, When `themes doctor` runs, Then no `shell-crash`
  issue is reported.
* AC-DOCTOR-09: Given a theme whose `INHERITS` is a missing id, When `themes doctor` runs, Then there is
  one `inherits` warning naming the theme.
* AC-DOCTOR-10: Given `themes doctor` with no `--fix` on a healthy system, When run twice, Then
  `state.json` is unchanged by the second run and (target behaviour, G-DOCTOR-3) by the first.
* AC-DOCTOR-11: Given the schema `io.github.OlehDzhumyk.Desk` is not installed, When `themes doctor`
  runs, Then it reports an error `schema` (target behaviour, G-DOCTOR-1).

## Open questions

1. Should the login service notify when the crash cause is a theme extension, offering one-click
   "switch to safe theme"?
2. Should `--fix` also offer to prune `trash/` and old `backups/`?
3. Machine-readable severity names in the accessible text: translate or keep English tokens?
