// SPDX-License-Identifier: GPL-3.0-or-later
//! Програма «Теми» (GTK 4 + libadwaita).

mod a11y;
mod card;
mod credits;
mod features;
mod preview;
mod window;

use std::sync::atomic::{AtomicBool, Ordering};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use themes_core::APP_ID;

#[derive(Clone, Debug)]
pub struct GuiOptions {
    pub screenshot: Option<String>,
    pub color_scheme: Option<String>,
    pub size: (i32, i32),
    pub page: Option<String>,
}

impl Default for GuiOptions {
    fn default() -> Self {
        GuiOptions {
            screenshot: None,
            color_scheme: None,
            size: (1180, 780),
            page: None,
        }
    }
}

const CSS: &str = r#"
.theme-card { border-radius: 14px; padding: 0; }
.theme-card .preview { border-radius: 14px 14px 0 0; background: alpha(currentColor, 0.08); }
.theme-card.active { box-shadow: 0 0 0 3px var(--accent-bg-color); }
.theme-card .info { padding: 10px 14px 12px 14px; }
.badge { background: var(--accent-bg-color); color: var(--accent-fg-color);
         border-radius: 999px; padding: 2px 10px; font-weight: bold; font-size: 0.85em; }
.badge.partial { background: alpha(black, 0.55); color: white; }
.badge.experimental { background: var(--warning-bg-color); color: var(--warning-fg-color); }
.badge.origin { background: alpha(currentColor, 0.12); color: inherit; font-weight: normal; }
.badges { margin: 10px; }
@media (prefers-contrast: more) {
  .theme-card.active { box-shadow: 0 0 0 4px currentColor; }
  .badge, .badge.partial, .badge.experimental { background: var(--warning-bg-color); color: var(--warning-fg-color); }
.badge.origin { background: none; color: inherit; border: 2px solid currentColor; }
  .busy-veil { background: alpha(black, 0.6); }
}
.busy-veil { background: alpha(black, 0.35); border-radius: 14px 14px 0 0; }
.module-chip { font-size: 0.85em; }
flowboxchild { border-radius: 14px; padding: 0; }
"#;

/// Перезапустити вікно після виходу з циклу (дія `app.restart`, напр. після зміни мови).
pub static RESTART: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------- один екземпляр
// GApplication не може зайняти io.github.OlehDzhumyk.Themes (цю назву має D-Bus-служба),
// тому вікно працює як NON_UNIQUE, а єдиність забезпечує окрема назва на шині.
const WINDOW_NAME: &str = "io.github.OlehDzhumyk.Themes.Window";

struct WindowIface;

#[zbus::interface(name = "io.github.OlehDzhumyk.Themes.Window")]
impl WindowIface {
    fn present(&self) {
        glib::MainContext::default().invoke(|| {
            if let Some(app) = gio::Application::default() {
                app.activate();
            }
        });
    }
}

fn claim_single_instance() -> Result<Option<zbus::Connection>, ()> {
    let conn = match zbus::block_on(zbus::Connection::session()) {
        Ok(c) => c,
        Err(_) => return Ok(None), // без шини — просто працюємо
    };
    let flags = zbus::fdo::RequestNameFlags::DO_NOT_QUEUE;
    let got = zbus::block_on(async {
        let _ = conn
            .object_server()
            .at("/io/github/OlehDzhumyk/Themes/Window", WindowIface)
            .await;
        conn.request_name_with_flags(WINDOW_NAME, flags).await
    });
    match got {
        Ok(
            zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner,
        ) => Ok(Some(conn)),
        _ => {
            let _ = zbus::block_on(conn.call_method(
                Some(WINDOW_NAME),
                "/io/github/OlehDzhumyk/Themes/Window",
                Some("io.github.OlehDzhumyk.Themes.Window"),
                "Present",
                &(),
            ));
            Err(())
        }
    }
}

pub fn run(opts: GuiOptions) -> i32 {
    let _guard = if opts.screenshot.is_none() {
        match claim_single_instance() {
            Ok(c) => c,
            Err(()) => return 0, // уже відкрито — показали наявне вікно
        }
    } else {
        None
    };
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let exit = std::rc::Rc::new(std::cell::Cell::new(0));
    {
        let opts = opts.clone();
        app.connect_startup(move |app| {
            let provider = gtk::CssProvider::new();
            provider.load_from_string(CSS);
            gtk::style_context_add_provider_for_display(
                &gdk::Display::default().expect("display"),
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            let sm = adw::StyleManager::default();
            match opts.color_scheme.as_deref() {
                Some("dark") => sm.set_color_scheme(adw::ColorScheme::ForceDark),
                Some("light") => sm.set_color_scheme(adw::ColorScheme::ForceLight),
                _ => {}
            }
            window::add_app_actions(app);
        });
    }
    {
        let exit = exit.clone();
        app.connect_activate(move |app| {
            if let Some(w) = app.active_window() {
                w.present();
                return;
            }
            window::ThemesWindow::build(app, opts.clone(), exit.clone());
        });
    }
    let argv0 = std::env::args().next().unwrap_or_else(|| "themes".into());
    let code = app.run_with_args(&[argv0]);
    if RESTART.load(Ordering::SeqCst) {
        restart(_guard.as_ref());
    }
    if exit.get() != 0 {
        exit.get()
    } else {
        i32::from(code)
    }
}

/// Замінити процес новим вікном (мова та інші налаштування читаються заново).
fn restart(conn: Option<&zbus::Connection>) {
    use std::os::unix::process::CommandExt;
    if let Some(c) = conn {
        let _ = zbus::block_on(c.release_name(WINDOW_NAME));
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    desk_settings::i18n::restore_child_env(&mut cmd);
    let err = cmd.arg("gui").exec();
    eprintln!("themes: restart failed: {err}");
}
