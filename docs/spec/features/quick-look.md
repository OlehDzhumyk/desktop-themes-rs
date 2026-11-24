# Quick Look

## Summary

Select a file in Files and press `Space` (`Ctrl+Space` in the Windows 11 skin) to see its contents at once in a
floating preview, without opening an application; arrow keys move through the selected files; `Space` or `Esc`
closes.

## Status

**Partial** (stage 4). Origin badge: *From macOS, adapted for Windows 11 style* (Windows gets it as PowerToys Peek).
Basis: GNOME **Sushi** (GPL-2.0-or-later), integrated rather than reimplemented where it works on Shell 51; our own
skinned previewer replaces or wraps it where it does not (licence compatible with GPL-3.0-or-later; attribution in
CREDITS.md).

Done: `themes quick-look <paths>…` (separate process) with images (fit, zoom), text and code (capped at
`max-text-kb`, truncation notice), folders, the details card, the error card, the per-skin chrome, the keys of
"Content" below except media and pages, the settings with per-theme shortcuts, and the bridge binding of
`quick-look-shortcut` (normal mode only; it previews the local files copied to the clipboard, because Files has no
selection API, and a second press closes the preview). Not yet: the hand-over from Files (`org.gnome.NautilusPreviewer`
D-Bus, the minimal path; owning that name would replace Sushi, so it waits for a way to coexist), the
GStreamer player, PDF/EPUB, archives, Office thumbnails, syntax highlighting, drag to pan, the remembered
`windows11` size, the neighbour in Files' order with a single selection, «Файл змінено» while open, the spinner for remote files, and the `PrivateNetwork` sandbox. No
open/close animation exists yet, so AC-QL-08 holds trivially. Sushi code is not used so far.

## Reference behaviour

* **macOS Quick Look:** `Space` in Finder (and in many dialogs) opens a floating, rounded preview that animates from the
  file's icon; supports images, video and audio with controls, PDF with page scroll, text and code, documents, folders,
  archives (content list); arrows switch files (grid/list navigation continues behind), `Cmd+Y` too; `Return` is not
  used; an "Open with" button and a share button in the title bar; `Esc` or `Space` closes.
* **Windows 11 (PowerToys Peek):** `Ctrl+Space` shows a preview window with the file name, an "Open with" button and
  left/right arrows for the selection.

## Our behaviour

### Opening

| Skin | Key | Window |
|---|---|---|
| `macos` | `Space` with a file selected in Files (also in GTK file chooser dialogs when the host allows); the global shortcut is `Super+Y` (Cmd+Y) | borderless, rounded, centred over the Files window, dark title strip with the name, «Відкрити в…», no minimise |
| `windows11` | `Ctrl+Space` | titled window with the file name, icon, «Відкрити», `<` `>` buttons; remembers its size |
| `default` | `Space` (Files and Sushi; no global shortcut of ours in the `fedora` theme) | Sushi-like plain window |

The previewer is a **separate process** (never in the shell): the bridge only sees the key and asks deskd to start
it; this keeps a crashing decoder away from the shell.

### Content

| Type | Preview |
|---|---|
| Images (incl. RAW via system loaders, SVG as image) | fit-to-window, `+`/`-` zoom, `0` fit, drag to pan |
| Video, audio | built-in player (GStreamer): play/pause with `K` or the button (plain `Space` closes, as in macOS); seek with `Shift+Left/Right` (plain arrows go to the next file); position shown |
| PDF, EPUB | page view, `Page Up/Down`, scroll |
| Text, code, markdown, JSON | syntax-highlighted read-only view; first 1 MiB only, then «Показано початок файлу» |
| Folders | list of contents with sizes |
| Archives | list of entries (no extraction) |
| Office documents | thumbnail/first page when a converter is installed, else a metadata card |
| Other / unknown | icon, name, size, type, dates, permissions |

Arrow keys (`Left/Right/Up/Down`) move to the previous/next **selected** file; with a single selection, to the
neighbouring file in Files' order (Files' selection follows). `Return` opens in the default app and closes; `Esc`
and `Space` close; `Cmd+I`-like info: `I` toggles a metadata strip.

### Focus and animation

Opening gives the previewer keyboard focus; closing returns focus to Files with the selection intact. Open/close:
scale + fade from the file's position, <= 150 ms; instant with reduced motion.

### Empty and error states

Unreadable file: «Не вдається прочитати файл» with the reason (permission, missing). Decoder failure: the metadata card
with «Перегляд недоступний». Remote/slow location: placeholder with a spinner (a label when reduced motion) and a
10 s timeout.

## Settings (`io.github.OlehDzhumyk.Desk.QuickLook`)

`enabled` (true), `quick-look-shortcut` (accelerators, default `<Control>space`; per theme: `windows11`
Ctrl+Space, `macos` Super+Y, `fedora` none; override `quick-look-toggle` in `io.github.OlehDzhumyk.Desk.Shortcuts`),
`max-text-kb` (uint 64..8192 step 64, 1024), `autoplay-media` (false, reserved for the player), `follow-selection`
(true). Plain `Space` is never a shell keybinding: those are global and would eat every typed space; `Space` in
Files needs the Files hand-over.

## Privacy & security

Files are **never executed**; scripts are shown as text; HTML is shown as source, never rendered with scripts;
decoders run in the previewer process with no network (`PrivateNetwork`) and a read-only filesystem view of
the file's folder where the sandbox allows. Thumbnails use the system thumbnail cache only. No file name or
content is logged.

## Edge cases

* File deleted or renamed while open: the preview shows «Файл змінено»; arrows still move on.
* Selection of 1000 files: next/previous stays O(1); a counter «3 з 1000» is shown.
* Symbolic links: previews the target, the title shows the link name.
* Huge media: streamed, never loaded whole.
* `Space` typed in a rename or search field in Files: not intercepted (only when a file row has focus).
* Password-protected archives/PDFs: metadata card plus «Захищено паролем».
* Sandboxed (Flatpak) Files: needs portal access; documented limitation.

## Accessibility

Preview content has text alternatives where the format provides them (PDF text, image alt in metadata); the window
has a title announced on open; all controls keyboard-reachable; zoom is independent of `text-scaling-factor`; reduced
motion honoured.

## Performance

Window visible <= 100 ms for images and text (the previewer process is pre-forked lazily on first use and ends after
60 s idle), <= 16 ms is not required because it is a window, not a panel; memory limited to 512 MB per preview; deskd
unaffected.

## Acceptance criteria

* AC-QL-01: Given `macos` and a PNG selected in Files, When `Space` is pressed, Then a preview of the image appears
  within 150 ms; `Space` again closes it and Files has the focus.
* AC-QL-02: Given three files selected, When `Right` is pressed twice, Then the third file is shown and the title says
  "3 з 3".
* AC-QL-03: Given `windows11`, When `Ctrl+Space` is pressed, Then the Peek-style window opens; plain `Space` does not.
* AC-QL-04: Given a 50 MB text file, Then only the first 1 MiB is shown with the truncation notice.
* AC-QL-05: Given a shell script, Then it is shown as text and is not executed.
* AC-QL-06: Given an unreadable file, Then the error card is shown, the previewer does not crash.
* AC-QL-07: Given the focus in the rename entry, When `Space` is pressed, Then a space is typed and no preview opens.
* AC-QL-08: Given reduced motion, Then no scale or fade runs.
* AC-QL-09: Given the previewer crashes, Then the shell and Files keep running.

## Open questions

1. Can Files (Nautilus) be told to hand the key to us, or do we only integrate Sushi's D-Bus interface
   (`org.gnome.NautilusPreviewer`)? The latter is the minimal path.
2. Previews for Office files need LibreOffice: optional dependency or skip?
3. Which of the two Peek/Quick Look key sets applies in the `default` skin?
