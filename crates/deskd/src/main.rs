// SPDX-License-Identifier: GPL-3.0-or-later
//! deskd — служба функцій робочого столу на сесійній шині (`io.github.OlehDzhumyk.Desk`).
//!
//! Містить рушії функцій (поки що — історію буфера обміну); інтерфейс у GNOME Shell малює міст
//! `desk-bridge@desktop-themes-rs`, а логіка й стан живуть тут. Активується шиною (D-Bus-файл
//! служби вказує на юніт systemd --user `deskd.service`).

mod clipboard;
mod persist;
mod service;
mod settings;

use std::sync::{Arc, Mutex};

use desk_proto::{BUS_NAME, CLIPBOARD_PATH, CLIPBOARD_SCHEMA_ID, DESK_PATH};
use desk_settings::tr;
use gio::glib;
use gio::prelude::*;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use clipboard::{Config, Engine};
use service::{ClipboardIface, Desk, Shared};

fn usage() {
    println!(
        "{}",
        tr!(
            "deskd {version} — desktop features service (D-Bus {name})\n\nUsage: deskd [--replace]\n  --replace   take the bus name over from a running deskd\n  --version",
            version = env!("CARGO_PKG_VERSION"),
            name = BUS_NAME
        )
    );
}

/// Сповістити клієнтів (міст, GUI), що список або властивості змінилися.
fn emit_changed(conn: &zbus::Connection) {
    let conn = conn.clone();
    std::thread::spawn(move || {
        zbus::block_on(async {
            if let Ok(iface) = conn
                .object_server()
                .interface::<_, ClipboardIface>(CLIPBOARD_PATH)
                .await
            {
                let em = iface.signal_emitter();
                let _ = ClipboardIface::changed(em).await;
                let g = iface.get().await;
                let _ = g.private_changed(em).await;
                let _ = g.enabled_changed(em).await;
                let _ = g.count_changed(em).await;
            }
        });
    });
}

fn main() {
    desk_settings::i18n::init(desk_settings::i18n::language_from_gsettings);
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!("deskd {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let replace = args.iter().any(|a| a == "--replace");

    let gsettings = settings::clipboard_settings();
    let cfg = match &gsettings {
        Some(s) => settings::read_config(s),
        None => {
            eprintln!(
                "deskd: {}",
                tr!(
                    "the schema {schema} is not installed (data/install.sh), using the default settings",
                    schema = CLIPBOARD_SCHEMA_ID
                )
            );
            Config::default()
        }
    };
    let (data_dir, runtime_dir) = Engine::default_dirs();
    let mut eng = Engine::new(cfg, data_dir, runtime_dir);
    eng.load();
    let eng: Shared = Arc::new(Mutex::new(eng));

    let saver = persist::Saver::spawn(eng.clone(), persist::QUIET, persist::MAX_WAIT);
    let clip = ClipboardIface {
        eng: eng.clone(),
        saver: saver.clone(),
        set_private: settings::set_private_async,
    };
    let desk = Desk {
        features: vec!["clipboard".into()],
    };
    let conn = zbus::block_on(async {
        let mut flags = zbus::fdo::RequestNameFlags::ALLOW_REPLACEMENT;
        if replace {
            flags |= zbus::fdo::RequestNameFlags::REPLACE_EXISTING;
        }
        let conn = zbus::connection::Builder::session()
            .serve_at(DESK_PATH, desk)
            .serve_at(CLIPBOARD_PATH, clip)
            .build()
            .await?;
        let reply = conn
            .request_name_with_flags(BUS_NAME, flags | zbus::fdo::RequestNameFlags::DO_NOT_QUEUE)
            .await?;
        if matches!(reply, zbus::fdo::RequestNameReply::Exists) {
            return Err(zbus::Error::NameTaken);
        }
        Ok::<_, zbus::Error>(conn)
    });
    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "deskd: {}",
                tr!("cannot own {name}: {error}", name = BUS_NAME, error = e)
            );
            std::process::exit(1);
        }
    };
    eprintln!("deskd {}: running as {BUS_NAME}", env!("CARGO_PKG_VERSION"));

    let main_loop = glib::MainLoop::new(None, false);

    // інший deskd забрав назву (--replace) — виходимо
    {
        let conn = conn.clone();
        let ml = main_loop.clone();
        std::thread::spawn(move || {
            zbus::block_on(async {
                use futures_util::StreamExt;
                if let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await
                    && let Ok(mut lost) = proxy.receive_name_lost().await
                {
                    while let Some(sig) = lost.next().await {
                        if sig.args().map(|a| a.name == BUS_NAME).unwrap_or(false) {
                            eprintln!("deskd: another process took {BUS_NAME}, exiting");
                            ml.quit();
                            break;
                        }
                    }
                }
            });
        });
    }

    if let Some(s) = &gsettings {
        let eng = eng.clone();
        let conn = conn.clone();
        let typed = s.clone();
        s.0.connect_changed(None, move |_, key| {
            let cfg = settings::read_config(&typed);
            {
                let mut e = eng.lock().unwrap_or_else(|p| p.into_inner());
                e.set_config(cfg);
                if key != "private-mode" {
                    let _ = e.save();
                }
            }
            emit_changed(&conn);
        });
    }

    // logout / `systemctl --user stop`: leave the main loop so that the pending history is written
    match Signals::new([SIGHUP, SIGINT, SIGTERM]) {
        Ok(mut signals) => {
            let ml = main_loop.clone();
            std::thread::spawn(move || {
                if signals.forever().next().is_some() {
                    ml.quit();
                }
            });
        }
        Err(e) => {
            eprintln!("deskd: cannot watch SIGTERM, the history is saved on changes only: {e}")
        }
    }
    main_loop.run();
    saver.flush();
    drop(gsettings);
}
