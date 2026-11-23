#!/usr/bin/python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Measurements of tools/perf that talk to the isolated headless session (run through
tools/headless/run.sh, see run.sh): deskd memory/CPU/wakeups, clipboard panel latency.

  measure.py deskd  --idle-seconds 60 --pushes 200 --out FILE.json
  measure.py panel  --runs 30 --skins default,macos,windows11 --out FILE.json
  measure.py shell-rss --out FILE.json
  measure.py cli --themes BIN --themes-dir DIR --workdir DIR --out FILE.json

Only the standard library and PyGObject (GLib/Gio) are used. Nothing here writes outside --out.
"""
import argparse
import json
import os
import statistics
import sys
import time

import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

TICKS = os.sysconf("SC_CLK_TCK")
DESK = "io.github.OlehDzhumyk.Desk"
CLIP_PATH = "/io/github/OlehDzhumyk/Desk/Clipboard"
CLIP_IFACE = DESK + ".Clipboard"


def bus():
    addr = os.environ["DBUS_SESSION_BUS_ADDRESS"]
    if "headless-" not in addr:
        sys.exit("refusing to run: DBUS_SESSION_BUS_ADDRESS is not a headless test bus (%s)" % addr)
    return Gio.DBusConnection.new_for_address_sync(
        addr, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None, None)


def call(conn, name, path, iface, method, args=None, rtype=None, timeout=30000):
    return conn.call_sync(name, path, iface, method, args, GLib.VariantType(rtype) if rtype else None,
                          Gio.DBusCallFlags.NONE, timeout, None)


def owner_pid(conn, name):
    r = call(conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
             "GetConnectionUnixProcessID", GLib.Variant("(s)", (name,)), "(u)")
    return r.unpack()[0]


def status(pid):
    d = {}
    with open("/proc/%d/status" % pid) as f:
        for line in f:
            k, _, v = line.partition(":")
            d[k] = v.strip()
    return d


def rss_kib(pid):
    return int(status(pid)["VmRSS"].split()[0])


def cpu_ticks(pid):
    with open("/proc/%d/stat" % pid) as f:
        rest = f.read().rsplit(")", 1)[1].split()
    return int(rest[11]) + int(rest[12])  # utime + stime


def ctxt_switches(pid):
    """(voluntary, involuntary) summed over all threads."""
    v = n = 0
    for t in os.listdir("/proc/%d/task" % pid):
        try:
            s = status_task(pid, t)
        except OSError:
            continue
        v += int(s["voluntary_ctxt_switches"])
        n += int(s["nonvoluntary_ctxt_switches"])
    return v, n


def status_task(pid, tid):
    d = {}
    with open("/proc/%d/task/%s/status" % (pid, tid)) as f:
        for line in f:
            k, _, v = line.partition(":")
            d[k] = v.strip()
    return d


def pct(values, p):
    values = sorted(values)
    if not values:
        return None
    k = (len(values) - 1) * p / 100
    lo, hi = int(k), min(int(k) + 1, len(values) - 1)
    return values[lo] + (values[hi] - values[lo]) * (k - lo)


def summary(values, digits=3):
    if not values:
        return None
    return {"n": len(values), "min": round(min(values), digits), "p50": round(pct(values, 50), digits),
            "p95": round(pct(values, 95), digits), "max": round(max(values), digits),
            "mean": round(statistics.fmean(values), digits)}


def push(conn, text, mime="text/plain;charset=utf-8"):
    return call(conn, DESK, CLIP_PATH, CLIP_IFACE, "Push",
                GLib.Variant("(sayass)", (mime, text.encode(), [mime], "perf")), "(t)").unpack()[0]


def measure_deskd(a):
    conn = bus()
    # activate deskd through the private bus (its service file points at --deskd BIN)
    t0 = time.monotonic()
    call(conn, DESK, CLIP_PATH, CLIP_IFACE, "List", GLib.Variant("(su)", ("", 1)), "(a(tssssbxt))")
    activation_ms = (time.monotonic() - t0) * 1000
    pid = owner_pid(conn, DESK)
    exe = os.readlink("/proc/%d/exe" % pid)
    time.sleep(3)
    out = {"pid": pid, "exe": exe, "activation_ms": round(activation_ms, 1),
           "threads": int(status(pid)["Threads"]), "rss_start_kib": rss_kib(pid)}

    # idle: CPU time and wakeups (voluntary context switches of all threads) over the window
    c0, (v0, n0), w0 = cpu_ticks(pid), ctxt_switches(pid), time.monotonic()
    time.sleep(a.idle_seconds)
    c1, (v1, n1), w1 = cpu_ticks(pid), ctxt_switches(pid), time.monotonic()
    minutes = (w1 - w0) / 60
    out["idle"] = {"seconds": round(w1 - w0, 1),
                   "cpu_percent": round((c1 - c0) / TICKS / (w1 - w0) * 100, 4),
                   "cpu_ticks": c1 - c0,
                   "wakeups_per_min": round((v1 - v0) / minutes, 2),
                   "involuntary_per_min": round((n1 - n0) / minutes, 2)}
    out["rss_idle_kib"] = rss_kib(pid)
    out["rss_hwm_kib"] = int(status(pid)["VmHWM"].split()[0])

    # pushes: distinct texts of 200 B .. 8 KiB (what real copying looks like), latency per push
    lat = []
    ids = 0
    for i in range(a.pushes):
        size = 200 + (i * 7919) % 8000
        text = ("push %d " % i) + ("lorem ipsum %d " % i) * (size // 16)
        t = time.monotonic()
        if push(conn, text):
            ids += 1
        lat.append((time.monotonic() - t) * 1000)
    time.sleep(2)
    out["pushes"] = {"count": a.pushes, "stored": ids, "latency_ms": summary(lat, 2)}
    out["rss_after_pushes_kib"] = rss_kib(pid)
    count = call(conn, DESK, CLIP_PATH, "org.freedesktop.DBus.Properties", "Get",
                 GLib.Variant("(ss)", (CLIP_IFACE, "Count")), "(v)").unpack()[0]
    out["items_in_history"] = count
    # idle again after the load: does it go back to quiet?
    c0, (v0, _), w0 = cpu_ticks(pid), ctxt_switches(pid), time.monotonic()
    time.sleep(min(30, a.idle_seconds))
    c1, (v1, _), w1 = cpu_ticks(pid), ctxt_switches(pid), time.monotonic()
    out["idle_after_pushes"] = {"seconds": round(w1 - w0, 1),
                                "cpu_percent": round((c1 - c0) / TICKS / (w1 - w0) * 100, 4),
                                "wakeups_per_min": round((v1 - v0) / ((w1 - w0) / 60), 2)}
    out["rss_final_kib"] = rss_kib(pid)
    call(conn, DESK, CLIP_PATH, CLIP_IFACE, "Clear", GLib.Variant("(b)", (False,)), "()")
    return out


def measure_panel(a):
    conn = bus()
    probe = ("org.desktopthemesrs.PerfProbe", "/org/desktopthemesrs/PerfProbe", "org.desktopthemesrs.PerfProbe")
    # something to render: a realistic history of 40 items
    for i in range(40):
        push(conn, "panel item %d: " % i + "the quick brown fox jumps over the lazy dog " * (1 + i % 4))
    res = {}
    for skin in a.skins.split(","):
        # the bridge listens to changed::skin, so write it through the private dconf database
        os.system("dconf write /io/github/OlehDzhumyk/Desk/skin \"'%s'\"" % skin)
        for _ in range(100):
            time.sleep(0.1)
            if call(conn, *probe[:2], probe[2], "Skin", None, "(s)").unpack()[0] == skin:
                break
        else:
            res[skin] = {"error": "skin did not switch"}
            continue
        time.sleep(1)
        vis, ren, pai, rtt = [], [], [], []
        first = None
        for i in range(a.runs + 1):
            t = time.monotonic()
            v, r, p = call(conn, *probe[:2], probe[2], "Toggle", None, "(ddd)").unpack()
            rtt.append((time.monotonic() - t) * 1000)
            if i == 0:
                first = {"visible_ms": round(v, 2), "rendered_ms": round(r, 2), "painted_ms": round(p, 2)}
                continue  # the first open of a skin is cold; reported separately
            vis.append(v)
            ren.append(r)
            pai.append(p)
            time.sleep(0.15)
        res[skin] = {"cold_first_open": first, "visible_ms": summary(vis, 2), "rendered_ms": summary(ren, 2),
                     "painted_ms": summary(pai, 2), "dbus_roundtrip_ms": summary(rtt[1:], 2)}
    return res


def measure_cli(a):
    """Wall time of the `themes` CLI. It runs with a FAKE $HOME (GTK links, flatpak overrides, the
    extension directory all derive from it), its own theme root and a no-op `flatpak`, against the
    private dconf database and bus of the headless session: nothing of the real session is touched."""
    import shutil
    import subprocess
    work = os.path.realpath(a.workdir)
    if "desktop-themes-headless" not in work:
        sys.exit("refusing: --workdir must live under .../desktop-themes-headless/ (got %s)" % work)
    home, root, shim = work + "/home", work + "/root", work + "/shim"
    shutil.rmtree(work, ignore_errors=True)
    for d in (home, shim):
        os.makedirs(d)
    shutil.copytree(a.themes_dir, root + "/themes")
    # baseline snapshot of the PRIVATE dconf db, as scripts/bootstrap.sh takes it for a real user
    os.makedirs(root + "/baseline-perf")
    with open(root + "/baseline-perf/full.dconf", "w") as f:
        subprocess.run(["dconf", "dump", "/"], stdout=f, check=True)
    os.symlink("baseline-perf", root + "/baseline")
    with open(root + "/current", "w") as f:
        f.write("fedora\n")
    with open(shim + "/flatpak", "w") as f:
        f.write("#!/bin/sh\nexit 0\n")
    os.chmod(shim + "/flatpak", 0o755)
    env = dict(os.environ, HOME=home, DESKTOP_THEMES_ROOT=root, PATH=shim + ":" + os.environ["PATH"],
               XDG_DATA_HOME=home + "/.local/share")
    # XDG_CONFIG_HOME stays the private one of the session: that is where the private dconf db lives

    def run(*args, n=1, warm=True):
        times = []
        if warm:
            subprocess.run([a.themes, *args], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(n):
            t = time.monotonic()
            r = subprocess.run([a.themes, *args], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            times.append((time.monotonic() - t) * 1000)
            if r.returncode not in (0, 3):
                sys.stderr.write("themes %s: exit %d: %s\n" % (" ".join(args), r.returncode, r.stderr.decode()[-300:]))
        return summary(times, 1)

    out = {"binary": a.themes, "list_ms": run("list", n=20), "list_json_ms": run("--json", "list", n=20),
           "current_ms": run("current", n=10), "version_ms": run("--version", n=20)}
    out["apply_dry_run_ms"] = run("apply", "macos", "--dry-run", n=5)
    # real applies, into the private dconf database; the first one also takes the baseline snapshot
    out["apply_first_ms"] = run("apply", "macos", "--no-verify", "--yes", warm=False)
    seq = []
    for t in ("windows11", "macos", "fedora") * 2:
        seq.append(run("apply", t, "--no-verify", "--yes", warm=False)["p50"])
    out["apply_no_verify_ms"] = summary(seq, 1)
    out["reset_ms"] = run("reset", "--no-verify", warm=False)
    return out


def measure_shell_rss(_a):
    conn = bus()
    pid = owner_pid(conn, "org.gnome.Shell")
    time.sleep(a_settle())
    s = status(pid)
    return {"pid": pid, "rss_kib": int(s["VmRSS"].split()[0]), "rss_anon_kib": int(s["RssAnon"].split()[0]),
            "threads": int(s["Threads"])}


def a_settle():
    return float(os.environ.get("PERF_SETTLE", "20"))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("deskd")
    d.add_argument("--idle-seconds", type=float, default=60)
    d.add_argument("--pushes", type=int, default=200)
    p = sub.add_parser("panel")
    p.add_argument("--runs", type=int, default=30)
    p.add_argument("--skins", default="default,macos,windows11")
    sub.add_parser("shell-rss")
    c = sub.add_parser("cli")
    c.add_argument("--themes", required=True)
    c.add_argument("--themes-dir", required=True)
    c.add_argument("--workdir", required=True)
    for s in sub.choices.values():
        s.add_argument("--out", required=True)
    a = ap.parse_args()
    data = {"deskd": measure_deskd, "panel": measure_panel, "shell-rss": measure_shell_rss, "cli": measure_cli}[a.cmd](a)
    with open(a.out, "w") as f:
        json.dump(data, f, indent=2)
    print(json.dumps(data, indent=2))


if __name__ == "__main__":
    main()
