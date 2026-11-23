# OCR text extractor

## Summary

Select any region of the screen and the text in it is recognised **locally** and copied to the clipboard.
Works on images, videos, remote desktops, PDFs rendered as pictures: anything visible.

## Status

**Planned** (stage 3). Origin badge: *In Windows 11 and macOS*. Engine: Tesseract (Apache-2.0), run as a
child process; language data from the distribution's `tesseract-langpack-*` packages or a user-installed
`tessdata` directory. No bundled models.

## Reference behaviour

* **Windows 11:** PowerToys "Text Extractor" (`Win+Shift+T`): the screen dims, the user drags a region, the text is
  copied to the clipboard; the Snipping Tool also offers "Text actions" on a capture.
* **macOS:** Live Text: text in images is selectable in Preview, Quick Look and screenshots; the Screenshot
  thumbnail offers "copy text"; no global region OCR key.

## Our behaviour

### Flow

1. Shortcut (`windows11` `Shift+Super+T`; `macos` `Ctrl+Shift+Super+T`; `default` `Shift+Super+T`) or the "Text" mode in
   the [screenshots](screenshots.md) toolbar (the macOS skin shows it as "Copy text" in the thumbnail and editor as
   well, where Live Text lives).
2. Region selection exactly as in screenshots (same overlay, `Esc` cancels).
3. The pixels are written to a temp file in `$XDG_RUNTIME_DIR/deskd/ocr/` (0600), `tesseract` runs with the
   configured languages, the file is deleted immediately, even on failure.
4. The text (paragraph breaks kept, hyphenation at line ends joined, trailing whitespace trimmed) is placed on the
   clipboard and a toast shows the first line: «Скопійовано текст: <first 60 characters>…».
5. Whether the result enters clipboard history follows `record-in-history` (default true; private mode wins).

### Languages

Default `ukr+eng`; `languages` setting lists installed codes only; missing language data: toast
«Немає даних мови <code>: встановіть tesseract-langpack-<code>» and OCR runs with what is available.

### Empty and error states

No text: «Тексту не знайдено» (nothing copied, clipboard untouched). Tesseract missing: «Для розпізнавання потрібен
tesseract» with the package name; the shortcut still opens the overlay only if the engine exists (else it shows this
message at once). Timeout 20 s: process killed, «Розпізнавання перервано».

### Keyboard, focus, animation

Same as the screenshot overlay; during recognition the dimmed overlay is replaced by a small spinner toast
(a static label when reduced motion). Focus returns to the previous window.

## Settings (proposed, `io.github.OlehDzhumyk.Desk.Ocr`)

`enabled` (true), `languages` (as, `['ukr','eng']`), `record-in-history` (true), `keep-line-breaks` (true),
`ocr-shortcut` (per theme, overridable in `io.github.OlehDzhumyk.Desk.Shortcuts` `ocr-toggle`; not
`toggle-shortcut`, because shell keybinding names are global), `max-megapixels` (uint 1..50, 20).

## Privacy & security

**Local only**: no cloud OCR, no telemetry, no network access by the process (it is started without network where
the sandbox permits: `PrivateNetwork`/`unshare -n`). Images exist only in the runtime directory for the time of
recognition. The recognised text is as sensitive as the screen: password managers and other windows are
captured if visible; `private mode` of the clipboard prevents history recording.

## Edge cases

* Tiny region (< 8 px): ignored. Huge region: refused above `max-megapixels` with a message.
* Rotated or curved text, handwriting: best effort; low-contrast text is upscaled 2x (and inverted if dark
  background) before recognition.
* Right-to-left or mixed scripts: only installed languages are recognised.
* Two monitors with different scaling: region read at physical pixels.
* Concurrent OCR requests: one at a time; a second shortcut press while running is ignored.

## Accessibility

Provides text from inaccessible pixels (a feature for screen-reader users too): the result toast is announced;
the whole flow is keyboard operable; contrast of the overlay per tokens.

## Performance

Overlay visible <= 16 ms; recognition of a typical 1080p region <= 2 s on the reference laptop; idle cost is
zero (no daemon: the engine is a child process spawned per request); child memory is transient and not counted in the
deskd 20 MB budget, but is limited with `RLIMIT_AS` of 1 GB.

## Acceptance criteria

* AC-OCR-01: Given an image with the text "Привіт, world" on screen, When the region is selected, Then the clipboard
  contains that text (case and punctuation preserved) within 3 s.
* AC-OCR-02: Given no text in the region, Then the clipboard is unchanged and the toast says no text was found.
* AC-OCR-03: Given tesseract is missing, Then the message names the package and no overlay stays open.
* AC-OCR-04: Given recognition finished or failed, Then no file remains in `$XDG_RUNTIME_DIR/deskd/ocr/`.
* AC-OCR-05: Given private mode, Then the recognised text is on the clipboard but not in history.
* AC-OCR-06: Given the process makes a network call (test with network blocked), Then OCR still succeeds.
* AC-OCR-07: Given `Esc` during selection, Then nothing runs.
* AC-OCR-08: Given the Ukrainian language data is missing, Then English recognition still works and the
  toast names the missing data.

## Open questions

1. Tesseract vs. a newer local engine (PaddleOCR/ONNX): licensing, size, accuracy on Ukrainian.
2. Should the macOS skin also offer in-place selectable text on screenshots (Live Text style) instead of only copy?
3. Provide QR-code reading in the same mode?
