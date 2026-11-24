#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Remove what scripts/bootstrap.sh, data/install.sh and scripts/migrate-from-legacy.sh installed.
# Everything is user level (nothing outside your home is touched). Order:
#   1. `themes reset`: your desktop goes back to the baseline snapshot taken before the first theme
#      (skipped with --no-reset, or when no baseline exists; if it fails the script stops)
#   2. stop and disable deskd and the doctor unit; stop a running theme service
#   3. take our extensions (and, unless --keep-extensions, the ones bootstrap fetched) out of
#      enabled/disabled-extensions
#   4. remove binaries (themes, theme, themes-gui, deskd, desk-shell, desk-win11-session), the
#      labwc session config, D-Bus activation files, .desktop file,
#      systemd units, the GSettings schema, extensions, third-party themes/icons/cursors/fonts/
#      wallpapers/sounds that bootstrap installed (unless --keep-assets) and the download cache
# KEPT unless --purge: the theme root (~/.local/share/desktop-themes: your themes and snapshots,
# the baseline, state, backups, trash). --purge also deletes it, deskd's clipboard history and the
# settings of the io.github.OlehDzhumyk schemas in dconf.
# Never removed: the build toolbox (--remove-toolbox does), Flatpak apps, Rust/Node toolchains.
#
# Usage: scripts/uninstall.sh [--dry-run] [--yes] [--purge] [--no-reset] [--keep-extensions]
#                             [--keep-assets] [--remove-toolbox]
#   --dry-run          print every action, change nothing (the safe way to see what would go)
#   --yes              do not ask for confirmation
#   --purge            also delete theme root, clipboard history and dconf settings
#   --no-reset         skip `themes reset` (e.g. no graphical session)
#   --keep-extensions  keep the third-party extensions bootstrap installed (dash-to-dock, ...)
#   --keep-assets      keep the third-party themes, icons, cursors, fonts, wallpapers, sounds
#   --remove-toolbox   also delete the build toolbox ($DT_TOOLBOX)
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

YES=0 PURGE=0 NO_RESET=0 KEEP_EXT=0 KEEP_ASSETS=0 RM_TOOLBOX=0
parse_common_flags "$@"
for a in "${DT_ARGS[@]+"${DT_ARGS[@]}"}"; do
  case "$a" in
    --yes|-y) YES=1 ;;
    --purge) PURGE=1 ;;
    --no-reset) NO_RESET=1 ;;
    --keep-extensions) KEEP_EXT=1 ;;
    --keep-assets) KEEP_ASSETS=1 ;;
    --remove-toolbox) RM_TOOLBOX=1 ;;
    -h|--help) sed -n '2,29p' "$0"; exit 0 ;;
    *) die "unknown option: $a (see --help)" ;;
  esac
done

[ "$(id -u)" -ne 0 ] || die "do not run as root: everything was installed for your user"

APP_ID=io.github.OlehDzhumyk.Themes
DESK_ID=io.github.OlehDzhumyk.Desk
BIN=$HOME/.local/bin
UNITS=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user
EXT_DIR=$DATA_HOME/gnome-shell/extensions
OWN_EXT=(desk-bridge@desktop-themes-rs lockscreen-layout@desktop-themes-rs themes-quick@desktop-themes-rs)
EGO_UUIDS=()
for e in "${EGO_EXTENSIONS[@]}"; do EGO_UUIDS+=("${e%% *}"); done
EGO_UUIDS+=("$LIQUID_GLASS_UUID")

# rm_path PATH...: remove files/directories that exist (reports each one)
rm_path() {
  local p
  for p in "$@"; do
    [ -e "$p" ] || [ -L "$p" ] || continue
    note "remove ${p/#$HOME/\~}"
    run rm -rf -- "$p"
  done
}

# ---- plan ---------------------------------------------------------------------------------------
say "uninstall plan (theme root: $DT_ROOT)"
note "1. themes reset (back to the baseline)$([ "$NO_RESET" = 1 ] && echo " [skipped: --no-reset]")"
note "2. stop/disable deskd.service, themes-doctor.service"
note "3. remove from enabled/disabled-extensions: ${OWN_EXT[*]}$([ "$KEEP_EXT" = 1 ] || echo " and the third-party extensions of bootstrap")"
note "4. delete binaries, D-Bus/systemd/desktop files, schema, extensions$([ "$KEEP_ASSETS" = 1 ] || echo ", third-party themes/icons/fonts/wallpapers/sounds"), cache"
if [ "$PURGE" = 1 ]; then
  note "5. PURGE: delete $DT_ROOT (your own themes and snapshots, baseline, state, backups), ~/.local/share/deskd (clipboard history) and the dconf settings of $DESK_ID"
else
  note "kept: $DT_ROOT (themes, baseline, state; use --purge to delete)"
fi
if [ "$DRY_RUN" != 1 ] && [ "$YES" != 1 ]; then
  [ -t 0 ] || die "not interactive: pass --yes to confirm, or --dry-run to preview"
  printf 'Continue? [y/N] '
  read -r ans
  case "$ans" in y|Y|yes|YES) ;; *) die "aborted, nothing changed" ;; esac
  if [ "$PURGE" = 1 ]; then
    printf 'PURGE deletes your own themes and the baseline permanently. Type "purge" to confirm: '
    read -r ans
    [ "$ans" = purge ] || die "aborted, nothing changed"
  fi
fi

# ---- 1. reset to the baseline -------------------------------------------------------------------
say "reset to the baseline"
if [ "$NO_RESET" = 1 ]; then
  note "skipped (--no-reset)"
elif [ ! -x "$BIN/themes" ]; then
  note "$BIN/themes not found, nothing to reset"
elif [ ! -e "$DT_ROOT/baseline" ] && ! ls -d "$DT_ROOT"/baseline-* >/dev/null 2>&1; then
  note "no baseline in $DT_ROOT, nothing to reset"
elif [ "$DRY_RUN" = 1 ]; then
  note "+ $BIN/themes reset"
elif ! "$BIN/themes" reset; then
  die "themes reset failed; nothing was removed. Fix it (themes doctor), or re-run with --no-reset to remove the files anyway"
fi

# ---- 2. services --------------------------------------------------------------------------------
say "services"
if have systemctl; then
  run systemctl --user disable --now deskd.service themes-doctor.service || true
else
  note "systemctl not found"
fi
# a deskd or theme service started by D-Bus activation outside systemd; exact names, own processes only
if have pkill; then
  run pkill -u "$(id -u)" -x deskd || true
  run pkill -u "$(id -u)" -f -- "$BIN/themes --dbus-service" || true
fi

# ---- 3. extension lists -------------------------------------------------------------------------
say "extension lists"
REMOVE_UUIDS=("${OWN_EXT[@]}")
[ "$KEEP_EXT" = 1 ] || REMOVE_UUIDS+=("${EGO_UUIDS[@]}")
fix_list() { # dconf-key
  local key=$1 cur new
  cur=$(dconf read "$key" 2>/dev/null || true)
  [ -n "$cur" ] || return 0
  new=$(python3 - "$cur" "${REMOVE_UUIDS[@]}" <<'PY'
import ast, re, sys
lst = ast.literal_eval(re.sub(r"^@as ", "", sys.argv[1]))
drop = set(sys.argv[2:])
out = [u for u in lst if u not in drop]
print("[" + ", ".join("'%s'" % u for u in out) + "]" if out else "@as []")
PY
)
  if [ "$new" != "$cur" ]; then
    note "$key: $new"
    run dconf write "$key" "$new"
  fi
}
if have dconf; then
  fix_list /org/gnome/shell/enabled-extensions
  fix_list /org/gnome/shell/disabled-extensions
else
  warn "dconf not found: enabled-extensions not edited"
fi
# let the shell unload them before their files disappear
[ "$DRY_RUN" = 1 ] || sleep 2

# ---- 4. files -----------------------------------------------------------------------------------
say "binaries and desktop integration"
rm_path "$BIN/themes" "$BIN/theme" "$BIN/themes-gui" "$BIN/deskd" "$BIN/themes.new" "$BIN/deskd.new"
rm_path "$BIN/desk-shell" "$BIN/desk-shell.new" "$BIN/desk-win11-session" "$DATA_HOME/desktop-themes/labwc"
rm_path "$DATA_HOME/dbus-1/services/$APP_ID.service" "$DATA_HOME/dbus-1/services/$DESK_ID.service"
rm_path "$DATA_HOME/applications/$APP_ID.desktop"
rm_path "$UNITS/deskd.service" "$UNITS/themes-doctor.service"
rm_path "$UNITS"/*.wants/themes-doctor.service "$UNITS"/*.wants/deskd.service
if have systemctl; then run systemctl --user daemon-reload || true; fi
if have update-desktop-database; then run update-desktop-database "$DATA_HOME/applications" || true; fi

say "GSettings schema"
SCHEMAS=$DATA_HOME/glib-2.0/schemas
if [ -e "$SCHEMAS/$DESK_ID.gschema.xml" ]; then
  rm_path "$SCHEMAS/$DESK_ID.gschema.xml"
  if [ "$DRY_RUN" = 1 ] || ls "$SCHEMAS"/*.gschema.xml >/dev/null 2>&1; then
    if have glib-compile-schemas; then run glib-compile-schemas "$SCHEMAS"
    else tb glib-compile-schemas "$SCHEMAS"; fi
  else
    rm_path "$SCHEMAS/gschemas.compiled"        # no schema left in the user directory
  fi
fi

say "extensions"
for u in "${OWN_EXT[@]}"; do rm_path "$EXT_DIR/$u"; done
if [ "$KEEP_EXT" != 1 ]; then
  for u in "${EGO_UUIDS[@]}"; do rm_path "$EXT_DIR/$u"; done
else
  note "kept the third-party extensions (--keep-extensions)"
fi

say "third-party themes, icons, fonts, wallpapers, sounds"
if [ "$KEEP_ASSETS" = 1 ]; then
  note "kept (--keep-assets)"
else
  # names installed by install-assets.sh / generate-assets.sh
  for d in "$DATA_HOME"/themes/WhiteSur* "$DATA_HOME"/themes/Fluent* \
           "$DATA_HOME"/icons/WhiteSur* "$DATA_HOME"/icons/Fluent* \
           "$DATA_HOME"/fonts/Inter "$DATA_HOME"/fonts/JetBrainsMono "$DATA_HOME"/fonts/CascadiaCode \
           "$DATA_HOME"/backgrounds/macos "$DATA_HOME"/backgrounds/windows11 \
           "$DATA_HOME"/sounds/macos-like "$DATA_HOME"/sounds/windows11-like; do
    rm_path "$d"
  done
  if have fc-cache; then run fc-cache -f "$DATA_HOME/fonts" || true; fi
fi

say "cache"
rm_path "$DT_CACHE"

if [ "$PURGE" = 1 ]; then
  say "purge"
  rm_path "$DT_ROOT" "$DATA_HOME/deskd" "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/deskd"
  if have dconf; then run dconf reset -f "/io/github/OlehDzhumyk/Desk/" || true; fi
fi

if [ "$RM_TOOLBOX" = 1 ] && have toolbox && ! in_container; then
  say "toolbox $DT_TOOLBOX"
  run toolbox rm --force "$DT_TOOLBOX" || true
fi

say "done"
note "log out and in again so the shell drops the removed extensions"
[ "$PURGE" = 1 ] || note "your themes and the baseline are still in $DT_ROOT (scripts/uninstall.sh --purge deletes them)"
if have flatpak; then note "Flatpak apps installed by bootstrap are left alone: flatpak uninstall --user com.mattjakeman.ExtensionManager page.tesk.Refine"; fi
exit 0
