// SPDX-License-Identifier: GPL-3.0-or-later
//! `desk-shell`: the DE-agnostic Wayland desktop shell of desktop-themes-rs (ADR 0012).
//!
//! A taskbar and a Start/Search flyout drawn as `wlr-layer-shell` surfaces on labwc, niri, sway,
//! Hyprland, KWin or COSMIC, with `deskd` and `desk-search` behind them. Without layer-shell
//! (GNOME, `--windowed`) the same surfaces open as normal windows for previews and tests.
//!
//! One instance runs per session (GTK application `io.github.OlehDzhumyk.DeskShell`); a second
//! `desk-shell --toggle-start`, bound to a key in the compositor, only toggles the flyout of the
//! running one over D-Bus.

mod capture;
mod deskd;
mod launch;
mod layer_shell;
mod pinned;
mod start_menu;
mod style;
mod taskbar;

use std::cell::RefCell;
use std::rc::Rc;

use clap::{Parser, ValueEnum};
use desk_search::{Catalog, ParseOptions, SearchOptions, UsageStore, desktop};
use desk_settings::accessors::{DeskSettings, SearchSettings, Skin};
use desk_settings::i18n::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};

use start_menu::StartMenu;

/// The GTK application id (single instance over D-Bus).
pub const APP_ID: &str = "io.github.OlehDzhumyk.DeskShell";

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum SkinArg {
    Windows11,
    Macos,
    Default,
}

impl From<SkinArg> for Skin {
    fn from(s: SkinArg) -> Skin {
        match s {
            SkinArg::Windows11 => Skin::Windows11,
            SkinArg::Macos => Skin::Macos,
            SkinArg::Default => Skin::Default,
        }
    }
}

/// desk-shell: taskbar and Start/Search flyout for Wayland compositors with wlr-layer-shell
#[derive(Clone, Debug, Parser, PartialEq, Eq)]
#[command(name = "desk-shell", version)]
struct Args {
    /// Skin (default: the `skin` setting, else windows11)
    #[arg(long, value_enum)]
    skin: Option<SkinArg>,
    /// Open normal windows instead of layer-shell surfaces (previews, GNOME)
    #[arg(long)]
    windowed: bool,
    /// Toggle the Start/Search flyout of the running shell (start it if none runs)
    #[arg(long)]
    toggle_start: bool,
}

/// The skin to draw: the flag, else the setting, else Windows 11 (the first supported one).
fn skin_for(flag: Option<SkinArg>, setting: Option<Skin>) -> Skin {
    match (flag, setting) {
        (Some(s), _) => s.into(),
        (None, Some(Skin::Default) | None) => Skin::Windows11,
        (None, Some(s)) => s,
    }
}

/// The running shell of the primary instance.
struct Shell {
    menu: Rc<StartMenu>,
}

fn default_browser() -> Option<String> {
    gio::AppInfo::default_for_uri_scheme("https")
        .and_then(|a| a.id())
        .map(|id| id.to_string())
}

fn build(app: &gtk::Application, args: &Args) -> Shell {
    let display = gtk::gdk::Display::default().expect("a display");
    style::install(&display);
    let look = style::current(skin_for(args.skin, DeskSettings::new().map(|s| s.skin())));
    let apps = desktop::scan(
        &desktop::application_dirs(&desktop::data_dirs()),
        &ParseOptions::from_env(),
    );
    let browser = default_browser();
    let grid = pinned::start_grid(&apps, browser.as_deref());
    let pins = pinned::taskbar(&apps, browser.as_deref());
    let search = SearchSettings::new();
    let defaults = SearchOptions::default();
    let opts = SearchOptions {
        web_search: search.as_ref().is_some_and(SearchSettings::web_search),
        max_results: search
            .as_ref()
            .and_then(|s| usize::try_from(s.max_results()).ok())
            .unwrap_or(defaults.max_results),
        web_title: gettext("Search the web for “{query}”"),
    };
    // read-only: deskd owns search-usage.json and counts launches (RecordLaunch)
    let usage = UsageStore::load(UsageStore::default_path(), true);
    let menu = StartMenu::new(app, Catalog::new(apps.clone(), gettext), usage, opts, &grid);
    let bar = taskbar::build(app, &menu, &pins);
    for w in app.windows() {
        look.apply(&w);
    }
    // follow the system's dark and contrast preferences
    if let Some(settings) = gtk::Settings::default() {
        let app = app.downgrade();
        let skin = look.skin;
        let restyle = move |_: &gtk::Settings| {
            if let Some(app) = app.upgrade() {
                let look = style::current(skin);
                for w in app.windows() {
                    look.apply(&w);
                }
            }
        };
        settings.connect_gtk_interface_color_scheme_notify(restyle.clone());
        settings.connect_gtk_interface_contrast_notify(restyle);
    }
    bar.present();
    Shell { menu }
}

fn command_line(
    app: &gtk::Application,
    cmd: &gio::ApplicationCommandLine,
    shell: &RefCell<Option<Shell>>,
) -> glib::ExitCode {
    let args = match Args::try_parse_from(cmd.arguments()) {
        Ok(a) => a,
        Err(e) => {
            // the calling process parsed the same arguments already, so this is rare
            eprintln!("desk-shell: {e}");
            return glib::ExitCode::FAILURE;
        }
    };
    let mut shell = shell.borrow_mut();
    let first = shell.is_none();
    let running = shell.get_or_insert_with(|| build(app, &args));
    if args.toggle_start {
        running.menu.toggle();
    } else if !first {
        eprintln!("desk-shell: already running");
    }
    glib::ExitCode::SUCCESS
}

fn main() -> glib::ExitCode {
    // parsed here too, so --help and bad flags answer before anything starts
    let args = Args::parse();
    if args.windowed {
        layer_shell::force_windowed();
    } else {
        // first, before any thread: may re-execute with gtk4-layer-shell preloaded
        layer_shell::ensure_preloaded();
    }
    desk_settings::i18n::init(desk_settings::i18n::language_from_gsettings);
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let shell: Rc<RefCell<Option<Shell>>> = Rc::default();
    app.connect_command_line(move |app, cmd| command_line(app, cmd, &shell));
    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_parse() {
        let a = Args::try_parse_from(["desk-shell", "--skin", "macos", "--windowed"]).unwrap();
        assert_eq!(a.skin, Some(SkinArg::Macos));
        assert!(a.windowed && !a.toggle_start);
        let a = Args::try_parse_from(["desk-shell", "--toggle-start"]).unwrap();
        assert!(a.toggle_start);
        assert!(Args::try_parse_from(["desk-shell", "--skin", "aqua"]).is_err());
    }

    #[test]
    fn the_skin_comes_from_the_flag_then_the_setting() {
        assert_eq!(
            skin_for(Some(SkinArg::Macos), Some(Skin::Windows11)),
            Skin::Macos
        );
        assert_eq!(skin_for(None, Some(Skin::Macos)), Skin::Macos);
        // the plain GNOME skin and a missing schema both give Windows 11 here
        assert_eq!(skin_for(None, Some(Skin::Default)), Skin::Windows11);
        assert_eq!(skin_for(None, None), Skin::Windows11);
        assert_eq!(skin_for(Some(SkinArg::Default), None), Skin::Default);
    }
}
