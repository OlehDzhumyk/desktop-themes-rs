# Design tokens (`tokens.toml`)

A theme can describe its look as **design tokens** instead of (or in addition to) hand-made CSS and
dconf files. When the theme is applied, the manager generates from them:

| Output | Where | Consumer |
|---|---|---|
| libadwaita 1.10 stylesheet | `~/.cache/desktop-themes/<id>/gtk4/gtk.css`, **copied** into `~/.config/gtk-4.0/` | GTK 4 / libadwaita apps |
| GNOME Shell layer | `.../<id>/shell/gnome-shell/gnome-shell.css`, linked as `~/.local/share/themes/desktop-themes-<id>` | user-theme extension |
| Ptyxis palette | `.../<id>/ptyxis/desktop-themes-<id>.palette`, linked into `~/.local/share/org.gnome.Ptyxis/palettes/` | Ptyxis |
| wallpaper (optional) | `.../<id>/wallpaper/{light,dark}.svg` | `org.gnome.desktop.background` |
| dconf keys | merged into the look / sounds / apps modules | the usual apply pipeline |
| labwc `themerc` (on request) | `.../<id>/labwc/openbox-3/themerc` | labwc window decorations (desktop shell session) |
| desk-shell CSS (on request) | `.../<id>/desk-shell/desk-shell.css` | desk-shell (desktop shell session) |

So a theme can be just `theme.conf` + `tokens.toml` (+ `extensions.txt`, `behavior.dconf` if it
wants them): see `themes/lavender`. Nothing generated is ever written into the repository or the
theme directory. The code lives in `crates/theme-tokens` (model, validation, generators, no GNOME
dependencies) and `crates/themes-core/src/tokens.rs` (integration).

## Rules

* **Hand-made wins.** A key in the theme's `*.dconf` files overrides the same key derived from
  tokens. A `gtk4/` directory anywhere in the `INHERITS` chain switches the generated stylesheet
  off; a `user-theme name` in the theme's dconf files switches the generated shell layer off.
  This is how `macos` and `windows11` keep WhiteSur and Fluent as their base.
* **Inheritance.** `tokens.toml` files along the `INHERITS` chain are deep-merged: a child's key
  replaces the parent's, tables merge, arrays (palettes) are replaced. A child without a
  `tokens.toml` inherits its parent's tokens.
* **Only what is set is emitted.** Every key is optional. Unset colours keep libadwaita's values,
  unset radii keep its metrics; the generated CSS redefines nothing else.
* **Modules.** The file belongs to the look module; derived keys are routed to their modules
  (`[sounds]` → sounds, `[terminal]` → apps), so mixing modules works as with dconf files.
* **Invalid tokens stop the apply** before anything is changed, with every problem listed.
  `themes doctor` reports them too; other themes with invalid tokens contribute no derived keys.

## Schema (schema = 1)

```toml
schema = 1                 # optional; other values are rejected
name = "Lavender"          # palette name etc. (default: theme NAME)
color_scheme = "default"   # org.gnome.desktop.interface color-scheme: default | prefer-dark | prefer-light
```

### `[colors]`, `[colors.light]`, `[colors.dark]`

Keys in `[colors]` apply to both schemes unless a scheme table sets them. Colours are `#rgb`,
`#rgba`, `#rrggbb` or `#rrggbbaa`.

| Key | libadwaita variable |
|---|---|
| `accent` | `--accent-bg-color`; a GNOME accent **name** (`blue`, `teal`, `green`, `yellow`, `orange`, `red`, `pink`, `purple`, `slate`) only sets `accent-color` and emits no CSS |
| `accent_fg` | `--accent-fg-color` |
| `window_bg`, `window_fg`, `view_bg`, `view_fg` | `--window-*-color`, `--view-*-color` |
| `headerbar_bg`, `headerbar_fg`, `headerbar_border`, `headerbar_backdrop` | `--headerbar-*-color` |
| `sidebar_bg`, `sidebar_fg`, `sidebar_backdrop` | `--sidebar-*-color` |
| `card_bg`, `card_fg`, `popover_bg`, `popover_fg`, `dialog_bg`, `dialog_fg` | `--card-*`, `--popover-*`, `--dialog-*` |
| `destructive_bg/fg`, `success_bg/fg`, `warning_bg/fg`, `error_bg/fg` | `--destructive-*` etc. |
| `shade` | `--shade-color` (also the colour of generated shadows) |
| `border_opacity` | `--border-opacity` (0..1) |

The accent also sets `org.gnome.desktop.interface accent-color` to the nearest GNOME accent
(OKLCH hue; greys → `slate`), so the shell and apps the CSS does not reach follow it.

### Other tables

| Table | Keys | Used by |
|---|---|---|
| `[radii]` (px) | `window` (0..48), `button`, `card`, `popover`, `panel` (0..64) | GTK: `--window-radius`, buttons/entries (linked groups keep square inner corners; circular and pill buttons untouched), boxed lists/cards, popovers. Shell: panel buttons, menus, quick toggles (`button`), menus (`popover`), quick settings/OSD/switchers/dash (`panel`), notifications (`card`), dialogs (`window`) |
| `[fonts]` | `ui`, `document`, `mono`, `titlebar` (Pango "Family [Style] Size", size 4..72), `title_weight` (100..900) | `font-name`, `document-font-name`, `monospace-font-name`, `titlebar-font` (derived from `ui` + weight when absent); CSS weight of window titles |
| `[spacing]` (px) | `xs` <= `sm` <= `md` <= `lg` <= `xl` (0..96) | shell only (panel button padding, menu and quick-settings padding); GTK keeps libadwaita's metrics |
| `[shadows.card]`, `[shadows.popover]` | `x`, `y`, `blur`, `spread`, `opacity` | cards/boxed lists and popovers in GTK; notifications and menus in the shell |
| `[surface]` | `panel_opacity`, `popover_opacity` (0..1), `blur` (intent) | shell backgrounds; `blur` documents that the theme expects Blur my Shell (it never enables extensions) |
| `[motion]` | `fast_ms` <= `normal_ms` <= `slow_ms` (0..2000), `easing` (`linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `ease-out-quad`, `ease-out-cubic`, `ease-out-quint`, `ease-in-out-cubic`, `cubic-bezier(a, b, c, d)`) | transition duration/timing of buttons, rows, switches, checks, entries (GTK) and panel buttons, menu items, quick toggles (shell) |
| `[sounds]` | `theme` | `org.gnome.desktop.sound theme-name` |
| `[terminal]` | `name`, `font`, `builtin` **or** `[terminal.dark]`/`[terminal.light]` with `fg`, `bg`, `cursor`, `palette` (16 colours) | Ptyxis palette file + the default profile's `palette`, `use-system-font=false` + `font-name`; a missing scheme reuses the other one |
| `[shell]` | `generate` (default true when the table exists), `scheme` (`dark` default / `light`), `panel_bg`, `panel_fg` | turns the shell layer on; the user-theme extension is added to the theme's extensions automatically |
| `[gtk]` | `generate = false` | never generate the libadwaita stylesheet |
| `[wallpaper]` | `style = "gradient"` or `image = "~/path"` | generated SVG (window background shading into the accent) or the image; sets `picture-uri`, `picture-uri-dark`, `picture-options`, the screensaver picture |

## Validation

Errors (the theme cannot be applied): TOML syntax, unknown keys and wrong types (with file, line
and column), unsupported `schema`, out-of-range numbers, non-monotonic spacing or durations, bad
easing, malformed font descriptions, palettes without 16 colours, `builtin` together with own
colours, and **contrast** below WCAG 2.x AA:

* 4.5:1 for text on surfaces: window, view, headerbar, sidebar, card, popover, dialog, the shell
  panel, the terminal foreground;
* 3:1 for text on filled controls (accent, destructive, success, warning, error): bold button
  labels count as large text / UI components.

A pair is checked when the theme sets at least one side; the other side falls back to libadwaita's
value (translucent colours are composited on the window background). Stock values alone are never
reported. Warnings (shown by `themes tokens check` and `themes doctor`): accent-coloured text
(libadwaita derives it by clamping OKLab lightness) below 4.5:1 on the view background, terminal
colours below 3:1 on the terminal background (black/white excluded), a hard-to-see cursor.

## Generators

**libadwaita 1.10.** libadwaita (since 1.6) exposes its named colours as CSS custom properties on
`:root` and switches them with `@media (prefers-color-scheme: dark)`; this was checked in the 1.10
library's compiled stylesheet. The generated `gtk.css` (loaded by GTK at USER priority) redefines
the variables in `@media (prefers-color-scheme: light)` and `(…: dark)` blocks, plus the legacy
`@define-color` names for older custom CSS. Light values must not sit in a plain `:root`: at USER
priority they would also win in dark mode. One file serves both schemes; GTK 4 does not load a
`gtk-dark.css` from `~/.config/gtk-4.0`.

**GNOME Shell.** A small override layer: the generated stylesheet starts with
`@import url("resource:///org/gnome/shell/theme/gnome-shell-dark.css")` (or `-light`) and then
restyles a few surfaces (panel, menus, quick settings, OSD, switchers, dash, notifications,
dialogs) and, for an exact accent colour, checked toggles, default buttons, today in the
calendar, sliders and focus rings including the lock-screen password entry. Chosen over a full
recolour of the stylesheet extracted from `gnome-shell-theme.gresource` because nothing upstream is
copied or regex-patched: after a Shell upgrade every widget is still styled by that Shell's own
stylesheet and only the overridden properties come from the tokens. St has no CSS variables or
media queries, so the layer uses one scheme (dark by default, like the bundled shell themes).

**Ptyxis.** GKeyFile as read by `ptyxis-palette.c`: `[Palette] Name`, `[Light]`/`[Dark]` with
`Foreground`, `Background`, `Cursor`, `Color0`..`Color15`. Ptyxis loads user palettes from
`$XDG_DATA_HOME/org.gnome.Ptyxis/palettes/` and the palette id is the file name without
`.palette`: `desktop-themes-<id>`.

**labwc and desk-shell (on request).** These two outputs serve the desktop shell session, not
GNOME, so `generate::render` never writes them; a caller asks for one with
`generate::render_extra(tokens, target, ExtraTarget::LabwcTheme | ExtraTarget::DeskShellCss)`. Both
are always complete: an unset token keeps the consumer's default.

* `LabwcTheme`: an Openbox-style `themerc` (labwc-theme(5)) in one scheme, the shell scheme
  (`shell.scheme`, dark by default). Active titlebar = `headerbar_bg`/`headerbar_fg`, inactive =
  `headerbar_backdrop` (else `window_bg`) with the text at 60 %; the 1 px border is
  `headerbar_border` or the text colour at `border_opacity`; the close button hover is
  `destructive_bg`/`destructive_fg`; menus and the window switcher use `popover_bg`/`popover_fg`,
  the selected item a 10 % text tint, the switcher border and the snap overlay the accent (a GNOME
  accent name gives its colour). `spacing.sm` → `padding.width` and menu item `padding.y`,
  `spacing.md` → menu item `padding.x`, `spacing.xs` → titlebar padding and button spacing,
  `radii.button` → the button hover corner radius, `shadows.popover` → window shadows. Unset
  colours are libadwaita's; translucent colours are composited on the window background, so the
  decorations stay solid. `titlebar.height` is not written (deprecated in labwc 0.20); the window
  corner radius belongs to labwc's `rc.xml`.
* `DeskShellCss`: the `--ds-*` custom properties of desk-shell's stylesheet
  (`crates/desk-shell/src/style.rs`) for `.desk-shell.light` and `.desk-shell.dark`, the same
  values as `@define-color desk_shell_<scheme>_<name>`, `radii.button` → `--ds-radius`,
  `radii.popover` → `--ds-flyout-radius`, and the surface rules of the taskbar, Start menu and
  flyouts (background, border, hover, active, checked). `window_bg` × `surface.panel_opacity` is
  the taskbar, `popover_bg` × `surface.popover_opacity` the flyouts, `window_fg` the text and the
  tints of dim, hover, active and border (`border_opacity`) text; `shadows.popover` sets the
  flyout shadow. An exact accent sets `--ds-accent`/`--ds-on-accent`; a GNOME accent name keeps
  desk-shell's per-skin accent. Unset tokens keep desk-shell's Windows 11 values, and the palette
  rules skip `.hc`, so high contrast always wins.

## Cache and performance

Output for a theme goes to `~/.cache/desktop-themes/<id>/` (`$XDG_CACHE_HOME`,
`DESKTOP_THEMES_CACHE` overrides; with a custom `DESKTOP_THEMES_ROOT` it is `<root>/.cache`). The
`.stamp` holds an FNV-1a 64 key of the generator version, the target (theme, home, which hand-made
files exist) and the merged canonical tokens: unchanged tokens → nothing is rendered or written.
A new generation is written beside the old one and swapped in with a rename, so the links stay
valid. If the cache is cleaned, `themes doctor` reports it and `--fix` regenerates.

Measured on the development laptop (Lavender, the largest token file): parse + validate 0.12 ms,
render + write 0.16 ms, cache check 0.02 ms (release); the debug `themes tokens generate --repeat`
end to end, including reading the theme, ~1.5 ms. Budget: 50 ms per theme.

## Commands

```sh
themes tokens check [ID...]          # all themes with tokens, or the given ones; exit 1 on errors
themes tokens generate ID [--force] [--repeat N]   # fill the cache, print files and time
themes tokens keys ID                # derived dconf keys (dconf load format)
themes tokens from-image PATH --name ID [--title NAME] [--no-wallpaper] [--out DIR] [--force]
themes tokens seed PATH              # seed colour, nearest GNOME accent, light/dark preview (writes nothing)
themes tokens overlay SEED | --remove   # live wallpaper palette over the applied token theme (deskd runs it)
themes apply ID --dry-run            # also lists generated files and links
```

**Theme from wallpaper.** The image is decoded with gdk-pixbuf (any installed loader, JPEG XL
included) at most 256 px wide, up to 16 384 pixels are clustered with k-means (k = 8,
deterministic farthest-point start) in OKLab, and the seed is the cluster with the best
population × chroma score that is neither near-black nor near-white (grey images get a muted
accent). A tonal palette on the seed hue (fixed OKLCH lightness steps, low-chroma neutrals,
status colours on fixed hues) is pushed until every pair passes AA, and the terminal palette uses
the six ANSI hues nudged 15 % towards the seed. The result is a normal `tokens.toml` (validated
before it is written) in `themes/<ID>/` of the theme root, with the image as wallpaper. A slideshow
XML gives its first image; images under 16×16 px or without opaque pixels are refused; a
near-monochrome image (seed chroma < 0.04) is reported («The image has almost no colour») and its
GNOME accent is `slate`. `--force` over a theme in the themes root moves the old directory to
`trash/` first. `themes tokens seed` runs the same analysis and prints it; deskd's live "accent
from wallpaper" mode runs it as a child process ([feature spec](spec/features/theme-from-wallpaper.md)).

**Live wallpaper palette.** While "accent from wallpaper" is on, deskd runs `themes tokens overlay
SEED` after each new seed. When the applied look's `gtk.css` is a generated copy, the theme's token
chain is loaded with one more layer on top, the seed's tonal palette (`[colors.light]` and
`[colors.dark]`, backdrop colours included; validated like any theme), and the libadwaita
stylesheet rendered from it replaces the copy's content. The result lives in
`<cache>/wallpaper-overlay/<id>/gtk.css`; `state.json` records it (`wallpaper_overlay`) with the
theme's own generated file, which `--remove`, a new look, `themes reset` or `themes doctor --fix`
(with the mode off) write back. Hand-made stylesheets are never replaced
([ADR 0010](adr/0010-live-wallpaper-palette-overlay.md)).

## Visual checks

`tools/headless/tokens-screenshot.sh THEME OUTDIR` generates the theme into a private root and
takes light and dark screenshots of the isolated headless shell with a libadwaita demo window
(`adw-demo.py`), without touching the real session (shell theme linked into the private data dir,
`gtk.css` into the private config dir, keys loaded into the private dconf database).

## Limitations

* GTK 3 apps are not themed by tokens (Adwaita GTK 3 compiles its colours into literals).
* Flatpak apps see `~/.config/gtk-4.0` only with an override (`flatpak.conf`). That is why the
  generated `gtk.css` is copied there, not linked into `~/.cache`: the copy is recorded in
  `state.json` with its content hash, removed (and a displaced file of yours restored) when the look
  changes, rewritten on the next apply or by `themes doctor --fix` after a new cache generation,
  and never touched once you edit it.
* The shell layer has one scheme; spacing is shell-only; motion covers common widgets only; RTL
  linked groups keep libadwaita's own corners.
