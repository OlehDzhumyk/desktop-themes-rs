# Clipboard history

## Summary

A history of what the user copied (text and images) with pins, search, deletion and a private mode,
opened with a shortcut and shown in the skin of the current theme: a flyout next to the text cursor
(`windows11`), a Spotlight-like panel (`macos`), a compact pointer menu plus a quick-settings tile
(`default`). Choosing an item pastes it into the window that had the focus.

## Status

**Implemented** (stage 1, on `main`). Origin badge: *In Windows 11 and macOS*; capture, paste and image
handling *based on Clipboard Indicator* (MIT), see [CREDITS.md](../../../CREDITS.md). Gaps below.

## Reference behaviour

* **Windows 11:** `Win+V` opens a flyout at the text cursor with tabs (emoji, GIFs, symbols, clipboard
  history), a list of recent items as cards with a pin button and a "..." menu (pin, delete), "Clear all"
  that keeps pinned items; pinned items survive restarts; a limited number of items; history is off
  until the user turns it on; cross-device sync is opt-in. No search in the clipboard tab.
* **macOS 26:** no standalone clipboard history key; clipboard history appears inside the Spotlight
  panel (a large search field, a list with relative times, short retention). Items of password managers
  are marked concealed by the source and are not recorded by well-behaved history tools.

Our skins follow these idioms; we are **not** syncing anything (see Privacy).

## Our behaviour

### Opening and closing

| Skin | Open/close | Where |
|---|---|---|
| `windows11` | `Super+V` (toggles) | next to the text caret (below it, +6 px) if the focused app reported a caret rectangle on this monitor, else at the pointer (+8 px); horizontally centred on that point; shifted to stay >= 12 px inside the work area; if it would overflow the bottom it opens **above** the point |
| `macos` | `Shift+Super+V` | horizontally centred on the current monitor, top at 22% of its height |
| `default` | `Shift+Super+V`, or the quick-settings tile «Буфер обміну» | at the pointer (+8 px), same fitting rule |

The shortcut is the key `io.github.OlehDzhumyk.Desk.Clipboard toggle-shortcut`, set by the theme's
`desk.dconf` (see [behavior](behavior.md) for ownership: in the `windows11` behaviour `Super+V` is
free because the notification list moved to `Super+N`; in `macos` and stock GNOME `Super+V` stays the
notification list). The binding works in the normal and overview shell modes, not on the lock screen,
and is removed when the feature is disabled. Auto-repeat is ignored (holding the key does not flicker).

The panel opens on the **current monitor**. It takes a modal keyboard grab; `Esc`, a click outside
the panel, choosing an item, or pressing the shortcut again closes it and returns the grab (and the
keyboard focus) to the previously focused window.

### Items and the list

* Newest first. Text items show one line (whitespace collapsed); images show a thumbnail and a label
  «Зображення PNG 640×480». Pinned items show a pin icon.
* `default`/`macos`: list rows; `macos` adds a preview pane (up to 1200 characters of text or the image up
  to 220 px, then "<n> симв. · <relative time>[ · закріплено]") and a hint bar (`↵` paste, `Ctrl+P` pin,
  `Ctrl+⌫` delete, `Esc` close) plus a settings button. `default` is the same list, up to 12 rows, no
  preview and no hint bar.
* `windows11`: tabs «Буфер обміну» (active), «Емодзі» and «Символи» (disabled until
  [emoji](emoji.md) lands); a title row with «Очистити все» (visible only if there is at least one
  unpinned item); cards of up to 3 lines (first 400 characters) or a 96 px image; each card has a
  pin button and a "..." button that expands to delete + pin.
* Relative times (macos): "щойно", "n хв тому", "n год тому", "n д тому".
* Selection starts at the first row each time the panel opens; the search field is cleared on open.
* The mouse pointer moving over a row selects it; a click activates it.

### Keyboard (all skins)

| Key | Action |
|---|---|
| `Up` / `Down` | previous / next item (stops at the ends) |
| `Page Up` / `Page Down` | 5 items |
| `Enter` | activate (paste) the selected item |
| `Esc` | close |
| `Ctrl+P` | pin / unpin the selected item |
| `Delete` (`windows11`) | delete the selected item |
| `Ctrl+Backspace` or `Ctrl+Delete` (`macos`, `default`) | delete the selected item |
| typing (`macos`, `default`) | search (focus is in the search field, arrows and Enter still work while typing) |

`windows11` has no search field (as on Windows 11).

### Search

Case-insensitive substring match over the item text (an image's label counts as its text), 80 ms
after the last keystroke; newest first; the selection returns to the top. Empty query shows everything.
Empty-result text: «Нічого не знайдено за «<query>»».

### Activation and paste

1. The panel closes, then deskd's `Activate(id)` returns the content and moves the item to the top.
2. The bridge puts the content on the clipboard (text as text; image bytes with their MIME type).
3. If `paste-on-select` is on, **after 120 ms** (time for the focus to return) it sends `Shift+Insert`,
   or `Ctrl+Shift+Insert` when the focused client's input purpose is `TERMINAL`. With `paste-on-select`
   off the item is only put on the clipboard.
4. Re-capture of our own clipboard write is harmless: the same content is a duplicate and only bumps.

Only the plain-text flavour of text is stored; pasting an item gives plain text (rich formatting,
HTML and file lists are not kept).

### Capture (bridge)

The shell's `Meta.Selection::owner-changed` for the clipboard selection triggers one read. Reads are
**serialised**: a change while reading schedules exactly one more read afterwards, so a burst of
copies records the first read and the latest value; values overwritten before they could be read are
not recorded. Order of types: `text/plain;charset=utf-8`, `UTF8_STRING`, `text/plain`, `STRING`, then (if
images are on) `image/png`, `jpeg`, `jpg`, `gif`, `webp`, `svg+xml`; the first type that the source
offers and returns non-empty is stored. Text wins when a source offers both. `UTF8_STRING` is stored as
UTF-8 text (works around gnome-shell issue 8233). The focused window's WM class at read time is passed
to deskd as the source application.

### Engine rules (deskd, binding)

* Skipped (silently, `Push` returns 0): feature disabled; private mode; **any** offered type equal
  to `x-kde-passwordManagerHint` or `application/x-nspasteboard-concealed-type` (case-insensitive,
  and the bridge does not even read the content); source application in `excluded-apps` (case-insensitive
  exact match of WM class or app id); empty content; text of only whitespace; text larger than
  **256 KiB**; an image when images are off or larger than `image-max-kb`; any other MIME type.
* **Duplicates** (same kind and same bytes) are not added again: the existing item moves to the top
  with a new time; its pin is kept.
* **Trimming:** after each add or unpin, the oldest *unpinned* items beyond `history-size` are
  removed (image files too). Pinned items are never trimmed and do not count.
* `Clear(keep_pinned)`: removes all unpinned items (and all pinned ones when `false`). The panel's
  «Очистити все» and the quick-settings «Очистити історію» keep pinned; the settings window's «Очистити
  історію» removes everything, after a confirmation («Буде видалено всі записи, зокрема закріплені.
  Це не можна скасувати.»).
* Ids are unique, never reused within a store.
* Storage: text history in `~/.local/share/deskd/clipboard/history.json` (dir 0700, file 0600,
  written atomically via temp file + rename, debounced: 2 s after the last change, at most 10 s after
  the first, and flushed on SIGTERM/SIGINT/SIGHUP so a logout loses nothing) when `persist` is on; images in
  `$XDG_RUNTIME_DIR/deskd/clipboard/images` (gone at session end) unless `persist` and `persist-images`
  are both on, then in `~/.local/share/deskd/clipboard/images`. Changing those settings moves files
  accordingly; turning `persist` off deletes `history.json`. A corrupt store is ignored with a log
  line; image items whose file is missing are dropped at load; orphan image files are deleted.
* Changes emit `Changed` (and property changes) so open panels refresh.

### Quick-settings tile (`default` skin only)

Tile «Буфер обміну», checked when not private; subtitle «приватний режим» when private. Menu:
the 8 latest items (60 characters each, pinned marked with a check), «Приватний режим» switch, «Очистити
історію», «Налаштування буфера обміну…». «Історія порожня» / «deskd недоступний» as insensitive rows.

### Empty and error states

| State | Text (`windows11`; others similar) |
|---|---|
| Empty | «Тут з'являтимуться скопійовані елементи. Натисніть Ctrl+C, щоб скопіювати.» (macos/default: «Історія порожня. Скопіюйте щось (Ctrl+C), і воно з'явиться тут.») |
| Private mode | «Приватний режим увімкнено: скопійоване не зберігається.» (macos/default: «Приватний режим: нічого не записується») |
| deskd unreachable | the list is empty and a warning is logged; D-Bus activation starts deskd on the first call; the panel must stay usable |

### Animation

None today (the panels appear and disappear instantly). Any animation added later (fade/scale on open,
at most 120 ms) must be disabled when `enable-animations` is off, and must not delay input or the
paste.

## Settings

Schema `io.github.OlehDzhumyk.Desk.Clipboard` (path `/io/github/OlehDzhumyk/Desk/clipboard/`) and
`io.github.OlehDzhumyk.Desk skin`:

| Key | Type | Default | Range |
|---|---|---|---|
| `enabled` | bool | `true` | |
| `history-size` | uint | 50 | 5..500 (unpinned items) |
| `images` | bool | `true` | |
| `image-max-kb` | uint | 8192 | 64..65536 |
| `persist` | bool | `true` | |
| `persist-images` | bool | `false` | |
| `private-mode` | bool | `false` | |
| `paste-on-select` | bool | `true` | |
| `toggle-shortcut` | as | `['<Shift><Super>v']` | set by the theme (read-only in the window) |
| `excluded-apps` | as | KeePassXC, Bitwarden, 1Password, Secrets, Seahorse, Proton Pass ids | gsettings only today |
| `skin` (parent schema) | choice | `default` | `default` `macos` `windows11`; set by the theme |

The window page `--page clipboard` shows History, Privacy, "How to open" and a deskd status row
(«Служба deskd»: "працює; записів в історії: n").

## Privacy & security

* Local only; no network. History in a 0600 file in the user's data directory; images session-only
  by default. The user can disable persistence, use private mode, and clear everything.
* Secrets: hinted content and password-manager windows are never read or stored. Not a guarantee for
  content that carries no hint (a password copied from a web page): the user must use private mode;
  the spec's job is to make that one click away (tile switch, window switch).
* D-Bus is the session bus; any process of the user can call deskd. History is as private as the user's
  session; sandboxed apps cannot reach it unless granted the bus name.
* Logs must never contain item text (today they do not).

## Edge cases

| Case | Required behaviour |
|---|---|
| Huge text (> 256 KiB) | skipped, not truncated (a truncated paste would corrupt data); no error shown; the clipboard itself still has it |
| Huge image (> `image-max-kb`) | skipped; same |
| Image + text offered | text is recorded, image is not |
| HTML / rich text | recorded as plain text only |
| File copied in Files | not recorded as a file (no `text/uri-list` support); a text form, if offered, is recorded |
| Password manager hint | not read, not stored, no trace in the list or in logs |
| Password manager as the focused window | skipped by `excluded-apps` |
| Private mode on | nothing recorded; existing items can still be browsed and pasted; the tile says so |
| Rapid copies (several per second) | no crash, no duplicate rows, the latest value is always recorded; intermediate values may be lost (documented above) |
| Same text copied again | moves to the top, keeps pin, no duplicate |
| Paste into a terminal | `Ctrl+Shift+Insert` when the client reports the terminal purpose; terminals that do not report it get `Shift+Insert`, which most terminals also accept |
| Paste into an X11 (XWayland) app that binds `Shift+Insert` to the PRIMARY selection | may paste the wrong text; documented limitation; `paste-on-select` off is the workaround |
| Wayland focus-stealing limits | the shell is the compositor, so it may hand focus back and synthesise keys; but if the previous window is gone or focus moves within the 120 ms, the paste goes to whatever has the focus, never to a hidden window |
| Focus was on the shell (overview, panel menus) | paste goes to the focused shell element; harmless |
| Lock screen | shortcut inactive; capture continues for the session clipboard |
| Multi-monitor | panel on the monitor with the pointer/focus (`currentMonitor`) |
| Caret unknown (many GTK4/Wayland apps report none) | falls back to the pointer |
| deskd restarted while a panel is open | the next call re-activates it; the list refreshes on `Changed` |
| Corrupt or unwritable store | deskd keeps working in memory, logs, and does not crash |
| Setting changes while running | applied live (trim, move images, delete store) |

## Accessibility

* Fully operable from the keyboard (above); the focus returns to the previous window on close.
* Reduced motion: no animation today; none may be added without the switch.
* **Required** and not yet met (see gaps): every control has an accessible name and role; selection is
  exposed to assistive technologies; contrast >= 4.5:1 in dark and light schemes; the accent and
  `color-scheme` are honoured; text scales with `text-scaling-factor`; the panel is announced when it
  opens.

## Performance

Panel visible <= 16 ms after the shortcut (the panel is built once and shown/hidden; the list is
filled asynchronously); list filled <= 50 ms; `List` over 500 items must not block deskd (linear scan is
fine); deskd <= 20 MB resident after 200 pushes; capture adds no polling (event-driven). See
[principles](../principles.md#performance-budgets).

## Gaps found

* **G-CLIP-1** Source application is the *focused window's* WM class at read time
  (`bridge/src/clipboard/watcher.ts`), not the owner of the selection: a copy made by a background
  process or while the shell has the focus is attributed to the wrong (or no) app, so `excluded-apps`
  cannot be relied on for those.
* **G-CLIP-2** Windows flyout: buttons are focusable but `Tab` is not handled and the key focus stays on the
  backdrop (`winFlyout.ts` `focusInitial`), so the pin, "..." and delete buttons are unreachable by
  keyboard (only `Ctrl+P`/`Delete` work); rows are `St.Button` with `can_focus: false`, no accessible
  name/role, and the selected state is only a style class (`panel.ts`). Fails the accessibility
  principle.
* **G-CLIP-3** `bridge/static/stylesheet.css` is dark-only with fixed colours and a hard-coded accent
  `#60cdff`: no light scheme, no GNOME accent, no high contrast (principle 6). Design tokens (stage
  1.5) are expected to replace it.
* **G-CLIP-4** The caret position reads the private `Main.inputMethod._cursorRect`
  (`panel.ts` `_anchorPoint`), which can disappear in a Shell update; the pointer fallback hides the
  failure.
* **G-CLIP-5** *(resolved)* `Entry.hash` was std `DefaultHasher`, whose algorithm Rust does not promise to
  keep. It is now FNV-1a 64 (`desk_settings::stable_hash`); store version 2, and a version 1 history has its
  hashes recomputed at load. The thumbnail cache of the window uses the same hash (old thumbnails are purged).
* **G-CLIP-6** Windows flyout: the pin button uses the same icon for pinned and unpinned
  (`it.pinned ? 'view-pin-symbolic' : 'view-pin-symbolic'`), the state is only a style class; `_expanded`
  keeps its index after a delete, so the next card appears expanded.
* **G-CLIP-7** Skipped items give no feedback at all; for *too large* content a short, non-sensitive hint
  would help (not for privacy skips, which must stay silent). See open questions.
* **G-CLIP-8** All panel strings are hard-coded Ukrainian (`panel.ts`, `listPanel.ts`, `winFlyout.ts`).
* **G-CLIP-9** `excluded-apps` is editable only with `gsettings`; the hub should expose it (see
  [settings-hub](settings-hub.md)).

## Acceptance criteria

* AC-CLIP-01: Given the `windows11` skin and three texts copied in order A, B, C, When `Super+V` is pressed,
  Then the panel shows C, B, A top to bottom and C is selected.
* AC-CLIP-02: Given the panel is open, When `Down` then `Enter` are pressed with a text field focused
  before opening, Then B is pasted into that field and the panel is closed.
* AC-CLIP-03: Given the panel is open, When `Esc` is pressed, Then the panel is hidden and the previous
  window has the keyboard focus again.
* AC-CLIP-04: Given text A is in the history, When A is copied again, Then the list still has one A, at
  the top, with its previous pinned state.
* AC-CLIP-05: Given `history-size` = 5 and 7 distinct texts with one pinned, When all are copied, Then
  the list has the pinned item plus the 5 newest unpinned ones.
* AC-CLIP-06: Given a source offering `x-kde-passwordManagerHint`, When it copies a text, Then
  `List("",0)` is unchanged and `Count` is unchanged.
* AC-CLIP-07: Given the focused app's WM class equals an entry of `excluded-apps` in another letter case,
  When it copies, Then nothing is recorded.
* AC-CLIP-08: Given private mode, When any text is copied, Then nothing is recorded; When private mode
  is turned off and another text is copied, Then only that one is recorded.
* AC-CLIP-09: Given a text of 256 KiB + 1 byte, When it is copied, Then it is not recorded; a text of exactly
  256 KiB is recorded.
* AC-CLIP-10: Given `images` on and a 100 KiB PNG, When copied, Then an image item with the label
  «Зображення PNG WxH» exists and its file is in `$XDG_RUNTIME_DIR/deskd/clipboard/images`; with `images`
  off it is not recorded.
* AC-CLIP-11: Given `persist` on, When deskd restarts, Then the text items and pins are back and image
  items are gone (unless `persist-images`).
* AC-CLIP-12: Given `persist` is turned off, Then `history.json` is deleted.
* AC-CLIP-13: Given an item is pinned, When «Очистити все» is chosen, Then only pinned items remain;
  When the window's «Очистити історію» is confirmed, Then the list is empty.
* AC-CLIP-14: Given the `macos` skin and the query "second", When typed, Then only items containing
  "second" (any case) are listed.
* AC-CLIP-15: Given a terminal-purpose client is focused, When an item is activated with
  `paste-on-select`, Then `Ctrl+Shift+Insert` is sent; otherwise `Shift+Insert`; with the setting off no key
  is sent.
* AC-CLIP-16: Given 20 copies in one second, When they finish, Then deskd is still responsive, the last
  copy is the newest item, and there are no duplicates.
* AC-CLIP-17: Given the history file has permissions wider than 0600 (set by hand), When deskd next
  saves, Then it is 0600 again.
* AC-CLIP-18: Given the `windows11` skin, When the panel is opened by the keyboard, Then every button
  can be reached with `Tab` and has an accessible name (target, G-CLIP-2).
* AC-CLIP-19: Given `color-scheme` is `prefer-light`, When the panel opens, Then text/background
  contrast is >= 4.5:1 (target, G-CLIP-3).
* AC-CLIP-20: Given `Super+V` is pressed twice quickly, Then the panel is closed (toggle), never doubled.

## Open questions

1. Retention: should unpinned items expire after N hours/days (macOS-like), with a setting?
2. Should "too large" skips show a toast like «Занадто великий вміст не записано»?
3. Should we record multiple flavours per item (text + HTML) so paste can keep formatting?
4. Where does the user edit `excluded-apps`: a list editor on the clipboard page, or "Never record
   from this app" in the item "..." menu?
5. Sync across the user's own devices (via [phone](phone.md))? Default answer: no.
