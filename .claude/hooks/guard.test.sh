#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# shellcheck disable=SC2016  # the commands under test are literal strings
# Table test for guard.sh: `.claude/hooks/guard.test.sh`. Prints failures, exits 1 on any.
set -u
here=$(cd "$(dirname "$0")" && pwd)
export CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-$(cd "$here/../.." && pwd)}
fail=0
t() { # expected-exit command
  local want=$1 cmd=$2 got
  got=$(jq -n --arg c "$cmd" '{tool_input:{command:$c}}' | "$here/guard.sh" >/dev/null 2>&1; echo $?)
  [ "$got" = "$want" ] || { echo "FAIL (want $want, got $got): $cmd"; fail=1; }
}
t 2 'pkill -f gnome-shell'
t 2 'cd x && killall deskd'
t 2 'ssh host '"'"'pkill foo'"'"
t 2 'pgrep -f deskd | xargs kill'
t 2 'dconf write /org/gnome/desktop/interface/font-name "x"'
t 2 'gsettings set org.gnome.desktop.interface gtk-theme Adwaita'
t 2 'themes apply macos'
t 2 'git push origin main'
t 2 'git -C ~/Developer/x push'
t 2 'cd a; git push --force'
t 0 'DCONF_PROFILE=/tmp/x/profile dconf write /a/b 1'
t 0 'XDG_CONFIG_HOME=$(mktemp -d) gsettings set a b c'
t 0 'GSETTINGS_BACKEND=memory gsettings set a b c'
t 0 'tools/headless/run.sh dconf write /a/b 1'
t 0 'themes apply macos --dry-run'
t 0 'git commit -m "explain why pkill is banned"'
t 0 'git status && git log --oneline'
t 0 'grep -rn pkill scripts'
t 0 'cargo test --workspace'
t 0 'echo push'
if [ -d "$CLAUDE_PROJECT_DIR/themes/macos-glass" ]; then
  t 2 'tools/headless/run.sh themes apply macos-glass'
  t 0 'tools/headless/run.sh themes apply macos-glass --yes'
  t 0 'tools/headless/run.sh themes apply macos'
fi
[ $fail = 0 ] && echo "guard tests: ok"
exit $fail
