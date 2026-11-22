#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Compare tools/perf results against budgets.toml and write a markdown summary.

  compare.py RESULTS.json [--budgets budgets.toml] [--markdown OUT.md] [--strict]

Exit status: 0 = within budget (or only a debug build / --report-only), 1 = a budget is exceeded.
A debug build is detected from the deskd path in the results; budgets are for release builds, so a
debug run is reported but does not fail unless --strict.
"""
import argparse
import json
import sys
import tomllib


def dig(data, path):
    for part in path.split("."):
        if not isinstance(data, dict) or part not in data:
            return None
        data = data[part]
    return data


def fmt(v, unit):
    if v is None:
        return "n/a"
    if unit == "KiB":
        return "%.1f MiB" % (v / 1024)
    return "%g %s" % (round(v, 3), unit)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("results")
    ap.add_argument("--budgets", default=__file__.rsplit("/", 1)[0] + "/budgets.toml")
    ap.add_argument("--markdown")
    ap.add_argument("--strict", action="store_true")
    ap.add_argument("--report-only", action="store_true", help="always exit 0")
    a = ap.parse_args()
    res = json.load(open(a.results))
    budgets = tomllib.load(open(a.budgets, "rb"))["budget"]
    exe = dig(res, "deskd.exe") or ""
    debug = "/debug/" in exe
    rows, failed = [], []
    for b in budgets:
        v = dig(res, b["metric"])
        limit = b["max"]
        if v is None:
            status = "skipped"
        elif v <= limit:
            status = "ok"
        else:
            status = "OVER"
            failed.append(b["id"])
        rows.append((b["id"], fmt(v, b["unit"]), "<= " + fmt(limit, b["unit"]), status, b["note"]))
    lines = ["## Performance vs budgets", ""]
    if debug:
        lines += ["> Debug build (`%s`): budgets are for release builds, so overruns are informational." % exe, ""]
    lines += ["| budget | measured | limit | status | what |", "|---|---|---|---|---|"]
    lines += ["| %s | %s | %s | %s | %s |" % r for r in rows]
    text = "\n".join(lines) + "\n"
    print(text)
    if a.markdown:
        open(a.markdown, "w").write(text)
    if failed and not a.report_only and (a.strict or not debug):
        print("over budget: " + ", ".join(failed), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
