#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Move an installation made from an older, private build of this project to the public one.
# Idempotent: running it again changes nothing.
#
# Legacy installs used their own ids: a D-Bus name / app id `io.github.<someone>.Themes` and the
# extensions `themes-quick@<host>.local` and `lockscreen-layout@<host>.local`. This script
#   1. retires them: their D-Bus activation file and .desktop file are removed, the extensions are
#      dropped from enabled/disabled-extensions (lockscreen-layout is switched to
#      lockscreen-layout@desktop-themes-rs), their directories are moved to the theme root's trash;
#      themes-quick@desktop-themes-rs and logomenu@aryan_k, which desk-bridge replaced, are retired the same way;
#   2. renames those uuids in every installed theme's extensions.txt (your own themes too) and in
#      state.json (applied/broken extensions);
#   3. installs the public build: themes (scripts/install-themes.sh, bundled themes only, stale
#      files go to trash), binaries, deskd, D-Bus/systemd files, GSettings schema, extensions
#      (data/install.sh), and enables desk-bridge@desktop-themes-rs.
# state.json, baseline*, backups/ and themes with other ids are kept. It never touches
# org.gnome.shell disable-user-extensions and never enables theme extensions by itself.
# Extensions installed while you are logged in load after the next login.
#
# Usage: scripts/migrate-from-legacy.sh [--dry-run] [--reapply]
#   --reapply  afterwards re-apply the current theme (`themes apply <look theme>`) and run doctor
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_common_flags "$@"
REAPPLY=0
for a in "${DT_ARGS[@]+"${DT_ARGS[@]}"}"; do
  case "$a" in
    --reapply) REAPPLY=1 ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) die "unknown option: $a" ;;
  esac
done

NEW_APP=io.github.OlehDzhumyk.Themes
BRIDGE=desk-bridge@desktop-themes-rs
LOCK_NEW=lockscreen-layout@desktop-themes-rs
EXT_DIR=$DATA_HOME/gnome-shell/extensions
STAMP=$(date +%Y%m%d-%H%M%S)
TRASH=$DT_ROOT/trash/legacy-$STAMP

# ---- 1a. D-Bus activation and .desktop files of legacy app ids
say "legacy D-Bus services and launchers"
for f in "$DATA_HOME"/dbus-1/services/io.github.*.Themes.service "$DATA_HOME"/applications/io.github.*.Themes.desktop; do
  [ -e "$f" ] || continue
  case "$(basename "$f")" in "$NEW_APP".*) continue ;; esac
  note "remove $(basename "$f")"
  run rm -f -- "$f"
done

# ---- 1b. enabled/disabled-extensions
say "extension lists"
fix_list() { # key
  local key=$1 cur new
  cur=$(dconf read "$key" 2>/dev/null || true)
  [ -n "$cur" ] || return 0
  new=$(python3 - "$cur" "$BRIDGE" "$key" <<'PY'
import ast, re, sys
cur, bridge, key = sys.argv[1], sys.argv[2], sys.argv[3]
lst = ast.literal_eval(re.sub(r"^@as ", "", cur))
out = []
for u in lst:
    if u in ("lockscreen-layout@desktop-themes-rs", bridge):
        r = u
    elif re.fullmatch(r"lockscreen-layout@.*\.local", u):
        r = "lockscreen-layout@desktop-themes-rs"
    elif u.startswith("themes-quick@") or u == "logomenu@aryan_k":
        r = ""
    else:
        r = u
    if r and r not in out:
        out.append(r)
if key.endswith("enabled-extensions") and bridge not in out:
    out.append(bridge)
if key.endswith("disabled-extensions") and bridge in out:
    out.remove(bridge)
print("[" + ", ".join("'%s'" % u for u in out) + "]" if out else "@as []")
PY
)
  if [ "$new" != "$cur" ]; then
    note "$key: $new"
    run dconf write "$key" "$new"
  fi
}
fix_list /org/gnome/shell/enabled-extensions
fix_list /org/gnome/shell/disabled-extensions

# ---- 1c. legacy extension directories -> trash (after the shell had a moment to disable them)
moved=0
for d in "$EXT_DIR"/themes-quick@* "$EXT_DIR"/lockscreen-layout@*.local "$EXT_DIR"/logomenu@aryan_k; do
  [ -d "$d" ] || continue
  [ "$moved" = 1 ] || { [ "$DRY_RUN" = 1 ] || sleep 2; }
  moved=1
  note "retire $(basename "$d") -> ${TRASH#"$DT_ROOT"/}/"
  run mkdir -p "$TRASH"
  run mv -- "$d" "$TRASH/"
done

# ---- 2. uuids in installed themes and in state.json
say "theme extension lists and state"
for f in "$DT_ROOT"/themes/*/extensions.txt; do
  [ -f "$f" ] || continue
  if grep -Eq '^[[:space:]]*[+-]?(lockscreen-layout@.*\.local|themes-quick@|logomenu@aryan_k([[:space:]]|#|$))' "$f"; then
    note "update $(basename "$(dirname "$f")")/extensions.txt"
    if [ "$DRY_RUN" = 0 ]; then
      sed -E -i \
        -e "s/^([[:space:]]*[+-]?)lockscreen-layout@[^[:space:]#]*\.local/\1$LOCK_NEW/" \
        -e '/^[[:space:]]*[+-]?themes-quick@/d' \
        -e '/^[[:space:]]*[+-]?logomenu@aryan_k([[:space:]]|#|$)/d' "$f"
    fi
  fi
done
if [ -f "$DT_ROOT/state.json" ]; then
  if [ "$DRY_RUN" = 1 ]; then
    note "+ rename legacy uuids in state.json"
  else
    python3 - "$DT_ROOT/state.json" <<'PY'
import json, re, sys, os
p = sys.argv[1]
st = json.load(open(p, encoding="utf-8"))
def ren(u):
    if re.fullmatch(r"lockscreen-layout@.*\.local", u):
        return "lockscreen-layout@desktop-themes-rs"
    if u.startswith("themes-quick@") or u == "logomenu@aryan_k":
        return None
    return u
changed = False
if isinstance(st.get("applied_extensions"), list):
    new = []
    for u in st["applied_extensions"]:
        r = ren(u)
        if r and r not in new:
            new.append(r)
    changed |= new != st["applied_extensions"]
    st["applied_extensions"] = new
if isinstance(st.get("broken_extensions"), list):
    new = [b for b in st["broken_extensions"] if ren(b.get("uuid", "")) is not None]
    for b in new:
        b["uuid"] = ren(b["uuid"])
    changed |= new != st["broken_extensions"]
    st["broken_extensions"] = new
if changed:
    tmp = p + ".tmp"
    json.dump(st, open(tmp, "w", encoding="utf-8"), ensure_ascii=False, indent=2)
    os.replace(tmp, p)
    print("    state.json updated")
PY
  fi
fi

# ---- 3. public build
say "install the public build"
if [ ! -x "$REPO/target/release/themes" ] || [ ! -x "$REPO/target/release/deskd" ]; then
  tb bash -c 'export PATH="$HOME/.cargo/bin:$PATH"; cd "$1" && cargo build --release --locked' _ "$REPO"
fi
export DT_TOOLBOX
if [ "$DRY_RUN" = 1 ]; then
  "$REPO/scripts/install-themes.sh" --dry-run
  "$REPO/data/install.sh" --dry-run
else
  "$REPO/scripts/install-themes.sh"
  "$REPO/data/install.sh"
fi
fix_list /org/gnome/shell/enabled-extensions

if [ "$REAPPLY" = 1 ]; then
  look=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("modules",{}).get("look",""))' "$DT_ROOT/state.json" 2>/dev/null || true)
  if [ -n "$look" ]; then
    say "re-apply the current theme: $look"
    run "$HOME/.local/bin/themes" apply "$look"
  fi
  run "$HOME/.local/bin/themes" doctor || true
fi
say "migration done; log out and in once so GNOME Shell loads $BRIDGE and $LOCK_NEW"
