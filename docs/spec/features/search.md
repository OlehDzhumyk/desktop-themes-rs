# Search and launcher

## Summary

One keystroke opens a search box that finds applications, files, settings, calculations and system
actions, and starts what the user picks. Today the `macos` theme uses the third-party extension
**Spotlight** (`spotlight@nin`) with two local patches; `windows11` uses the ArcMenu start menu;
`fedora` uses GNOME's overview search. The plan is to replace the third-party extension with an own
search engine in deskd (provider-based, privacy-preserving) drawn by the bridge in each skin.

## Status

* **Current implementation: Partial.** Works through third-party extensions; its behaviour is not under
  this project's control beyond a theme's dconf keys and two patches.
* **Own search engine: Planned** (stage 4). Origin badge: *Based on Spotlight by itsnin
  (GPL-3.0-or-later)* for the current launcher; the engine itself is *In Windows 11 and macOS*.

## Reference behaviour

* **macOS Spotlight:** `Cmd+Space`; a wide rounded search field in the upper third; results grouped
  (top hit, applications, documents, folders, calculations, conversions, definitions, system settings,
  web suggestions) with the top hit preselected; `Cmd+Enter` reveals in Finder; `Cmd+1..9` pick by number; on macOS 26 it also
  hosts actions, shortcuts and clipboard history.
* **Windows 11:** `Win` key or `Win+S`; the Start menu with pinned apps, recommended items and a search
  box that finds apps, files, settings and the web; the Search flyout has filters (apps, documents, web,
  settings); typing immediately searches.

> **Transitional:** `spotlight@nin` stays only until the own engine below replaces it. The two Shell 51 patches are
> offered upstream regardless.

## Our behaviour: current implementation

| Theme | Component | Open | Notes |
|---|---|---|---|
| `fedora` | GNOME overview search | `Super` tap, or type in the overview | GNOME search providers: apps, files, settings, calculator, ... |
| `macos` | Spotlight (`spotlight@nin`, v15 pinned) | `Super+Space` (`spotlight.dconf` sets `toggle-shortcut=['<Super>space']`, `theme-preference='default'`) | replaces the overview search; positioned 25% from the top; Esc or click outside closes |
| `windows11` | ArcMenu (layout 11, icon button on the panel) | `Super` tap (`arcmenu-hotkey=Super_L`), also `Super+S` opens the application grid | start-menu style launcher in Dash to Panel |

`spotlight@nin` features (from its documentation): reuses GNOME's search infrastructure; apps, files ranked
by usage, calculator with clipboard copy, system actions (lock, suspend), GNOME Settings panels, web search from the
browser; translucent panel in light/dark following the system. Our theme sets only the shortcut and the
theme preference; `Super+Space` must not also toggle the overview (`toggle-overview` is cleared by the
macOS look and behaviour, see [behavior](behavior.md)).

**Local patches** ([patches/README.md](../../../patches/README.md)): (1) wait for `startup-complete`
before taking over the overview search widgets (taking them during the Shell 51 start-up animation
crashed the shell); (2) use `orientation` instead of `vertical` for `St.BoxLayout` (Shell 51). Patches are
applied as diffs to the pinned upstream build and skipped with a warning if they no longer apply.

## Our behaviour: target (own engine)

### Architecture

* **deskd search engine:** aggregates **providers**; no private index of the user's files.
  Providers: applications (desktop entries + usage ranking kept locally), files and folders (the
  desktop's own indexer, `localsearch`/Tracker, queried over D-Bus; never a crawler of ours), GNOME
  Settings panels, calculator and unit/currency-free conversion, system actions (lock, suspend, log out,
  restart, toggle dark mode, toggle private mode), clipboard history (the [clipboard](clipboard.md) engine),
  emoji ([emoji](emoji.md)), windows (open windows via the bridge), and existing GNOME **search
  providers** (`org.gnome.Shell.SearchProvider2`) so third-party apps appear.
* **Bridge:** draws the skinned box and results, forwards keys, activates results.
* Web search is **off by default** and only opens the browser with the query (no network request by us).

### Query language

| Input | Meaning |
|---|---|
| plain text | all providers, ranked |
| `= 2+2*3`, or an expression | calculator; `Enter` copies the result |
| `> text` | system actions only |
| `. text` or `file:` | files only |
| `# text` | clipboard history (macOS skin: `Super+4` opens the panel on this scope) |
| `: text` | emoji |

### Ranking

Exact name match > prefix > word prefix > substring > fuzzy (typo-tolerant, Unicode case folding,
Cyrillic and Latin keyboard layout mix-ups tolerated); ties broken by usage frequency with decay (stored
locally); applications outrank files.

### Skins

* **`macos`:** Spotlight panel, centred, top at 22% of the monitor height, one wide field, grouped
  results with a **top hit** preselected, group titles, a preview pane for files (see
  [quick-look](quick-look.md)), `Up/Down` move, `Tab` jumps to the next group, `Enter` opens, `Ctrl+Enter`
  reveals the file in Files, `Super+1..9` pick that row, `Esc` closes. Shortcut `Super+Space`.
* **`windows11`:** start-menu-like flyout anchored above the taskbar: a search box at the top, a
  filter row (All, Apps, Documents, Settings, Web off), pinned apps grid when the box is empty, results list
  with a details pane. `Super` tap or `Super+S` opens; `Tab` cycles filters; `Enter` opens; `Esc` closes.
* **`default`:** GNOME-native: results appear in the overview; `Super` tap (the engine registers itself as
  a search provider for the overview rather than drawing its own box).

### Keyboard, focus, animation

The field has focus on open; typing starts the search without delay (first results within 50 ms; later
providers append, never reorder what the user is looking at once the user has moved the selection);
`Esc` first clears the field, then closes. Open/close fades at most 120 ms, instant when
`enable-animations` is off. Focus returns to the previously focused window on close.

### Empty and error states

Empty query: recent items and pinned apps (skin-dependent) or nothing. No results: «Нічого не знайдено за
«<query>»». A provider that errors or times out (> 300 ms) is skipped silently for that query and logged.

## Settings

*Proposed* (`io.github.OlehDzhumyk.Desk.Search`): `enabled` (bool, true), `shortcut` (as, set by theme),
`providers` (as, default all but web), `web-search` (bool, false), `max-results` (uint 5..50, 12),
`remember-usage` (bool, true), `files-scope` (as, `~`), `excluded-paths` (as). Today the only knobs are the
extension's `toggle-shortcut` and `theme-preference` (`spotlight.dconf`).

## Privacy & security

Queries are never sent anywhere. File search is delegated to the user's own indexer and respects its
exclusions. The usage ranking (names of launched items and counts) is stored in `~/.local/share/deskd/` (0600),
clearable, off with `remember-usage=false`. Private mode of the clipboard hides clipboard results. Launching an
item starts it with the user's rights, exactly like the stock overview. The calculator evaluates arithmetic
only, never shell or script input.

## Edge cases

* The overview and the launcher must never both react to one key.
* A very long query is truncated at 256 characters for matching.
* IME composition in the field must not trigger search until committed (or must search on preedit
  if the IME allows), and must not lose characters.
* Slow disks or an unavailable indexer: apps and settings still answer instantly.
* Two monitors: the panel appears on the monitor with the pointer.
* Lock screen: inactive.

## Accessibility

All results have accessible names with group names; arrow-key navigation announces the selection; the
box works with screen readers (Orca) and high-contrast; reduced motion honoured.

## Performance

Panel visible <= 16 ms after the shortcut; first results <= 50 ms; the engine adds no idle wakeups; apps
and settings come from in-memory caches built from desktop-entry change notifications (inotify), not
polling; deskd stays <= 20 MB with all providers loaded.

## Gaps found

* **G-SRCH-1** The current macOS launcher is an unmodified third-party extension plus two patches; its
  behaviour on a new Shell release is unpredictable (it already crashed Shell 51 at start-up, hence patch
  1).
* **G-SRCH-2** *(resolved)* the stale comment in `themes/macos/spotlight.dconf` is corrected.
* **G-SRCH-3** `spotlight.dconf` binds the launcher to `Super+Space` in the look module while the
  `windows11` behaviour binds `switch-input-source` to the same key: a macOS look + Windows 11 behaviour mix
  yields two owners; `themes apply` resolves it in favour of the launcher ([behavior](behavior.md) G-BEHAV-2).
* **G-SRCH-4** The Windows 11 theme has no search field of its own (ArcMenu's start menu has one); `Super+S`
  opens the application grid, not search, which differs from Windows.

## Acceptance criteria

* AC-SRCH-01: Given the `macos` look, When `Super+Space` is pressed, Then the launcher opens and the
  overview does not; pressing it again or `Esc` closes it.
* AC-SRCH-02: Given the target engine and an installed application "Text Editor", When "text ed" is typed,
  Then the application is the top hit and `Enter` launches it.
* AC-SRCH-03: Given the query `= 12*(3+4)`, Then the first result is `84` and `Enter` puts `84` on the
  clipboard.
* AC-SRCH-04: Given the query "сдшзищфкв" (the letters of "clipboard" typed on the Ukrainian layout, target), Then
  layout-tolerant matching finds clipboard-related results.
* AC-SRCH-05: Given a provider that never answers, When a query is typed, Then results from the others appear
  within 50 ms and the box never hangs.
* AC-SRCH-06: Given `web-search=false`, When any query is entered, Then no web results are listed and no
  process makes a network request.
* AC-SRCH-07: Given `remember-usage=false`, When an item is launched, Then no usage record is written.
* AC-SRCH-08: Given the box is open and `enable-animations=false`, Then it appears and disappears without
  any transition.
* AC-SRCH-09: Given the pinned patches are applied to `spotlight@nin`, When the Shell starts, Then the
  extension reaches `ACTIVE` and the shell does not crash (current implementation test).
* AC-SRCH-10: Given the box is open on monitor B, Then it is drawn on monitor B.

## Open questions

1. *(Decided)* we build the own search engine in deskd; `spotlight@nin` is **transitional** and is removed from the bundled themes when the engine ships. The two GNOME 51 fixes (`patches/spotlight@nin`) are still sent upstream to itsnin (principle 5).
2. Which indexer is guaranteed on Silverblue (`localsearch`) and what is the fallback when it is disabled?
3. Calculator scope: arithmetic only, or units and currencies (currency needs the network: out)?
4. Should `Super` tap on the Windows skin open search or the start menu with pinned apps?
