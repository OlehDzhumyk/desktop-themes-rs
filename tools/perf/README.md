# tools/perf

Performance harness for deskd, the clipboard panel and the CLI. It runs **only inside the isolated
headless session** of `tools/headless` (own D-Bus bus, display, runtime dir and dconf database) and
refuses to run against any other bus.

```sh
cargo build --release -p deskd -p themes
(cd bridge && npm ci && npm run build)
export HS_TOOLBOX=<toolbox with dbus-daemon>        # HS_TOOLBOX=none in a container
tools/perf/run.sh                                   # about 8 minutes; writes target/perf/{results.json,summary.md}
tools/perf/run.sh --idle-seconds 10 --panel-runs 5 --skip-shell    # quick smoke run
tools/perf/compare.py target/perf/results.json      # re-check results against budgets.toml
```

| What | How |
|---|---|
| deskd RSS idle / after 200 pushes, idle CPU, wakeups per minute | `/proc/<pid>/{status,stat,task/*/status}` of the process that owns `io.github.OlehDzhumyk.Desk` on the private bus; wakeups are the voluntary context switches of all threads over the idle window (60 s by default) |
| clipboard panel open latency | `probe/perf-probe@desktop-themes-rs`, a test-only extension loaded into the headless shell next to desk-bridge. Its `Toggle()` D-Bus method calls the bridge's own `ClipboardFeature` panel (the code path of the keyboard shortcut) and reports ms from the call to *visible*, *rendered* (items from deskd drawn) and *painted* (next frame). No change to the bridge or deskd. |
| `themes list`, `themes apply` | wall time of the real binary; it runs with a **fake `$HOME`**, its own theme root, a no-op `flatpak` and the session's private dconf database, so nothing of the real desktop is touched |
| gnome-shell RSS cost of desk-bridge | a headless shell started with and without the bridge, median of several rounds |

Budgets live in `budgets.toml`; they are for **release** builds. Software rendering in a headless
session and a shared machine make absolute times pessimistic: compare runs of the same machine.
