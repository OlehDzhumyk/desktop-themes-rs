// SPDX-License-Identifier: GPL-3.0-or-later
//! deskd — служба функцій робочого столу на сесійній шині (`io.github.OlehDzhumyk.Desk`).
//!
//! Містить рушії функцій (історію буфера обміну, мікшер гучності); інтерфейс у GNOME Shell малює міст
//! `desk-bridge@desktop-themes-rs`, а логіка й стан живуть тут. Активується шиною (D-Bus-файл
//! служби вказує на юніт systemd --user `deskd.service`).

mod appearance;
mod capture;
mod clipboard;
mod color_picker;
mod emoji;
mod engine;
mod focus;
mod layouts;
mod mixer;
mod ocr;
mod panic_policy;
mod persist;
mod schedule;
mod service;
mod settings;
mod trust;

use std::sync::Arc;

use desk_proto::{BUS_NAME, DESK_PATH};
use desk_settings::tr;
use gio::glib;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use engine::appearance::AppearanceFeature;
use engine::capture::CaptureFeature;
use engine::clipboard::ClipboardFeature;
use engine::color_picker::ColorPickerFeature;
use engine::emoji::EmojiFeature;
use engine::focus::FocusFeature;
use engine::layouts::LayoutsEngine;
use engine::mixer::MixerFeature;
use engine::ocr::OcrEngine;
use engine::{Engines, Setup};
use service::Desk;

fn usage() {
    println!(
        "{}",
        tr!(
            "deskd {version} — desktop features service (D-Bus {name})\n\nUsage: deskd [--replace]\n  --replace   development: take the bus name over from a deskd started with --replace, and let the next one take it\n  --version",
            version = env!("CARGO_PKG_VERSION"),
            name = BUS_NAME
        )
    );
}

/// Engines this build hosts (the `Features` property).
fn features() -> Vec<String> {
    let mut f = vec![
        "clipboard".to_string(),
        "appearance".to_string(),
        "emoji".to_string(),
        "color-picker".to_string(),
        "ocr".to_string(),
        "capture".to_string(),
        "layouts".to_string(),
    ];
    f.push("focus".into());
    if cfg!(feature = "pipewire") {
        f.push("mixer".into());
    }
    f
}

/// Make the engines, in start order. Shutdown runs in reverse: the schedulers (appearance, focus)
/// stop first, then the clipboard history and the mixer memory are written.
fn engines(setup: &Setup) -> Engines {
    Engines::new(vec![
        Box::new(MixerFeature::new(setup)),
        Box::new(EmojiFeature::new(setup)),
        Box::new(ColorPickerFeature::new(setup)),
        Box::new(OcrEngine::new(setup)),
        Box::new(CaptureFeature::new(setup)),
        Box::new(LayoutsEngine::new(setup)),
        Box::new(ClipboardFeature::new(setup)),
        Box::new(FocusFeature::new(setup)),
        Box::new(AppearanceFeature::new(setup)),
    ])
}

/// Serve all interfaces on the session bus and own [`BUS_NAME`]; exit when that fails.
fn connect(engines: &mut Engines, replace: bool) -> zbus::Connection {
    let builder = zbus::connection::Builder::session().serve_at(
        DESK_PATH,
        Desk {
            features: features(),
        },
    );
    let builder = engines.register(builder);
    let conn = zbus::block_on(async {
        let conn = builder.build().await?;
        service::own_name(&conn, replace).await?;
        Ok::<_, zbus::Error>(conn)
    });
    match conn {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "deskd: {}",
                tr!("cannot own {name}: {error}", name = BUS_NAME, error = e)
            );
            std::process::exit(1);
        }
    }
}

/// інший deskd забрав назву (--replace) — виходимо
fn quit_on_name_lost(conn: &zbus::Connection, main_loop: &glib::MainLoop) {
    let conn = conn.clone();
    let ml = main_loop.clone();
    std::thread::spawn(move || {
        zbus::block_on(async {
            use futures_util::StreamExt;
            if let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await
                && let Ok(mut lost) = proxy.receive_name_lost().await
            {
                while let Some(sig) = lost.next().await {
                    if sig.args().is_ok_and(|a| a.name == BUS_NAME) {
                        eprintln!("deskd: another process took {BUS_NAME}, exiting");
                        ml.quit();
                        break;
                    }
                }
            }
        });
    });
}

/// logout / `systemctl --user stop`: leave the main loop so that the pending history is written
fn quit_on_signals(main_loop: &glib::MainLoop) {
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

    let setup = Setup {
        bus: Arc::default(),
        gate: trust::Gate::new(Arc::new(trust::Trust::installed())),
    };
    let mut engines = engines(&setup);
    let conn = connect(&mut engines, replace);
    eprintln!("deskd {}: running as {BUS_NAME}", env!("CARGO_PKG_VERSION"));
    let _ = setup.bus.set(conn.clone());

    let main_loop = glib::MainLoop::new(None, false);
    engines.start(&conn).watch();
    quit_on_name_lost(&conn, &main_loop);
    quit_on_signals(&main_loop);
    main_loop.run();
    engines.shutdown();
}
