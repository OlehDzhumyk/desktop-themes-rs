// SPDX-License-Identifier: GPL-3.0-or-later
//! deskd — служба функцій робочого столу на сесійній шині (`io.github.OlehDzhumyk.Desk`).
//!
//! Містить рушії функцій (історію буфера обміну, мікшер гучності); інтерфейс у GNOME Shell малює міст
//! `desk-bridge@desktop-themes-rs`, а логіка й стан живуть тут. Активується шиною (D-Bus-файл
//! служби вказує на юніт systemd --user `deskd.service`).

mod appearance;
mod clipboard;
mod emoji;
mod mixer;
mod panic_policy;
mod persist;
mod schedule;
mod service;
mod settings;
mod trust;

use std::sync::{Arc, Mutex, OnceLock};

use desk_proto::{
    APPEARANCE_PATH, BUS_NAME, CLIPBOARD_PATH, CLIPBOARD_SCHEMA_ID, DESK_PATH, EMOJI_PATH,
    MIXER_PATH,
};
use desk_settings::tr;
use gio::glib;
use gio::prelude::*;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use clipboard::{Config, Engine};
use mixer::Mixer;
use service::{ClipboardIface, Desk, Shared};

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

/// Engines this build hosts (the `Features` property).
fn features() -> Vec<String> {
    let mut f = vec![
        "clipboard".to_string(),
        "appearance".to_string(),
        "emoji".to_string(),
    ];
    if cfg!(feature = "pipewire") {
        f.push("mixer".into());
    }
    f
}

/// Connect the mixer to PipeWire (again). After a lost connection one retry follows two seconds
/// later; after that, the next client request tries (see [`Mixer::wanted`]).
#[cfg(feature = "pipewire")]
fn start_mixer(m: &Arc<Mixer>) {
    if m.backend_alive() {
        return;
    }
    let weak = Arc::downgrade(m);
    let backend = mixer::pw::PwBackend::spawn(m.graph.clone(), m.notifier.clone(), move || {
        glib::MainContext::default().invoke(move || {
            glib::timeout_add_seconds_local_once(2, move || {
                if let Some(m) = weak.upgrade()
                    && !mixer::lock(&m.graph).connected
                    && m.has_backend()
                {
                    start_mixer(&m);
                }
            });
        });
    });
    m.set_backend(Some(Box::new(backend)));
}

#[cfg(not(feature = "pipewire"))]
fn start_mixer(_m: &Arc<Mixer>) {}

#[allow(
    clippy::too_many_lines,
    reason = "one linear start-up sequence (bus, settings, engines, signal threads); the engine trait of the next stage restructures it"
)]
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

    let saver = {
        let eng = eng.clone();
        persist::Saver::spawn(
            move || {
                if let Err(e) = service::lock(&eng).save() {
                    eprintln!("deskd: cannot save the clipboard history: {e}");
                }
            },
            persist::QUIET,
            persist::MAX_WAIT,
        )
    };
    let gate = trust::Gate::new(Arc::new(trust::Trust::installed()));
    let clip = ClipboardIface {
        eng: eng.clone(),
        saver: saver.clone(),
        set_private: settings::set_private_async,
        gate: gate.clone(),
    };
    let desk = Desk {
        features: features(),
    };

    // the mixer: its graph, the coalesced Changed signal (sent once the bus connection exists)
    let mixer_settings = settings::mixer_settings();
    let (mixer_on, mixer_cfg) = mixer_settings
        .as_ref()
        .map_or((true, Default::default()), settings::read_mixer);
    let store = mixer::store::default_path();
    let mut graph = mixer::model::Graph::new(mixer_cfg, {
        let dirs = mixer::icons::data_dirs();
        Box::new(move |c: &[String]| mixer::icons::resolve(&dirs, c))
    });
    graph.memory = mixer::store::load(&store);
    let graph = Arc::new(Mutex::new(graph));
    let bus: Arc<OnceLock<zbus::Connection>> = Arc::new(OnceLock::new());
    let notifier = mixer::notify::Notifier::spawn(
        {
            let g = graph.clone();
            move |now| mixer::lock(&g).next_expiry(now)
        },
        {
            let bus = bus.clone();
            move |flags| {
                if let Some(conn) = bus.get() {
                    zbus::block_on(mixer::service::emit(conn, flags));
                }
            }
        },
    );
    let emoji_settings = settings::emoji_settings();
    let (emoji_remember, emoji_tone) = emoji_settings
        .as_ref()
        .map_or((true, "none".to_string()), settings::read_emoji);
    let emoji_eng = Arc::new(Mutex::new(emoji::EmojiEngine::new(
        emoji::recents::Recents::default_path(),
        emoji_remember,
        emoji_tone,
    )));
    let emoji_iface = emoji::service::EmojiIface {
        engine: emoji_eng.clone(),
        gate: gate.clone(),
    };
    let mixer = Arc::new(Mixer::new(graph, notifier, Some(store)));
    let mixer_iface = mixer::service::MixerIface {
        mixer: mixer.clone(),
        gate: gate.clone(),
    };

    // automatic light/dark appearance engine and its one-shot wake scheduler
    let appearance_settings = settings::appearance_settings();
    let interface_settings = settings::interface_settings();
    let appearance_cfg = appearance_settings
        .as_ref()
        .map_or_else(Default::default, settings::read_appearance);
    let initial_scheme = interface_settings.as_ref().map_or(
        appearance::solar::Scheme::Light,
        settings::read_color_scheme,
    );
    let appearance_eng: appearance::Shared =
        Arc::new(Mutex::new(appearance::AppearanceEngine::new(
            appearance_cfg,
            initial_scheme,
            settings::set_color_scheme_async,
            appearance::local_now_system,
            appearance::cities::detect_system_timezone,
        )));
    let appearance_scheduler = schedule::Scheduler::spawn("deskd-appearance", {
        let eng = appearance_eng.clone();
        let bus = bus.clone();
        move || {
            let (delay, changed) = appearance::lock(&eng).tick();
            if changed && let Some(conn) = bus.get() {
                zbus::block_on(appearance::service::emit_changed(conn));
            }
            delay
        }
    });
    let wallpaper_eng: appearance::wallpaper::Shared =
        Arc::new(Mutex::new(appearance::wallpaper::WallpaperEngine::new(
            appearance_settings
                .as_ref()
                .map_or_else(Default::default, settings::read_wallpaper),
            appearance::wallpaper::state_path(),
        )));
    let appearance_iface = appearance::service::AppearanceIface {
        eng: appearance_eng.clone(),
        scheduler: appearance_scheduler.clone(),
        wallpaper: wallpaper_eng.clone(),
        gate,
    };

    let conn = zbus::block_on(async {
        let conn = zbus::connection::Builder::session()
            .serve_at(DESK_PATH, desk)
            .serve_at(CLIPBOARD_PATH, clip)
            .serve_at(EMOJI_PATH, emoji_iface)
            .serve_at(MIXER_PATH, mixer_iface)
            .serve_at(APPEARANCE_PATH, appearance_iface)
            .build()
            .await?;
        service::own_name(&conn, replace).await?;
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

    let _ = bus.set(conn.clone());

    let main_loop = glib::MainLoop::new(None, false);

    if mixer_on {
        start_mixer(&mixer);
    }
    {
        // a client asked while disconnected: try again, at most every few seconds
        let weak = Arc::downgrade(&mixer);
        let last: Arc<Mutex<Option<std::time::Instant>>> = Arc::default();
        mixer.on_reconnect(move || {
            let now = std::time::Instant::now();
            let mut l = last.lock().unwrap_or_else(|p| p.into_inner());
            if l.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(5)) {
                return;
            }
            *l = Some(now);
            let weak = weak.clone();
            glib::MainContext::default().invoke(move || {
                if let Some(m) = weak.upgrade()
                    && m.has_backend()
                    && !mixer::lock(&m.graph).connected
                {
                    start_mixer(&m);
                }
            });
        });
    }
    if let Some(s) = &mixer_settings {
        let m = mixer.clone();
        let typed = s.clone();
        s.0.connect_changed(None, move |_, key| {
            let (on, cfg) = settings::read_mixer(&typed);
            match key {
                "enabled" if on => start_mixer(&m),
                "enabled" => m.set_backend(None),
                "remember-app-volume" | "hide-apps" => {
                    let forget = !cfg.remember;
                    mixer::lock(&m.graph).cfg = cfg;
                    if forget {
                        m.forget();
                    }
                    m.notifier.touch(mixer::model::dirty::APPS);
                }
                _ => {}
            }
        });
    }

    // probable secrets live in memory for a short time only (clipboard::SECRET_TTL)
    {
        let eng = eng.clone();
        let conn = conn.clone();
        glib::timeout_add_seconds_local(15, move || {
            if service::lock(&eng).expire(service::now()) {
                emit_changed(&conn);
            }
            glib::ControlFlow::Continue
        });
    }

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

    if let Some(s) = &emoji_settings {
        let eng = emoji_eng.clone();
        let typed = s.clone();
        s.0.connect_changed(None, move |_, key| {
            if matches!(key, "remember-recents" | "skin-tone") {
                let (remember, tone) = settings::read_emoji(&typed);
                emoji::service::lock(&eng).set_config(remember, tone);
            }
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

    {
        let sched = appearance_scheduler.clone();
        schedule::watch_system_events(move || sched.trigger());
    }
    // live accent from the wallpaper: GSettings signals, a 500 ms debounce, a decoding child
    let wallpaper_watch = {
        let conn = conn.clone();
        appearance::wallpaper::Watch::start(wallpaper_eng.clone(), move || {
            let conn = conn.clone();
            std::thread::spawn(move || {
                zbus::block_on(appearance::service::emit_changed(&conn));
            });
        })
        .map(std::rc::Rc::new)
    };
    if let Some(s) = &appearance_settings {
        let eng = appearance_eng.clone();
        let sched = appearance_scheduler.clone();
        let conn = conn.clone();
        let typed = s.clone();
        let watch = wallpaper_watch.clone();
        s.0.connect_changed(None, move |_, key| {
            if matches!(key, "accent-from-wallpaper" | "accent-source") {
                if let Some(w) = &watch {
                    w.settings_changed(settings::read_wallpaper(&typed));
                }
                return;
            }
            let cfg = settings::read_appearance(&typed);
            appearance::lock(&eng).set_config(cfg);
            sched.trigger();
            let conn = conn.clone();
            std::thread::spawn(move || {
                zbus::block_on(appearance::service::emit_changed(&conn));
            });
        });
    }
    if let Some(s) = &interface_settings {
        let eng = appearance_eng.clone();
        let conn = conn.clone();
        s.connect_changed(Some("color-scheme"), move |s, _| {
            let scheme = settings::read_color_scheme(s);
            if appearance::lock(&eng).on_external_scheme_change(scheme) {
                let conn = conn.clone();
                std::thread::spawn(move || {
                    zbus::block_on(appearance::service::emit_changed(&conn));
                });
            }
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
    appearance_scheduler.shutdown();
    saver.flush();
    mixer.set_backend(None);
    mixer.flush();
    drop(gsettings);
    drop(emoji_settings);
    drop(mixer_settings);
    drop(appearance_settings);
    drop(interface_settings);
    drop(wallpaper_watch);
}
