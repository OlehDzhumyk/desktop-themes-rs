#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Performance harness. Measures, inside the ISOLATED headless GNOME Shell session of tools/headless
# (own D-Bus bus, display, runtime dir and dconf database; never the live session):
#   deskd      RSS idle / after N clipboard pushes, idle CPU and wakeups, push latency
#   panel      clipboard panel open latency per skin (D-Bus call -> visible / rendered / painted)
#   cli        `themes list` startup and `themes apply` wall time (fake $HOME, private dconf)
#   shell      gnome-shell RSS with desk-bridge enabled vs disabled
# and writes OUT/results.json, OUT/summary.md (against budgets.toml). Budgets are for release builds.
#
#   tools/perf/run.sh [--deskd BIN] [--themes BIN] [--out DIR] [--idle-seconds N] [--pushes N]
#                     [--panel-runs N] [--shell-runs N] [--skip-shell] [--report-only]
# Defaults: target/release/{deskd,themes} when present, else target/debug; OUT=target/perf.
# In a CI container use HS_TOOLBOX=none HS_NO_SHOTS=1 (see tools/headless/ci-env.sh).
set -eu
P=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$P/../.." && pwd)
H=$REPO/tools/headless
export HS_NAME=${HS_NAME:-perf}
DESKD="" THEMES="" OUT="$REPO/target/perf" IDLE=60 PUSHES=200 PANEL_RUNS=30 SHELL_RUNS=2 SKIP_SHELL=0 REPORT_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --deskd) DESKD=$(realpath "$2"); shift 2 ;;
    --themes) THEMES=$(realpath "$2"); shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    --idle-seconds) IDLE=$2; shift 2 ;;
    --pushes) PUSHES=$2; shift 2 ;;
    --panel-runs) PANEL_RUNS=$2; shift 2 ;;
    --shell-runs) SHELL_RUNS=$2; shift 2 ;;
    --skip-shell) SKIP_SHELL=1; shift ;;
    --report-only) REPORT_ONLY=1; shift ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
pick() { for c in "$REPO/target/release/$1" "$REPO/target/debug/$1"; do [ -x "$c" ] && { echo "$c"; return; }; done; }
[ -n "$DESKD" ] || DESKD=$(pick deskd)
[ -n "$THEMES" ] || THEMES=$(pick themes)
[ -x "$DESKD" ] && [ -x "$THEMES" ] || { echo "build first: cargo build --release -p deskd -p themes" >&2; exit 1; }
BRIDGE=$REPO/bridge/build/desk-bridge@desktop-themes-rs
[ -f "$BRIDGE/extension.js" ] || { echo "build the bridge first: (cd bridge && npm ci && npm run build)" >&2; exit 1; }
PROBE=$P/probe/perf-probe@desktop-themes-rs
mkdir -p "$OUT"; OUT=$(cd "$OUT" && pwd)
# shellcheck source=tools/headless/env.sh
. "$H/env.sh"
[ "$HS_RUNTIME" != "$HS_REAL_RUNTIME" ] || { echo "environment is not isolated" >&2; exit 1; }
R="$H/run.sh"
trap '"$H/stop.sh" >/dev/null 2>&1 || true' EXIT

start() {  # start [with-bridge]
  local args=(--ext "$PROBE" --deskd "$DESKD")
  [ "${1:-}" = bridge ] && args+=(--ext "$BRIDGE")
  "$H/start.sh" "${args[@]}" </dev/null > "$HS_STATE/start.log" 2>&1 || { cat "$HS_STATE/start.log"; exit 1; }
}
settle() { sleep "${PERF_SETTLE:-20}"; }
M() { "$R" python3 "$P/measure.py" "$@"; }

echo "== session with desk-bridge: deskd, panel, CLI"
start bridge
settle
M shell-rss --out "$OUT/shell-enabled-1.json" >/dev/null
M deskd --idle-seconds "$IDLE" --pushes "$PUSHES" --out "$OUT/deskd.json" >/dev/null
M panel --runs "$PANEL_RUNS" --out "$OUT/panel.json" >/dev/null
M cli --themes "$THEMES" --themes-dir "$REPO/themes" --workdir "$HS_ROOT/perf-cli" --out "$OUT/cli.json" >/dev/null
"$H/stop.sh" >/dev/null 2>&1 || true

if [ "$SKIP_SHELL" = 0 ]; then
  for i in $(seq "$SHELL_RUNS"); do
    echo "== gnome-shell RSS, round $i: bridge disabled / enabled"
    start; settle; M shell-rss --out "$OUT/shell-disabled-$i.json" >/dev/null; "$H/stop.sh" >/dev/null 2>&1 || true
    if [ "$i" -gt 1 ]; then
      start bridge; settle; M shell-rss --out "$OUT/shell-enabled-$i.json" >/dev/null; "$H/stop.sh" >/dev/null 2>&1 || true
    fi
  done
fi

python3 - "$OUT" "$DESKD" "$THEMES" "$REPO" <<'PY'
import glob, json, statistics, subprocess, sys, os
out, deskd, themes, repo = sys.argv[1:5]
L = lambda n: json.load(open(f"{out}/{n}.json"))
res = {"deskd": L("deskd"), "panel": L("panel"), "cli": L("cli")}
sh = {}
for kind in ("enabled", "disabled"):
    vals = [json.load(open(f))["rss_kib"] for f in sorted(glob.glob(f"{out}/shell-{kind}-*.json"))]
    sh[kind + "_kib"] = vals
if sh["disabled_kib"]:
    sh["rss_delta_kib"] = int(statistics.median(sh["enabled_kib"]) - statistics.median(sh["disabled_kib"]))
res["shell"] = sh
def git(*a):
    try: return subprocess.run(["git", *a], capture_output=True, text=True, cwd=repo).stdout.strip()
    except Exception: return ""
res["env"] = {"deskd": deskd, "themes": themes, "commit": git("rev-parse", "--short", "HEAD"),
              "kernel": os.uname().release, "cpus": os.cpu_count()}
json.dump(res, open(f"{out}/results.json", "w"), indent=2)
PY
EXTRA=()
[ "$REPORT_ONLY" = 1 ] && EXTRA=(--report-only)
python3 "$P/compare.py" "$OUT/results.json" --markdown "$OUT/summary.md" "${EXTRA[@]+"${EXTRA[@]}"}"
