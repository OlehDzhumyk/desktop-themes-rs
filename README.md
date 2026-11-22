# desktop-themes-rs

A desktop theme manager for **GNOME** (developed on Fedora Silverblue, GNOME Shell 51). It switches
the whole look of the desktop between **Fedora** (stock), **macOS**, **macOS Liquid Glass**
(experimental: it crashed GNOME Shell 51.1 on real hardware) and **Windows 11**, and lets you **mix modules**: the macOS look with Windows 11 keyboard behaviour, for
example. Everything is done at user level: no sudo, no rpm-ostree layering.

* `themes`: CLI, GTK 4 / libadwaita window, and a D-Bus service (Rust).
* `deskd`: a session daemon for desktop features both Windows 11 and macOS have, starting with
  **clipboard history** (Win+V flyout in the Windows 11 theme, a Spotlight-like panel on
  Shift+Super+V in the macOS theme, a menu in Fedora; pins, search, private mode, password-manager
  aware). See [docs/features.md](docs/features.md).
* One GNOME Shell extension, `desk-bridge@desktop-themes-rs` (TypeScript): the quick-settings
  "Theme" toggle and the shell-side UI of deskd's features, skinned per theme.
* Four modules per theme, each switchable on its own:

| Module | id | What it contains |
|---|---|---|
| Look | `look` | GTK and shell themes, icons, cursors, fonts, wallpapers, extensions, libadwaita CSS, Flatpak overrides |
| Behavior | `behavior` | shortcuts, Alt+Tab, touchpad, hot corners, font rendering, window placement |
| Sounds | `sounds` | sound theme and system event sounds |
| Apps | `apps` | per-app settings (e.g. Ptyxis palette) and optional apply/revert hooks |

* Live switching: extensions are enabled and disabled in the running shell, no logout needed
  (a logout is only required for extensions installed after login).
* Safe by design: a baseline snapshot of your own dconf, automatic rollback of extensions that
  error out, `themes doctor`, and `themes reset`.

> The user interface is translated with gettext: English source strings, Ukrainian in `po/uk.po`
> (language setting: system / українська / English, see [docs/i18n.md](docs/i18n.md)). In-code
> comments are mostly Ukrainian. The documentation here is English, with a Ukrainian section below.

## Quick start

On a fresh Fedora Silverblue with a GNOME session:

```sh
git clone https://github.com/OlehDzhumyk/desktop-themes-rs
cd desktop-themes-rs
scripts/bootstrap.sh --dry-run      # read what it would do
scripts/bootstrap.sh                # then do it
```

`bootstrap.sh` creates a build toolbox, builds the app, installs the themes, fetches the third-party
assets from upstream at pinned revisions, generates the sounds and wallpapers, installs the
extensions, installs the binary, `.desktop` file, D-Bus service and systemd unit, takes the baseline
snapshot and runs `themes list`. Log out and in once afterwards so the shell loads the new extensions.

Each step is also available separately: `scripts/install-themes.sh`, `install-assets.sh`,
`generate-assets.sh`, `install-extensions.sh`, `data/install.sh`. All accept `--dry-run`.

```sh
themes list                          # themes and their modules
themes current                       # which theme each module comes from
themes apply macos                   # whole theme
themes apply macos --only look,sounds
themes apply windows11 --module behavior
themes apply macos --dry-run         # show what would change
themes apply macos-glass --yes       # experimental themes ask y/N (or need --yes in scripts)
themes reset                         # back to the baseline
themes snapshot my-look --name "My look"   # save the current look as a theme
themes doctor --fix
themes gui
```

Requirements: GNOME Shell 51 (the extensions and shell themes are pinned for it; `--latest` in
`install-extensions.sh` tries other versions), toolbox, flatpak (optional), network access for the
upstream downloads.

## Repository layout

```
Cargo.toml, crates/     Rust workspace: themes-core (library), themes (CLI/GUI/service),
                        desk-proto (shared D-Bus definitions), deskd (features daemon)
bridge/                 the GNOME Shell bridge extension (TypeScript, npm)
themes/                 the themes: only contract files (dconf fragments, theme.conf, extensions.txt, ...)
extensions/             other GNOME Shell extensions of this project (lockscreen-layout)
patches/                unified diffs for a third-party extension (no vendored copy)
tools/                  sound synthesizer, wallpaper generator, headless GNOME Shell test tools
scripts/                bootstrap and installers (assets, extensions, generated files)
data/                   .desktop file, D-Bus service, systemd unit, binary install script
docs/                   architecture, theme contract, module system, theme notes, Ukrainian user guide
```

Third-party themes, icons, cursors, fonts, wallpapers and extensions are **not** stored here; the
scripts download them from upstream. See [CREDITS.md](CREDITS.md) for every project used and its
licence. The generated libadwaita CSS (derived from WhiteSur and Fluent) is likewise created on the
user's machine, not committed.

## Documentation

* [docs/architecture.md](docs/architecture.md): components, state, how applying works
* [docs/theme-contract.md](docs/theme-contract.md): files a theme may contain, path placeholders
* [docs/module-system.md](docs/module-system.md): modules, managed keys, mixing rules
* [docs/themes.md](docs/themes.md): notes on the bundled themes and their extensions
* [docs/design-tokens.md](docs/design-tokens.md): `tokens.toml`, generators, theme from wallpaper
* [docs/features.md](docs/features.md): desktop features (clipboard history), skins, privacy
* [docs/settings.md](docs/settings.md): every setting (generated from the settings registry)
* [docs/i18n.md](docs/i18n.md): translations and the language setting
* [docs/README.uk.md](docs/README.uk.md): full user guide in Ukrainian

## Development

```sh
toolbox run -c desktop-themes-build cargo test                 # all crates
toolbox run -c desktop-themes-build cargo build --release
toolbox run -c desktop-themes-build sh -c 'cd bridge && npm ci && npm run typecheck && npm run lint && npm run build'
tools/headless/clipboard-test.sh windows11 Super+v /tmp/shots  # isolated headless end-to-end test
tools/integration-test.sh target/release/themes   # CAUTION: writes your real dconf; run in a VM/spare session
```

`tools/headless/` starts a fully isolated headless GNOME Shell (own D-Bus bus, Wayland display,
runtime dir, dconf database and data-dir overlay) for screenshots and extension tests without
touching the real session; see `tools/headless/env.sh`.

Coming from an older private build with other app/extension ids? Run
`scripts/migrate-from-legacy.sh --dry-run`, then without `--dry-run`.

## Licence

GPL-3.0-or-later, see [LICENSE](LICENSE). Generated sounds and wallpapers are original works
released under CC0. Third-party material keeps its own licences (CREDITS.md). No Apple or Microsoft
assets (logos, fonts, wallpapers, sounds) are used; the macOS and Windows 11 themes are original
work inspired by those designs.

---

## Українською

**desktop-themes-rs** — менеджер тем робочого столу GNOME (розроблено на Fedora Silverblue, GNOME
Shell 51). Перемикає вигляд між **Fedora**, **macOS**, **macOS Liquid Glass** і **Windows 11**, а також
дозволяє **змішувати модулі** (вигляд, поведінка, звуки, програми): наприклад, вигляд macOS із
поведінкою Windows 11. Усе працює на рівні користувача, без sudo й без rpm-ostree.

Швидкий старт на свіжій Fedora Silverblue:

```sh
git clone https://github.com/OlehDzhumyk/desktop-themes-rs
cd desktop-themes-rs
scripts/bootstrap.sh --dry-run   # переглянути кроки
scripts/bootstrap.sh             # виконати
```

Скрипт збирає програму в toolbox, встановлює теми, завантажує сторонні ресурси (WhiteSur, Fluent,
значки, курсори, шрифти, шпалери) з оригінальних сховищ за зафіксованими комітами, генерує звуки й
шпалери, встановлює розширення, програму, службу D-Bus і юніт systemd, робить знімок baseline вашого
dconf. Після цього один раз вийдіть із сесії й увійдіть знову.

Повний посібник користувача (команди, як додати тему, що робити, якщо щось зламалося):
[docs/README.uk.md](docs/README.uk.md). Ліцензія: GPL-3.0-or-later; сторонні проєкти й їхні ліцензії
перелічено в [CREDITS.md](CREDITS.md). Інтерфейс перекладено (українська / англійська); мову можна вибрати у вікні: Функції → Загальні.
