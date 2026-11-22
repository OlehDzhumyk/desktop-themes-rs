# Theme from wallpaper

## Summary

Pick a picture and get a complete, accessible theme from it: an accent and tonal palette, terminal colours, light and dark variants,
the picture as wallpaper. A second, live mode lets the accent follow the current wallpaper automatically.

## Status

* `themes tokens from-image` (creating a theme from an image): **Implemented in the stage 1.5 design-token work** (PR in review; the
  command is not on `main` yet): treat as **Partial** until merged and until the hub has the button.
* Live "accent follows wallpaper": **Planned** (stage 2).

Origin badge: *From Windows, adapted for macOS style* (Windows 11 offers an automatic accent from the wallpaper; macOS tints windows by the
wallpaper and has a "multicolour" accent but does not derive a full theme). Algorithm: original (k-means in OKLab, see
design-tokens (`docs/design-tokens.md`, lands with its stage 1.5 pull request)).

## Reference behaviour

* **Windows 11:** Settings > Personalisation > Colours > Accent colour: **Automatic** picks an accent from the wallpaper; the Start menu,
  taskbar and title bars may use it; light and dark modes are separate.
* **macOS:** accent colour is a fixed list (or Multicolour); "Allow wallpaper tinting in windows" subtly tints window backgrounds
  from the wallpaper.
* **GNOME 47+:** `org.gnome.desktop.interface accent-color` with ten named colours (blue, teal, green, yellow, orange, red, pink, purple, slate; plus
  `default`), no automatic derivation.

## Our behaviour

### One-shot: create a theme (`themes tokens from-image`)

```
themes tokens from-image PATH --name ID [--title NAME] [--no-wallpaper] [--out DIR] [--force]
```

1. Decode the image with gdk-pixbuf (any installed loader) at most 256 px wide; sample up to 16 384 pixels.
2. Cluster in OKLab (k-means, k = 8, deterministic farthest-point start). The **seed** is the cluster with the best population x chroma score that
   is neither near-black nor near-white; a grey image gets a muted accent.
3. Build a tonal palette on the seed hue (fixed OKLCH lightness steps, low-chroma neutrals, status colours on fixed hues), pushed until
   **every text pair passes WCAG AA**, for light and dark; the terminal palette uses the six ANSI hues nudged 15% toward the seed.
4. Write `themes/<ID>/tokens.toml` (validated before it is written) and `theme.conf` in the theme root (or `--out DIR`), with the
   image as the wallpaper unless `--no-wallpaper`. Refuse to overwrite an existing theme without `--force`. The result is an ordinary
   token theme: editable, inheritable, deletable, listed by `themes list`, applied by `themes apply ID`.
5. Same image gives the same theme (deterministic). Time budget: token generation <= 50 ms; the whole command <= 300 ms for a 4K image.

### In the hub

Page «Вигляд» (see [settings-hub](settings-hub.md)): the button «Створити тему із шпалер…» opens a file chooser (portal), shows the
**seed colour and the light/dark preview** before saving, asks for a name, then offers «Застосувати». The `default` GNOME accent is also
set to the nearest named accent on apply, so GNOME's own widgets agree.

### Live mode: accent follows wallpaper (planned)

Setting `accent-from-wallpaper` (off by default). When on:

* deskd watches `org.gnome.desktop.background picture-uri`/`picture-uri-dark` (GSettings signal; no polling), debounces 500 ms, computes the
  seed of the **active variant** (dark or light per `color-scheme`), and updates only the accent-related token outputs: it regenerates the active
  theme's overlay (a generated, non-persistent override layered above the theme; the theme's own files are never edited) and sets
  `accent-color` to the nearest named accent.
* A theme with a **hand-made** stylesheet (`macos`, `windows11`: WhiteSur/Fluent) only gets the GNOME `accent-color` change (the hand-made
  CSS wins, per the token rules); token themes get the full palette.
* The generated overlay lives in `~/.cache/desktop-themes/` and is deleted when the setting is turned off.

### Skins

`macos`: an "Appearance" row with a coloured dot grid including a rainbow "From wallpaper" dot (as Multicolour). `windows11`: an
"Accent colour" drop-down with **Automatic** and Manual. `default`: a switch «Акцент за шпалерами». The preview in the create flow uses
the skin's panel style.

### Keyboard, focus, animation

File chooser and dialogs are standard (keyboard accessible); the preview swatches are focusable with names ("Seed #A1B2C3"). The accent
change is instant for apps; no crossfade of ours; reduced motion has nothing to disable.

### Empty and error states

Unreadable or unsupported image: «Не вдалося прочитати зображення». Image too small (< 16x16) or fully transparent: refused. Almost
monochrome image: result with the muted accent and a note «Зображення майже без кольору». Palette cannot meet AA (should not happen): the
command fails with the pair that failed.

## Settings

CLI flags above. Proposed `io.github.OlehDzhumyk.Desk.Appearance`: `accent-from-wallpaper` (bool, false), `accent-source`
(choice `active,light,dark`, `active`).

## Privacy & security

Images are read locally, never copied elsewhere except as the wallpaper path that the user chose (a copy is **not** made unless the image is outside
the user's home, in which case the file chooser's portal path is copied into `~/.local/share/backgrounds/`). No network, no thumbnails sent.
Decoding untrusted images uses gdk-pixbuf in-process in the CLI/window: a malformed file may crash the window; it must not crash deskd (decode in
the CLI child process; deskd spawns `themes tokens from-image`).

## Edge cases

* Animated/multi-frame images (GIF, WebP): first frame. SVG: rasterised at 256 px. JPEG XL: only if the loader is installed.
* Wallpapers that differ in light and dark variants: the active variant seeds the live mode.
* A theme id that exists: refused without `--force`; with `--force`, the old directory goes to `trash/` first.
* Slideshow wallpapers (XML): the first image.
* Very different images in quick succession (slideshow): debounced; at most one regeneration per 500 ms.
* Colour-blind friendliness: the palette guarantees contrast, not hue separation; status colours keep icon/text cues.

## Accessibility

Every generated theme passes WCAG AA for text and 3:1 for non-text pairs by construction (this is a hard requirement of the algorithm, tested);
the live mode never lowers contrast below AA; the preview shows contrast numbers; reduced motion and text scaling are untouched.

## Performance

Token generation <= 50 ms; seed extraction <= 100 ms for a 4K image (subsampled); live mode costs zero while the wallpaper is unchanged (a GSettings
signal, no timers except the 500 ms one-shot debounce).

## Acceptance criteria

* AC-WALL-01: Given an image, When `themes tokens from-image img.png --name test` runs twice, Then both `tokens.toml` files are byte-identical.
* AC-WALL-02: Given any generated `tokens.toml`, When `themes tokens check test` runs, Then it exits 0 and every text pair has contrast >= 4.5:1.
* AC-WALL-03: Given an existing theme `test`, When the command runs without `--force`, Then it exits non-zero and changes nothing.
* AC-WALL-04: Given a grey image, Then the seed has non-zero chroma (a muted accent) and the note is shown in the hub.
* AC-WALL-05: Given `--no-wallpaper`, Then `tokens.toml` has no wallpaper image.
* AC-WALL-06: Given `accent-from-wallpaper=true` and the wallpaper changed, Then within 1.5 s `accent-color` is the named accent nearest the new seed.
* AC-WALL-07: Given the live mode and the `macos` theme, Then the hand-made WhiteSur stylesheet is unchanged and only `accent-color` changes.
* AC-WALL-08: Given the setting is turned off, Then the generated overlay in the cache is removed and the accent returns to the theme's.
* AC-WALL-09: Given an unreadable file, Then the command exits 1 with a clear message and no theme directory is left behind.
* AC-WALL-10: Given no network, Then everything works.

## Open questions

1. Should the live mode also recolour hand-made themes' accents inside WhiteSur/Fluent (they are CSS, not tokens)?
2. Offer 2-3 alternative seeds from the image to choose from (the Windows 11 and Android "palette choices" idea)?
3. Slideshow wallpapers: follow each slide, or only the first?
