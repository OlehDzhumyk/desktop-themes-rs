// SPDX-License-Identifier: GPL-3.0-or-later
//! `desk-shell`: the DE-agnostic Wayland desktop shell of desktop-themes-rs (ADR 0012).
//!
//! A taskbar, a Start/Search flyout, a quick settings flyout (toggles and the volume mixer) and a
//! clipboard history flyout drawn as `wlr-layer-shell` surfaces on labwc, niri, sway,
//! Hyprland, KWin or COSMIC, with `deskd` and `desk-search` behind them. Without layer-shell
//! (GNOME, `--windowed`) the same surfaces open as normal windows for previews and tests.
//!
//! One instance runs per session (GTK application `io.github.OlehDzhumyk.DeskShell`); a second
//! `desk-shell --toggle-start` (or `--toggle-quick-settings`, `--toggle-clipboard`), bound to a
//! key in the compositor, only activates that action of the running one over D-Bus
//! (`org.gtk.Actions`), before it loads gtk4-layer-shell or the translations, so the key answers
//! at once.
//!
//! The look follows the `skin` setting and deskd's `Appearance.Changed` live (no restart).

mod capture;
mod clipboard_flyout;
mod deskd;
mod flyout;
mod launch;
mod layer_shell;
mod pinned;
mod quick_settings;
mod start_menu;
mod style;
mod taskbar;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use clap::{Parser, ValueEnum};
use desk_search::{Catalog, ParseOptions, SearchOptions, UsageStore, desktop};
use desk_settings::accessors::{DeskSettings, SearchSettings, Skin};
use desk_settings::i18n::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};

use clipboard_flyout::ClipboardFlyout;
use quick_settings::QuickSettings;
use start_menu::StartMenu;

/// The GTK application id (single instance over D-Bus).
pub const APP_ID: &str = "io.github.OlehDzhumyk.DeskShell";
/// The object path GApplication exports its actions on (`APP_ID` as a path).
const APP_PATH: &str = "/io/github/OlehDzhumyk/DeskShell";
/// The application actions a second instance activates.
const TOGGLE_START: &str = "toggle-start";
const TOGGLE_QUICK_SETTINGS: &str = "toggle-quick-settings";
const TOGGLE_CLIPBOARD: &str = "toggle-clipboard";

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
    /// Toggle the quick settings flyout of the running shell (start it if none runs)
    #[arg(long)]
    toggle_quick_settings: bool,
    /// Toggle the clipboard history flyout of the running shell (start it if none runs)
    #[arg(long)]
    toggle_clipboard: bool,
}

impl Args {
    /// The actions the toggle flags ask for, in a fixed order.
    fn toggle_actions(&self) -> Vec<&'static str> {
        [
            (self.toggle_start, TOGGLE_START),
            (self.toggle_quick_settings, TOGGLE_QUICK_SETTINGS),
            (self.toggle_clipboard, TOGGLE_CLIPBOARD),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }
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
    quick: Rc<QuickSettings>,
    clip: Rc<ClipboardFlyout>,
    _look: Rc<LiveLook>,
}

impl Shell {
    fn toggle(&self, action: &str) {
        match action {
            TOGGLE_START => self.menu.toggle(),
            TOGGLE_QUICK_SETTINGS => self.quick.toggle(),
            TOGGLE_CLIPBOARD => self.clip.toggle(),
            _ => {}
        }
    }
}

/// The skin and scheme of every shell window, kept current: the `skin` setting (unless
/// `--skin` is given), deskd's `Appearance.Changed`, and GTK's scheme and contrast.
struct LiveLook {
    app: glib::WeakRef<gtk::Application>,
    flag: Option<SkinArg>,
    skin: Cell<Skin>,
    /// The scheme deskd reported last (`None`: deskd is not running).
    deskd_dark: Cell<Option<bool>>,
    /// Kept alive for its `changed::skin` handler.
    settings: Option<DeskSettings>,
}

impl LiveLook {
    fn new(app: &gtk::Application, flag: Option<SkinArg>) -> Rc<LiveLook> {
        let settings = DeskSettings::new();
        let look = Rc::new(LiveLook {
            app: app.downgrade(),
            flag,
            skin: Cell::new(skin_for(flag, settings.as_ref().map(DeskSettings::skin))),
            deskd_dark: Cell::new(None),
            settings,
        });
        if let Some(s) = &look.settings {
            let me = Rc::downgrade(&look);
            s.0.connect_changed(Some(DeskSettings::SKIN), move |_, _| {
                if let Some(l) = me.upgrade() {
                    let setting = l.settings.as_ref().map(DeskSettings::skin);
                    l.skin.set(skin_for(l.flag, setting));
                    l.apply();
                }
            });
        }
        if let Some(gtk_settings) = gtk::Settings::default() {
            let me = Rc::downgrade(&look);
            let sync = move |_: &gtk::Settings| {
                if let Some(l) = me.upgrade() {
                    l.sync();
                }
            };
            gtk_settings.connect_gtk_interface_color_scheme_notify(sync.clone());
            gtk_settings.connect_gtk_interface_contrast_notify(sync);
        }
        let me = Rc::downgrade(&look);
        deskd::watch_appearance(move || {
            if let Some(l) = me.upgrade() {
                l.sync();
            }
        });
        look
    }

    /// Put the current look on every window of the shell.
    fn apply(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let look = style::current(self.skin.get(), self.deskd_dark.get());
        for w in app.windows() {
            look.apply(&w);
        }
    }

    /// Ask deskd for the scheme, then apply.
    fn sync(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        deskd::dark_mode(move |dark| {
            if let Some(l) = me.upgrade() {
                l.deskd_dark.set(dark);
                l.apply();
            }
        });
    }
}

fn default_browser() -> Option<String> {
    gio::AppInfo::default_for_uri_scheme("https")
        .and_then(|a| a.id())
        .map(|id| id.to_string())
}

fn build(app: &gtk::Application, args: &Args) -> Shell {
    let display = gtk::gdk::Display::default().expect("a display");
    style::install(&display);
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
    let quick = QuickSettings::new(app);
    let clip = ClipboardFlyout::new(app);
    let bar = taskbar::build(app, &menu, &quick, &clip, &pins);
    let look = LiveLook::new(app, args.skin);
    look.apply();
    look.sync();
    bar.present();
    Shell {
        menu,
        quick,
        clip,
        _look: look,
    }
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
    let actions = args.toggle_actions();
    for action in &actions {
        running.toggle(action);
    }
    if !first && actions.is_empty() {
        eprintln!("desk-shell: already running");
    }
    glib::ExitCode::SUCCESS
}

/// The `org.gtk.Actions.Activate` arguments of a parameterless action.
fn activate_args(action: &str) -> glib::Variant {
    glib::Variant::tuple_from_iter([
        action.to_variant(),
        Vec::<glib::Variant>::new().to_variant(),
        glib::VariantDict::new(None).end(),
    ])
}

/// Activate `actions` in the running shell over D-Bus, without GTK or gtk4-layer-shell.
/// `false` when no shell runs (or it activated nothing): this process becomes the shell.
fn remote_toggle(actions: &[&str]) -> bool {
    if actions.is_empty() {
        return false;
    }
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    let owned = bus
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(APP_ID,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|v| v.get::<(bool,)>())
        .is_some_and(|(b,)| b);
    if !owned {
        return false;
    }
    let mut done = 0;
    for action in actions {
        match bus.call_sync(
            Some(APP_ID),
            APP_PATH,
            "org.gtk.Actions",
            "Activate",
            Some(&activate_args(action)),
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
        ) {
            Ok(_) => done += 1,
            Err(e) => eprintln!("desk-shell: {action} in the running shell: {e}"),
        }
    }
    done > 0
}

fn main() -> glib::ExitCode {
    // parsed here too, so --help and bad flags answer before anything starts
    let args = Args::parse();
    // a key binding: hand the toggle to the running shell and exit, before the slow start-up
    if remote_toggle(&args.toggle_actions()) {
        return glib::ExitCode::SUCCESS;
    }
    if args.windowed {
        layer_shell::force_windowed();
    } else {
        // before GTK: may re-execute with gtk4-layer-shell preloaded
        layer_shell::ensure_preloaded();
    }
    desk_settings::i18n::init(desk_settings::i18n::language_from_gsettings);
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let shell: Rc<RefCell<Option<Shell>>> = Rc::default();
    for name in [TOGGLE_START, TOGGLE_QUICK_SETTINGS, TOGGLE_CLIPBOARD] {
        let action = gio::SimpleAction::new(name, None);
        let shell = Rc::clone(&shell);
        action.connect_activate(move |_, _| {
            if let Some(s) = shell.borrow().as_ref() {
                s.toggle(name);
            }
        });
        app.add_action(&action);
    }
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
        assert!(a.toggle_start && !a.toggle_quick_settings && !a.toggle_clipboard);
        let a = Args::try_parse_from(["desk-shell", "--toggle-quick-settings"]).unwrap();
        assert!(a.toggle_quick_settings && !a.toggle_start);
        let a = Args::try_parse_from(["desk-shell", "--toggle-clipboard"]).unwrap();
        assert!(a.toggle_clipboard && !a.toggle_start);
        assert_eq!(a.toggle_actions(), [TOGGLE_CLIPBOARD]);
        let a =
            Args::try_parse_from(["desk-shell", "--toggle-clipboard", "--toggle-start"]).unwrap();
        assert_eq!(a.toggle_actions(), [TOGGLE_START, TOGGLE_CLIPBOARD]);
        assert!(
            Args::try_parse_from(["desk-shell"])
                .unwrap()
                .toggle_actions()
                .is_empty()
        );
        assert!(Args::try_parse_from(["desk-shell", "--skin", "aqua"]).is_err());
    }

    #[test]
    fn remote_activation_has_the_org_gtk_actions_signature() {
        let v = activate_args(TOGGLE_START);
        assert_eq!(v.type_().as_str(), "(sava{sv})");
        assert_eq!(v.child_value(0).str(), Some("toggle-start"));
        // GApplication exports its actions on the application id as a path
        assert_eq!(format!("/{}", APP_ID.replace('.', "/")), APP_PATH);
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
