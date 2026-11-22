# Emoji and symbol picker

## Summary

A keyboard-first panel to find and insert emoji, symbols (arrows, maths, currency, punctuation) and
text faces, searched by name in Ukrainian and English, with recents and skin tones. Choosing an item
inserts it into the focused window, exactly like the [clipboard](clipboard.md) paste.

## Status

**Planned** (stage 2). Origin badge: *In Windows 11 and macOS*. Data: Unicode CLDR annotations
(Unicode licence, data only); optional integration with the GNOME emoji picker's data. The Windows 11 clipboard
flyout already shows disabled tabs «Емодзі» and «Символи» as placeholders.

## Reference behaviour

* **Windows 11:** `Win+.` (or `Win+;`) opens a panel at the text cursor with tabs: emoji, GIFs, kaomoji, symbols,
  clipboard history; a search field (typing starts the search at once), categories along the top or bottom,
  recently used, skin tone and gender variants; `Esc` closes; the panel stays open for multiple picks until
  dismissed (insert on `Enter` / click).
* **macOS:** `Ctrl+Cmd+Space` opens the Character Viewer (emoji and symbols) at the text cursor: a search
  field, categories on the bottom row, frequently used, skin tones through a long press; inserts on click or
  `Enter`, closes after insert.

## Our behaviour

### Opening

| Skin | Shortcut | Where |
|---|---|---|
| `windows11` | `Super+.` and `Super+;` | next to the text caret (pointer fallback); tab strip: «Емодзі», «Символи» (and «Буфер обміну» in the same flyout: the clipboard flyout and the emoji panel are **one** flyout with tabs, opened on the respective tab) |
| `macos` | `Ctrl+Super+Space` | compact floating panel near the caret: search field on top, grid, category row on the bottom, an expand button for a larger window |
| `default` | `Super+.` | popover at the pointer/caret, same content as `macos` compact |

All skins close with `Esc`, a click outside, or (macos, default) after inserting; `windows11` stays open
after an insert until dismissed, `Shift+Enter` keeps it open in the other skins.

### Content and search

* Categories: Recents, Smileys & people, Animals & nature, Food, Activities, Travel, Objects, Symbols (emoji),
  Flags; Symbols tab: Punctuation, Maths, Currency, Arrows, Latin and Ukrainian extra letters, Shapes, Text faces.
* **Search:** field focused on open; matches CLDR names and keywords in the active UI language **and** English;
  case-folded, diacritics-tolerant; results ranked: exact name, prefix, keyword, then recents first on ties; no
  debounce above 30 ms; works with input methods.
* **Recents:** the last 36 picked, most recent first; **Skin tone:** a default tone in settings; long press (or
  `Shift+Enter` / the menu key) on a person emoji opens the tone strip.
* Grid navigation: arrows move, `Page Up/Down` jump a row block, `Home/End`, `Tab` moves between search, grid and
  categories, `Enter` inserts, `Ctrl+1..9` jump to a category.
* Each cell has a tooltip and screen-reader name (CLDR name in the UI language).

### Insertion

Same mechanism as clipboard paste ([clipboard](clipboard.md) "Activation and paste"): the panel closes, the
bridge sets the clipboard and sends `Shift+Insert` (`Ctrl+Shift+Insert` in terminals) after 120 ms, then **restores
the previous clipboard content** so that the user's clipboard is not clobbered (the emoji is recorded neither
in history nor as the current clipboard afterwards). Preferred path when available: the input method's
commit-string (no clipboard use).

### Animation

Open/close fade <= 100 ms; instant under reduced motion. No emoji animation in the panel.

### Empty and error states

No match: «Нічого не знайдено за «<query>» ». No recents: the Recents category is hidden. Data file missing:
the panel opens with a built-in minimal set and a warning logged.

## Engine and architecture

* **deskd** `Emoji` engine: loads the CLDR dataset (generated at build time into a compact binary file, <= 1 MB),
  answers `Search(query, lang, limit)`, `Recents()`, `Pick(id)`; stores recents in `~/.local/share/deskd/emoji.json`
  (0600). No index in memory when idle beyond the loaded table (<= 2 MB).
* The bridge draws skins, forwards keys, inserts.

## Settings (proposed, `io.github.OlehDzhumyk.Desk.Emoji`)

| Key | Type | Default |
|---|---|---|
| `enabled` | bool | true |
| `toggle-shortcut` | as | per theme (`['<Super>period','<Super>semicolon']` windows11) |
| `skin-tone` | choice `none,light,medium-light,medium,medium-dark,dark` | `none` |
| `remember-recents` | bool | true |
| `close-after-insert` | bool | per skin |

## Privacy & security

Recents are local, clearable, and off with `remember-recents=false`. Nothing is sent. Clipboard-based insertion
must restore the previous clipboard and must not record the emoji in clipboard history (the engines coordinate:
`deskd` ignores the push that carries the marker mime `x-desk-internal`).

## Edge cases

* Apps that do not accept `Shift+Insert`: the commit-string path or a note «Скопійовано» (the emoji stays on the
  clipboard, the user pastes manually) if insertion cannot be confirmed.
* Terminals: `Ctrl+Shift+Insert` as for clipboard; ZWJ sequences render per the terminal's font.
* Emoji newer than the installed font: shown as a box in the grid; hidden if the font reports it missing (best
  effort).
* Password fields: insertion allowed (the user chose it); recents not updated for password-purpose clients.
* Lock screen and overview: inactive in the lock screen; in the overview it works as for the clipboard.
* Right-to-left text: insertion is a single code point sequence.

## Accessibility

Every cell and tab has an accessible name; the grid is a single tab stop with arrow navigation; focus ring visible
at 3:1; reduced motion honoured; the panel scales with `text-scaling-factor`; high contrast uses system
colours.

## Performance

Panel visible <= 16 ms (the grid is virtualised: only visible cells are created); `Search` <= 20 ms for any
query on the full dataset; the dataset loads lazily at first open and is unloaded after 5 minutes idle (no timer
wakeups beyond that single one-shot).

## Acceptance criteria

* AC-EMO-01: Given `windows11` and a text field focused, When `Super+.` is pressed, Then the flyout opens on the
  «Емодзі» tab with the search field focused.
* AC-EMO-02: Given the panel is open, When "усмішка" is typed, Then results include 🙂/😀-class emoji whose Ukrainian
  names or keywords match, and `Enter` inserts the selected one into the text field.
* AC-EMO-03: Given the same query in English "smile", Then the same class of results appears.
* AC-EMO-04: Given the clipboard held text X before, When an emoji is inserted through the clipboard path, Then
  the clipboard holds X again afterwards and X's history entry is unchanged.
* AC-EMO-05: Given `skin-tone=medium` and the emoji "wave", Then the inserted emoji carries the medium tone.
* AC-EMO-06: Given `windows11`, When an emoji is inserted, Then the panel stays open; for `macos` it closes.
* AC-EMO-07: Given `Esc`, Then the panel closes and focus returns to the previous window.
* AC-EMO-08: Given `enable-animations=false`, Then no fade runs.
* AC-EMO-09: Given the Ukrainian layout is active, When `Super+.` is pressed (a keysym binding), Then the panel
  opens.
* AC-EMO-10: Given a terminal is focused, Then the insertion uses `Ctrl+Shift+Insert`.

## Open questions

1. One flyout with tabs (Windows-like) vs. separate panels in every skin; the spec says one flyout for
   `windows11` only.
2. Use the GNOME emoji picker (GTK) data and code instead of a new dataset (principle 5: integrate)?
3. GIF search (Windows 11 has it) needs the network: out of scope by principle 7; confirm.
4. Kaomoji tab: include (Windows has it) or skip?
