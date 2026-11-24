# Screenshots, annotation and screen recording

## Summary

Capture a region, a window or the whole screen with one shortcut, optionally after a delay; mark the
picture up (pen, highlighter, arrow, shapes, text, redaction, crop); save it, copy it to the clipboard or
share it by file; record the screen or a region to video. It builds on GNOME Shell's own screenshot and
screencast machinery and adds the annotation editor and the per-skin toolbar and preview.

## Status

**Partial** (stage 3, with [ocr](ocr.md) and [color-picker](color-picker.md): one capture family). Done: deskd
saving and annotation rendering (`io.github.OlehDzhumyk.Desk.Capture`), the bridge toolbar, region and screen
capture, delay countdown, clipboard copy, skin preview and the full-screen recording toggle (AC-SHOT-01..04,
06..10 on the bridge side). Not yet: the annotation editor (`themes capture-edit`, AC-SHOT-05), window mode,
region recording, `record-format` and `record-audio` (GNOME's screencast service picks the encoder and records no
sound). Origin badge:
*In Windows 11 and macOS*. The capture itself is GNOME Shell's (GPL-2.0-or-later, used as-is through
`org.gnome.Shell.Screenshot` / the built-in UI); annotation candidates to integrate or port with attribution:
Satty (MPL-2.0), Swappy (MIT); licences re-checked at integration.

## Reference behaviour

* **Windows 11:** `Win+Shift+S` dims the screen and shows a small toolbar (rectangle, freeform, window, full
  screen; a video mode); the capture goes to the clipboard and a notification opens the Snipping Tool editor
  (pen, highlighter, ruler, crop, text actions); `Win+Shift+R` records a region; `Print` opens the same tool.
* **macOS:** `Shift+Cmd+3` full screen, `Shift+Cmd+4` region (then `Space` for a window), `Shift+Cmd+5`
  toolbar (capture modes, record modes, a timer 5/10 s, a save-location menu, "Options", show mouse pointer); a
  floating thumbnail appears at a corner for a few seconds: click to open Markup, swipe to save, `Ctrl` to copy.

## Our behaviour

### Shortcuts (see [behavior](behavior.md) registry; user-overridable)

| Action | `windows11` | `macos` | `default` |
|---|---|---|---|
| Region capture | `Shift+Super+S` (also `Print` opens the toolbar) | `Shift+Super+4` | `Print` (GNOME UI) |
| Full screen | `Super+Print` | `Shift+Super+3` | `Shift+Print` |
| Toolbar (modes, timer, record) | `Print` | `Shift+Super+5` | `Print` |
| Start/stop recording | `Shift+Super+R` | `Shift+Super+5` then Record; stop with the same keys or the panel indicator | GNOME's `Ctrl+Shift+Alt+R` |

### Flow

1. The shortcut dims the screen (all monitors) and shows the skin's toolbar; `Esc` cancels at any time.
2. **Region:** drag, adjust handles, `Enter` or release confirms; `Space` while dragging moves the region; `Tab`
   cycles Region / Window / Screen modes. **Window:** hover highlights, click captures. **Screen:** `Enter`.
   Delay: off / 3 s / 10 s with a visible countdown.
3. Result: PNG saved to `~/Pictures/Screenshots` (name `Screenshot <date> <time>.png`; the folder is a
   setting) **and** copied to the clipboard (setting `copy-to-clipboard`, default on). Then the skin shows:
   * `windows11`: a notification with «Редагувати» (and the image preview); activating opens the editor.
   * `macos`: a floating thumbnail (bottom right) for 6 s; click opens the editor; swipe/`Esc` dismisses; it
     stays while the pointer is on it; it is not shown with `show-preview=false`.
   * `default`: GNOME's notification.
4. **Editor** (a GTK 4 window in the skin's style, launched by deskd): tools Select, Pen, Highlighter, Arrow, Line,
   Rectangle, Ellipse, Text, **Redact** (pixelate or solid fill, destructive at save), Crop, colours (accent first),
   widths, Undo/Redo (`Ctrl+Z`, `Ctrl+Shift+Z`), zoom, Save (`Ctrl+S`), Copy (`Ctrl+C`), Save as, Esc closes with a
   «Зберегти зміни?» question only if edited.
5. **Recording:** area or screen, optional system audio and microphone (off by default), pointer shown (setting);
   encoded by GNOME's screencast (WebM/VP8 by default; `format` setting); an indicator in the top bar with
   elapsed time and a stop button; the file goes to `~/Videos/Screencasts`; a notification offers «Відкрити».

### Keyboard, focus, animation

The toolbar is fully keyboard operable (`Tab`, arrows, `Enter`, number keys 1..4 for modes); focus returns to the
previously focused window after capture. Dim fade <= 100 ms; the thumbnail slides in <= 150 ms; both instant when
`enable-animations` is off. Nothing animates inside the captured image: the dim layer is removed before the
pixels are read.

### Empty and error states

Capture fails (compositor refused): notification «Не вдалося зробити знімок: <reason>», nothing saved. Disk
full or folder unwritable: the image stays on the clipboard and the error says so. Recording fails to start:
the indicator never appears; a notification explains.

## Settings (proposed, `io.github.OlehDzhumyk.Desk.Capture`)

| Key | Type | Default |
|---|---|---|
| `enabled` | bool | true |
| `save-folder` | string | `~/Pictures/Screenshots` |
| `copy-to-clipboard` | bool | true |
| `show-preview` | bool | true (macos/windows11) |
| `delay` | choice `0,3,10` | 0 |
| `include-pointer` | bool | false |
| `record-format` | choice `webm,mp4` | `webm` |
| `record-audio` | choice `none,system,mic,both` | `none` |
| `annotate-after-capture` | bool | false |
| shortcuts | as | per theme, overridable |

## Privacy & security

Files only in the user's folders, mode 0600 for the editor's temporary files in `$XDG_RUNTIME_DIR`, deleted on
close. No upload, no "share" to online services. **Redact is real**: the covered pixels are replaced in the saved
file (the original is not embedded; undo history is not saved). PNG metadata contains no username, host name or
path. The lock screen and password prompts are never captured by our tool (GNOME blocks it). Screen-capture
permission for sandboxed apps stays with the portal, not with us. Recording audio requires an explicit setting.

## Edge cases

* Fractional scaling and mixed-DPI monitors: the saved image is at physical pixel size of the captured region.
* Region spanning monitors: stitched into one image.
* Protected (DRM) video surfaces appear black; documented, not worked around.
* Capture while another modal grab is active (a menu): the menu closes first; menus can be captured only with
  the delay timer.
* Very large capture (> 100 MP): refuse with a message instead of exhausting memory.
* Clipboard copy of an image uses the same path as clipboard history (it is then recorded there unless private
  mode); a marker prevents duplicate rows.
* Name collision in the same second: a numeric suffix.

## Accessibility

Capture is possible without a pointer: `Tab` to Screen mode and `Enter`, or arrow keys to move/resize the region
(`Shift` = 10 px). The editor tools have names and tooltips with shortcuts; colour choices are named; the
notification text is the screen-reader path.

## Performance

Toolbar visible <= 16 ms after the shortcut; capture-to-file <= 150 ms for a full 1080p screen; deskd holds no
image between captures (<= 20 MB RSS unchanged); the editor is a separate short-lived process.

## Acceptance criteria

* AC-SHOT-01: Given `windows11`, When `Shift+Super+S` is pressed and a region dragged and confirmed, Then a PNG of
  exactly that region exists in the save folder and the clipboard holds the same image.
* AC-SHOT-02: Given the toolbar is open, When `Esc` is pressed, Then nothing is saved and the dim layer is gone
  within 100 ms.
* AC-SHOT-03: Given `delay=3`, When capture is confirmed, Then a countdown shows and the image is taken after 3 s.
* AC-SHOT-04: Given `macos`, When a capture finishes, Then a thumbnail appears for ~6 s and activating it opens
  the editor.
* AC-SHOT-05: Given the editor with a Redact rectangle over text, When saved, Then the saved file's pixels in that
  rectangle no longer contain the original (pixel diff) and the file has no layered data.
* AC-SHOT-06: Given `copy-to-clipboard=false`, Then the clipboard is unchanged by a capture.
* AC-SHOT-07: Given recording started, When stop is pressed, Then a playable video exists and the indicator is
  gone.
* AC-SHOT-08: Given the shell is locked, Then the capture shortcuts do nothing.
* AC-SHOT-09: Given `enable-animations=false`, Then no fade or slide runs.
* AC-SHOT-10: Given the toolbar, Then every mode and the timer are reachable with the keyboard alone.

## Open questions

1. One binary for the editor (a mode of `themes`) or a separate `desk-shot`? (memory cost vs. packaging)
2. Use GNOME's screenshot UI as the toolbar in all skins and only theme it, instead of drawing our own?
3. MP4/H.264 availability on Silverblue (codec licensing) vs. WebM only.
4. Scrolling capture (Windows 11 has none; skip?).
