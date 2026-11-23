#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Інтеграційний тест: тема zz-test з усіма чотирма модулями + змішування macos/windows11.
# Запуск: tools/integration-test.sh [шлях-до-themes]   (змінює реальний dconf; наприкінці — reset)
set -u
B=${1:-themes}
R=${DESKTOP_THEMES_ROOT:-$HOME/.local/share/desktop-themes}
T=$R/themes/zz-test
MARK=${XDG_RUNTIME_DIR:-/tmp}/zz-test-hook.log
FAIL=0
ok()   { echo "  ok   $*"; }
bad()  { echo "  FAIL $*"; FAIL=1; }
expect() { # key expected
  local v; v=$(dconf read "$1")
  if [ "$v" = "$2" ]; then ok "$1 = $v"; else bad "$1 = '${v}' (очікували '$2')"; fi
}
# Ключі, якими теми не керують (стан програм/оболонки) і постійне розширення themes-quick
UNMANAGED='(last-panel|app-picker-layout|favorite-apps|check-timestamp|install-timestamp|update-notification-timestamp|enabled-extensions)='
basediff() {
  local d; d=$(dconf dump / | diff "$R/baseline/full.dconf" - | grep '^[<>]' | grep -vE "^[<>] ($UNMANAGED|\[)" | grep -v '^[<>] *$')
  echo "  повний diff: $(dconf dump / | diff "$R/baseline/full.dconf" - | grep -c '^[<>]') рядків; без некерованих ключів: $(printf %s "$d" | grep -c .)"
  if [ -z "$d" ]; then ok "dconf == baseline (крім некерованих ключів)"; else bad "відмінності від baseline:"; echo "$d" | sed 's/^/        /'; fi
  local en; en=$(dconf read /org/gnome/shell/enabled-extensions)
  [ "$en" = "['background-logo@fedorahosted.org', 'desk-bridge@desktop-themes-rs']" ] && ok "enabled-extensions = baseline + desk-bridge" || bad "enabled-extensions = $en"
}

echo "== підготовка zz-test"
rm -rf "$T" "$MARK"; mkdir -p "$T/apps/hooks"
cat > "$T/theme.conf" <<'C'
NAME="ZZ тест"
DESCRIPTION="Тимчасова тема для інтеграційного тесту"
C
printf '[org/gnome/desktop/interface]\nclock-show-seconds=true\n\n[org/gnome/desktop/wm/preferences]\nbutton-layout=%s\n' "'close:'" > "$T/settings.dconf"
printf '# поведінка\n[org/gnome/desktop/peripherals/touchpad]\ntap-to-click=true\n\n[org/gnome/mutter]\nedge-tiling=false\n' > "$T/behavior.dconf"
printf '; звуки\n[org/gnome/desktop/sound]\nevent-sounds=false\n' > "$T/sounds.dconf"
printf '# програми\n[org/gnome/Ptyxis]\nfont-name=%s\n' "'Monospace 13'" > "$T/apps.dconf"
echo "appindicatorsupport@rgcjonas.gmail.com" > "$T/extensions.txt"
for h in apply revert; do
  printf '#!/bin/sh\necho "%s THEME_ID=$THEME_ID PREV=$PREV_THEME_ID DIR=$(basename "$THEME_DIR")" >> %s\n' "$h" "$MARK" > "$T/apps/hooks/$h"
  chmod +x "$T/apps/hooks/$h"
done

echo "== 1. reset (вихідний стан)"
$B reset --no-verify >/dev/null || bad "reset"
basediff

echo "== 2. apply zz-test (усі модулі)"
$B apply zz-test | sed 's/^/  | /'
expect /org/gnome/desktop/interface/clock-show-seconds true
expect /org/gnome/desktop/wm/preferences/button-layout "'close:'"
expect /org/gnome/desktop/peripherals/touchpad/tap-to-click true
expect /org/gnome/mutter/edge-tiling false
expect /org/gnome/desktop/sound/event-sounds false
expect /org/gnome/Ptyxis/font-name "'Monospace 13'"
grep -q "apply THEME_ID=zz-test PREV=fedora DIR=zz-test" "$MARK" && ok "хук apply виконано" || bad "хук apply"
[ "$($B current --json | python3 -c 'import json,sys;print(sorted(set(json.load(sys.stdin)["modules"].values())))')" = "['zz-test']" ] && ok "current: усі модулі zz-test" || bad "current"
dconf read /org/gnome/shell/enabled-extensions | grep -q appindicatorsupport && ok "розширення з extensions.txt увімкнено" || bad "розширення"

echo "== 3. лише sounds з fedora (решта zz-test)"
$B reset --only sounds --no-verify >/dev/null
expect /org/gnome/desktop/sound/event-sounds ""
expect /org/gnome/mutter/edge-tiling false
expect /org/gnome/Ptyxis/font-name "'Monospace 13'"

echo "== 4. reset"
$B reset --no-verify | tail -3 | sed 's/^/  | /'
grep -q "revert THEME_ID=zz-test" "$MARK" && ok "хук revert виконано" || bad "хук revert"
basediff

echo "== 5. змішування: look=macos, behavior=windows11"
$B apply macos --only look --no-verify | tail -2 | sed 's/^/  | /'
$B apply windows11 --module behavior --no-verify --yes | tail -2 | sed 's/^/  | /'
$B current | sed 's/^/  | /'
cur=$($B current --json | python3 -c 'import json,sys;m=json.load(sys.stdin)["modules"];print(m.get("look"),m.get("behavior"),m.get("sounds"),m.get("apps"))')
[ "$cur" = "macos windows11 fedora fedora" ] && ok "вибір модулів: $cur" || bad "вибір модулів: $cur"
expect /org/gnome/desktop/interface/gtk-theme "'WhiteSur-Light'"
# ключі поведінки windows11 є, macOS-поведінки немає
W=$(grep -m1 -A1 '^\[org/gnome/desktop/wm/keybindings\]' "$R/themes/windows11/behavior.dconf" | tail -1)
WK=/org/gnome/desktop/wm/keybindings/${W%%=*}; WV=${W#*=}
expect "$WK" "$WV"
expect /org/gnome/desktop/sound/theme-name ""
dconf read /org/gnome/shell/enabled-extensions | grep -q dash-to-dock && ok "розширення macOS (dash-to-dock) увімкнено" || bad "dash-to-dock"
$B list | sed 's/^/  | /'

echo "== 6. reset після змішування"
$B reset --no-verify >/dev/null
basediff

rm -rf "$T"
echo
[ $FAIL = 0 ] && echo "РЕЗУЛЬТАТ: усі перевірки пройдено" || echo "РЕЗУЛЬТАТ: є помилки"
exit $FAIL
