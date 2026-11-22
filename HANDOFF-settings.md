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
* deskd: debounce `save()` (perf agent: sync_all+rename per push, p95 24 ms). Tried: a 1.5 s
  `save_soon` for Push/Activate works, but glib 0.22 here has no `unix_signal_add_local`, so a
  SIGTERM at logout could lose the last 1.5 s; reverted. Needs a SIGTERM hook (e.g. `signal-hook`
  crate or zbus-side flush) before debouncing.
* Keyboard-focus order review in the GUI; large-text check (text-scaling-factor 1.5 screenshots).

## Gaps reported by the spec agent (not done yet)
1. windows11 flyout: not reachable by Tab, items need accessible names. Partly done in this branch
   (cards have LIST_ITEM role + names, Tab/Shift+Tab call `navigate_focus`), but cards are
   `can_focus: false`, so Tab only reaches buttons: make cards focusable and sync focus ↔ selection.
2. bridge stylesheet.css: macos/windows11 skins are dark-only with hard-coded accent #60cdff /
   #2f6fe4. Follow light/dark (St.Settings color_scheme → `.desk-light` class) and the accent
   (St.Settings accent_color → inline style or class per accent); coordinate with the tokens agent
   (skin colours may come from design tokens).
3. Pin icon identical for pinned/unpinned (only `.checked` colour differs): use different icons
   (e.g. view-pin-symbolic vs view-pin-outline / a filled variant) + the accessible names already
   say «Закріпити»/«Відкріпити».
4. DONE (4e06c16): WinFlyout keeps the expanded card by item id.
5. Lock-screen style strings from settings are injected unvalidated
   (extensions/lockscreen-layout): move those keys into the registry with validation (colour regex,
   font name charset, size ranges via `Kind::UInt` ranges) and validate again in the extension.
6. Decision needed: per-feature shortcut overrides in the registry (user value) with theme-provided
   defaults (theme writes a `*-default` key or the registry reads the theme's desk.dconf). Today
   `toggle-shortcut` is written by themes and shown read-only.

## Cross-agent
* The tokens crate (theme-tokens, other branch) has English CLI/validation messages: once merged,
  wrap them in `desk_settings::tr!` and they are extracted by `cargo xtask pot` (it scans crates/**).
* Verified: this worktree has no theme-tokens files or Cargo.toml entry (shared-mirror mix-up);
  my mirror now lives in a uniquely named scratch dir.

## Next commands
```sh
cd ~/Developer/dtr-settings
toolbox run -c themes-rs-build sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo fmt && cargo test && cargo xtask check'
toolbox run -c themes-rs-build sh -c 'cd bridge && npm run typecheck && npm run lint && npm run build'
toolbox run -c themes-rs-build sh -c 'export PATH=$HOME/.cargo/bin:$PATH; cargo xtask pot'   # after string changes, then translate po/uk.po
target/shots.sh gui; target/shots.sh macos; target/shots.sh atspi uk
```
