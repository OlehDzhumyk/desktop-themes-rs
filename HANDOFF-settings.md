# Handoff: stage 1.5 "settings + i18n" (branch stage1.5-settings)

## Done
* **Settings registry** `crates/desk-settings` (`registry.rs` = the one source; `model.rs` types;
  `generate.rs` generators; `accessors.rs` generated typed GSettings accessors, feature `gio`).
  `cargo xtask gen` writes `data/io.github.OlehDzhumyk.Desk.gschema.xml`, `accessors.rs`,
  `docs/settings.md`; `cargo xtask check` / test `generated_files_up_to_date` catch drift.
  Test `stage1_keys_unchanged` pins stage-1 keys/types/defaults/paths (dconf values keep working).
* New key `io.github.OlehDzhumyk.Desk language` (system|uk|en).
* Window «Функції» page and its subpages (clipboard, general, credits) are generated from
  `desk_settings::PAGES` (`crates/themes/src/gui/features.rs`); `themes gui --page <id>` accepts any
  registry page id. Language change shows a toast with «Перезапустити» (`app.restart`, re-exec).
* deskd reads settings through `ClipboardSettings` accessors.
* **i18n**: gettext-rs 0.8 (gettext-system), one domain `desktop-themes-rs`; English source
  strings, `po/uk.po` carries the former Ukrainian texts (337 messages, all translated);
  `tr!`/`trn!` macros with `{named}` placeholders; `n_()` marks statics.
  `desk_settings::i18n::init` re-execs once with LANGUAGE=uk|en when the setting (or
  `DESKTOP_THEMES_LANGUAGE`) asks; `restore_child_env` gives children the user's LANGUAGE back.
  CLI help translated by walking the clap Command tree (`translate_help`). GUI, CLI, theme
  D-Bus service, themes-core messages, deskd --help and image labels converted.
* `cargo xtask pot` (xgettext Rust/TypeScript/desktop/metainfo → po/desktop-themes-rs.pot, msgmerge).
* Bridge: `bridge/src/i18n.ts` (`_`, `fmt`): 'system' → Extension.initTranslations/gettext (.mo in
  extension `locale/`), 'uk' → `locale/uk.json` built by assemble.mjs from po/uk.po, 'en' → source.
  UI rebuilds on `changed::language`. metadata.json has `gettext-domain` (name/description now English).
* `.desktop` → `data/*.desktop.in`, new `data/*.metainfo.xml.in`; `data/install.sh` compiles .mo into
  ~/.local/share/locale, merges translations into .desktop/metainfo, and on an upgrade from a
  pre-language install sets `language='uk'` once (keeps Ukrainian on the en_GB session).
  `gettext` added to the toolbox package list.
* **a11y**: `gui/a11y.rs`; labels for icon buttons, theme cards (FlowBoxChild label), card menu,
  credits link buttons, snapshot entry; high-contrast CSS (`prefers-contrast: more`).
  Bridge: Atk roles (dialog/list/list item/page tab), accessible names for rows (text + pinned),
  SELECTED state follows the selection, Home/End/Tab/Shift+Tab, `:focus` styles, fade only when
  enable-animations && !reduced-motion, `.desk-hc` outlines when high contrast, sizes in em.
* System look: default skin uses the shell theme's `popup-menu-content`/`popup-menu-item` (no own
  colours); macos/windows11 keep skin palettes. GUI badges use accent colours.
* Tests: `crates/desk-settings/tests/i18n_lint.rs` (no Cyrillic literals in code, no raw literals in
  UI setters, every id translated, placeholders kept). `cargo test` green; bridge typecheck/lint/build green.
* Screenshots (local only): ~/.local/share/desktop-themes/screenshots/stage1.5/ (gui-*-uk/en,
  bridge-windows11/fedora-uk/en, cli-help-*.txt). Local scripts: target/shots.sh, target/atspi_check.py.

## In progress / known issues
* AT-SPI check (`target/shots.sh atspi uk`): themes/clipboard/features pages 0 unnamed; «general»
  page: 3 unnamed `list item` (ComboRow popover items, probably GTK/libadwaita internals of the
  drop-down list) — investigate (maybe set a factory with labels) or accept.
* bridge-macos screenshots failed (headless shell not ready in time); rerun `target/shots.sh macos`.
* clap built-in strings ("Usage:", "Print help") stay English (as before).
* Bridge strings «Робочий стіл» kept as msgid "Desktop" for exact Ukrainian; the window itself is
  «Теми» — naming inconsistency predates this branch.
* `cargo fmt --check` not run on the new code yet.

## Remaining from the task stack
* docs/i18n.md (source language, domain, language setting, how to add a language) — referenced
  from docs/settings.md, not written yet; README note "no i18n yet" to update; docs/architecture.md
  mention of desk-settings/xtask (minimal additive edit).
* deskd: debounce `save()` (perf agent: sync_all+rename per push, p95 24 ms).
* Keyboard-focus order review in the GUI; large-text check (text-scaling-factor 1.5 screenshots).

## Next commands
```sh
cd ~/Developer/dtr-settings
toolbox run -c themes-rs-build sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo fmt && cargo test && cargo xtask check'
toolbox run -c themes-rs-build sh -c 'cd bridge && npm run typecheck && npm run lint && npm run build'
toolbox run -c themes-rs-build sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo xtask pot'   # after string changes, then translate po/uk.po
target/shots.sh gui; target/shots.sh macos; target/shots.sh atspi uk
```
