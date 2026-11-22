# HANDOFF (stage1.5-docs)

Work in progress on `docs/spec/**`. Remove this file when the spec tree is complete.

## Done (written, committed)
- docs/spec/README.md (purpose, legend, stages, feature map), principles.md, glossary.md
- features/: themes, doctor, quick-settings-theme, clipboard, behavior, sounds, lock-screen, search, liquid-glass
- Planned specs done: mixer, emoji
- README.md: one link line to docs/spec/

## Remaining (Planned specs, use the same section template; see features/mixer.md)
screenshots, ocr, color-picker, quick-look, window-layouts, shortcut-guide, focus, auto-dark,
theme-from-wallpaper (align with tokens agent: `themes tokens from-image`, crate theme-tokens),
phone (GSConnect), settings-hub (align with settings agent: crates/desk-settings registry,
docs/settings.md, pages `clipboard`, `general`, `credits`, key `language`).
The feature map in docs/spec/README.md already links all of them (links dangle until written).
Shortcut proposals for them are in features/behavior.md "Planned shortcut registry".
Then: check docs/README.uk.md/other index for a link, final privacy grep (no username/hostname/private theme).

## Gaps found so far (code vs intended behaviour)
- G-THEMES-1 snapshot omits Desk keys (skin/shortcut): crates/themes-core/src/snapshot.rs SNAPSHOT_PREFIXES
- G-THEMES-2/G-QS-1/G-GLASS-1..3 no EXPERIMENTAL marker or confirmation for macos-glass (cli.rs, gui/card.rs, bridge/src/quickSettings.ts)
- G-THEMES-3 stale comment in themes/macos/spotlight.dconf
- G-THEMES-4 apply exits 0 after auto-rollback
- G-THEMES-5/G-DOCTOR-3 read-only `themes doctor` writes state.json (doctor.rs)
- G-DOCTOR-1 doctor does not check deskd or GSettings schema
- G-DOCTOR-2 shell-crash warning keeps exit code 3 for 48 h
- G-DOCTOR-4 journal/coredumpctl parsing relies on English strings
- G-CLIP-1 source app = focused window, not selection owner (watcher.ts)
- G-CLIP-2 windows flyout not keyboard/AT accessible (Tab, accessible names) (winFlyout.ts, panel.ts)
- G-CLIP-3 stylesheet dark-only, hard-coded accent #60cdff
- G-CLIP-4 private Main.inputMethod._cursorRect (panel.ts)
- G-CLIP-5 persisted Entry.hash uses std DefaultHasher (deskd clipboard.rs)
- G-CLIP-6 same pin icon both states; _expanded index after delete
- G-BEHAV-2 macos look + windows11 behavior: two owners of Super+Space
- G-SND-1..4 theme restores event-sounds; unthemable events untested; no sounds page
- G-LOCK-2 unvalidated CSS strings in lockscreen-layout style; G-LOCK-3 no contrast guard
- G-SRCH-1..4 third-party launcher dependency, Super+S differs from Windows

## Open questions so far
Stage ordering 2-5 (proposal); experimental theme policy; engine location for mixer (deskd vs Gvc);
build own search engine vs upstream Spotlight; per-feature shortcut overrides; licences of "Based on" projects to verify.
