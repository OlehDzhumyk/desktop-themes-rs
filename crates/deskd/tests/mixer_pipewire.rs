// SPDX-License-Identifier: GPL-3.0-or-later
//! End-to-end test of the mixer against a real, isolated PipeWire: its own `dbus-daemon`,
//! `pipewire` (tests/pipewire/pipewire.conf: null sinks, no hardware) and `wireplumber` (device
//! monitors off), all in a temporary `XDG_RUNTIME_DIR` with a cleared environment, so nothing of
//! the session's own audio is reachable. deskd runs from `CARGO_BIN_EXE_deskd` on that bus and a
//! `pw-cat` stream plays silence into it. Skipped (passes with a note) when the programs are not
//! installed.
//!
//! Every wait is on a `Changed` signal of deskd; a watchdog only guards against a hang (it kills
//! the processes this test started and fails).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use desk_proto::mixer::{ChangedStream, MixerProxy};
use desk_proto::{BUS_NAME, MixerApp, MixerDevice};
use futures_util::StreamExt;

const APP: &str = "org.example.MixerTest";

fn have(prog: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
}

/// The processes of this test; dropping it kills them (and only them).
struct Procs(Vec<Child>);

impl Drop for Procs {
    fn drop(&mut self) {
        for c in self.0.iter_mut().rev() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new() -> Env {
        let dir = std::env::temp_dir().join(format!("deskd-mixer-pw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pipewire");
        for (sub, file, from) in [
            ("cfg/wireplumber/wireplumber.conf.d", "90-test.conf", "wireplumber.conf"),
            ("cfg/pipewire/client.conf.d", "90-test.conf", "client.conf"),
        ] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
            std::fs::copy(here.join(from), dir.join(sub).join(file)).unwrap();
        }
        for d in ["run", "state", "home", "data"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.join("run"), std::fs::Permissions::from_mode(0o700)).unwrap();
        Env { dir }
    }

    fn bus(&self) -> String {
        format!("unix:path={}", self.dir.join("run/bus").display())
    }

    /// A command with nothing of the session's environment.
    fn cmd(&self, prog: &str) -> Command {
        let d = &self.dir;
        let mut c = Command::new(prog);
        c.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("LANG", "C")
            .env("HOME", d.join("home"))
            .env("XDG_RUNTIME_DIR", d.join("run"))
            .env("PIPEWIRE_RUNTIME_DIR", d.join("run"))
            .env("XDG_CONFIG_HOME", d.join("cfg"))
            .env("XDG_STATE_HOME", d.join("state"))
            .env("XDG_DATA_HOME", d.join("data"))
            .env("DBUS_SESSION_BUS_ADDRESS", self.bus())
            .env("GSETTINGS_BACKEND", "memory")
            .stdin(Stdio::null());
        c
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Start `c` and return once a line of its output contains `ready` (the rest is drained).
fn start_until(mut c: Command, stderr: bool, ready: &str) -> Child {
    if stderr {
        c.stderr(Stdio::piped()).stdout(Stdio::null());
    } else {
        c.stdout(Stdio::piped()).stderr(Stdio::null());
    }
    let mut child = c.spawn().unwrap();
    let out: Box<dyn std::io::Read + Send> = if stderr {
        Box::new(child.stderr.take().unwrap())
    } else {
        Box::new(child.stdout.take().unwrap())
    };
    let (tx, rx) = mpsc::channel();
    let ready = ready.to_string();
    std::thread::spawn(move || {
        let mut tx = Some(tx);
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if line.contains(&ready)
                && let Some(t) = tx.take()
            {
                let _ = t.send(());
            }
        }
    });
    rx.recv().expect("the process ended before it was ready");
    child
}

async fn devices(p: &MixerProxy<'_>) -> Vec<MixerDevice> {
    p.list_devices("output").await.unwrap()
}

async fn app(p: &MixerProxy<'_>) -> Option<MixerApp> {
    p.list_apps(true).await.unwrap().into_iter().find(|a| a.key == APP)
}

/// Re-check after every `Changed` until `check` says yes.
macro_rules! wait_for {
    ($changed:expr, $check:expr) => {
        loop {
            if let Some(v) = $check {
                break v;
            }
            $changed.next().await.expect("deskd went away");
        }
    };
}

#[test]
fn mixer_against_an_isolated_pipewire() {
    let missing: Vec<&str> = ["dbus-daemon", "pipewire", "wireplumber", "pw-cat"]
        .into_iter()
        .filter(|p| !have(p))
        .collect();
    if !missing.is_empty() {
        eprintln!("SKIPPED: not installed: {}", missing.join(", "));
        return;
    }
    let env = Env::new();
    let mut procs = Procs(Vec::new());

    let mut bus = env.cmd("dbus-daemon");
    bus.args(["--session", "--nofork", "--print-address=1"])
        .arg(format!("--address={}", env.bus()));
    procs.0.push(start_until(bus, false, "unix:"));

    let conf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pipewire/pipewire.conf");
    let mut pw = env.cmd("pipewire");
    pw.arg("-c").arg(&conf).env("PIPEWIRE_DEBUG", "3");
    procs.0.push(start_until(pw, true, "Listening on 'pipewire-0'"));

    procs.0.push(env.cmd("wireplumber").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());

    // a hang guard, not a timing assertion: kill what this test started and fail
    let pids: Vec<u32> = procs.0.iter().map(Child::id).collect();
    let (done, watchdog) = mpsc::channel::<()>();
    let guard = std::thread::spawn(move || {
        if watchdog.recv_timeout(Duration::from_secs(120)) == Err(mpsc::RecvTimeoutError::Timeout) {
            for pid in pids {
                let _ = Command::new("kill").arg(pid.to_string()).status();
            }
            eprintln!("mixer test hung; killed its processes");
            std::process::exit(1);
        }
    });

    zbus::block_on(async {
        let conn = zbus::connection::Builder::address(env.bus().as_str())
            .build()
            .await
            .unwrap();
        let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
        let mut owners = dbus.receive_name_owner_changed().await.unwrap();

        let mut deskd = env.cmd(env!("CARGO_BIN_EXE_deskd"));
        deskd.stdout(Stdio::null()).stderr(Stdio::null());
        procs.0.push(deskd.spawn().unwrap());
        let name = zbus::names::BusName::try_from(BUS_NAME).unwrap();
        while !dbus.name_has_owner(name.clone()).await.unwrap() {
            owners.next().await.unwrap();
        }

        let p = MixerProxy::builder(&conn)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .unwrap();
        let mut changed: ChangedStream = p.receive_changed().await.unwrap();
        assert_eq!(p.version().await.unwrap(), 1);

        // the graph arrives: two sinks, WirePlumber picks the speakers (higher priority)
        let (speakers, headphones) = wait_for!(changed, {
            let d = devices(&p).await;
            let id = |n: &str| d.iter().find(|x| x.name == n).map(|x| x.id);
            match (id("test_speakers"), id("test_headphones")) {
                (Some(s), Some(h)) if d.iter().any(|x| x.id == s && x.is_default) => Some((s, h)),
                _ => None,
            }
        });
        assert!(p.available().await.unwrap());
        assert_eq!(p.default_output().await.unwrap(), speakers);
        let inputs = p.list_devices("input").await.unwrap();
        assert!(inputs.iter().any(|d| d.name == "test_mic"), "{inputs:?}");

        // an application starts playing
        let mut cat = env.cmd("pw-cat");
        cat.args(["--playback", "--raw", "--format", "s16", "--rate", "48000", "--channels", "2"])
            .arg("-P")
            .arg(format!("{{ application.name = \"Mixer Test\" application.id = \"{APP}\" }}"))
            .arg("-")
            .stdin(std::fs::File::open("/dev/zero").unwrap())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        procs.0.push(cat.spawn().unwrap());
        let a = wait_for!(changed, app(&p).await.filter(|a| a.device == speakers));
        assert_eq!(a.name, "Mixer Test");

        // AC-MIX-02: the server shows 30 % for that stream
        p.set_app_volume(APP, 0.3).await.unwrap();
        wait_for!(changed, app(&p).await.filter(|a| (a.volume - 0.3).abs() < 0.01));
        p.set_app_mute(APP, true).await.unwrap();
        wait_for!(changed, app(&p).await.filter(|a| a.mute));
        p.set_app_mute(APP, false).await.unwrap();
        wait_for!(changed, app(&p).await.filter(|a| !a.mute));

        // AC-MIX-01: the second device becomes the default and the stream moves with it
        p.set_default("output", headphones).await.unwrap();
        wait_for!(changed, {
            let d = devices(&p).await;
            d.iter().any(|x| x.id == headphones && x.is_default).then_some(())
        });
        assert_eq!(p.default_output().await.unwrap(), headphones);
        wait_for!(changed, app(&p).await.filter(|a| a.device == headphones));

        // "Play on…": back to the speakers, explicitly
        p.move_app(APP, speakers).await.unwrap();
        wait_for!(changed, app(&p).await.filter(|a| a.device == speakers));

        // device volume (a null sink has no route: the node's own volume)
        p.set_volume(headphones, 0.5).await.unwrap();
        wait_for!(changed, {
            let d = devices(&p).await;
            d.iter().find(|x| x.id == headphones).filter(|x| (x.volume - 0.5).abs() < 0.01).map(|_| ())
        });

        // AC-MIX-07: the audio server stops; deskd reports it and refuses requests
        let wp = procs.0.remove(2);
        let mut pwd = procs.0.remove(1);
        drop(Procs(vec![wp]));
        let _ = pwd.kill();
        let _ = pwd.wait();
        wait_for!(changed, (!p.available().await.unwrap()).then_some(()));
        assert!(p.set_app_volume(APP, 0.5).await.is_err());
        assert!(devices(&p).await.is_empty());
    });
    let _ = done.send(());
    let _ = guard.join();
    drop(procs);
    drop(env);
}
