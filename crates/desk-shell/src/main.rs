// SPDX-License-Identifier: GPL-3.0-or-later
//! `desk-shell`: the DE-agnostic Wayland desktop shell of desktop-themes-rs (ADR 0012).
//!
//! A taskbar on every monitor (with the running windows, over wlr foreign toplevel management),
//! a Start/Search flyout, a quick settings flyout (toggles and the volume mixer) and a
//! clipboard history flyout, an emoji panel and an on-screen display drawn as `wlr-layer-shell` surfaces on labwc, niri, sway,
//! Hyprland, KWin or COSMIC, with `deskd` and `desk-search` behind them. Without layer-shell
//! (GNOME, `--windowed`) the same surfaces open as normal windows for previews and tests.
//!
//! One instance runs per session (GTK application `io.github.OlehDzhumyk.DeskShell`); a second
//! `desk-shell --toggle-start` (or `--toggle-quick-settings`, `--toggle-clipboard`,
//! `--toggle-emoji`), bound to a key in the compositor, only activates that action of the running
//! one over D-Bus (`org.gtk.Actions`), before it loads gtk4-layer-shell or the translations, so
//! the key answers at once. `--capture-region`, `--ocr-region` and `--pick-color` are one-shot
//! commands without GTK ([`oneshot`]); they report in the display of the running shell
//! (`show-osd`, also `desk-shell --show-osd TEXT`).
//!
//! The look follows the `skin` setting and deskd's `Appearance.Changed` live (no restart).

mod capture;
mod clipboard_flyout;
mod deskd;
mod emoji_flyout;
mod flyout;
mod launch;
mod layer_shell;
mod notifications;
mod oneshot;
mod osd;
mod pinned;
mod quick_settings;
mod region;
mod running;
mod sni;
mod start_menu;
mod style;
mod system;
mod taskbar;
mod toplevels;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use clap::{Parser, ValueEnum};
use desk_search::{Catalog, ParseOptions, SearchOptions, UsageStore, desktop};
use desk_settings::accessors::{DeskSettings, SearchSettings, Skin};
use desk_settings::i18n::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};

use clipboard_flyout::ClipboardFlyout;
use emoji_flyout::EmojiFlyout;
use osd::Osd;
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
const TOGGLE_EMOJI: &str = "toggle-emoji";
/// Show a message in the on-screen display (parameter: [`osd::encode`]).
pub const SHOW_OSD: &str = "show-osd";

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
#[command(group = clap::ArgGroup::new("oneshot").multiple(false))]
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
    /// Toggle the emoji panel of the running shell (start it if none runs)
    #[arg(long)]
    toggle_emoji: bool,
    /// Select a region, save it as a screenshot, copy it and open the capture editor (grim, slurp)
    #[arg(long, group = "oneshot")]
    capture_region: bool,
    /// Select a region and copy the text in it (grim, slurp, deskd's OCR)
    #[arg(long, group = "oneshot")]
    ocr_region: bool,
    /// Pick the colour of a pixel and copy it (grim, slurp -p, deskd's colour picker)
    #[arg(long, group = "oneshot")]
    pick_color: bool,
    /// Show TEXT in the on-screen display of the running shell (nothing when none runs)
    #[arg(long, value_name = "TEXT", group = "oneshot")]
    show_osd: Option<String>,
    /// The symbolic icon of --show-osd
    #[arg(long, value_name = "ICON", requires = "show_osd")]
    osd_icon: Option<String>,
}

impl Args {
    /// The actions the toggle flags ask for, in a fixed order.
    fn toggle_actions(&self) -> Vec<&'static str> {
        [
            (self.toggle_start, TOGGLE_START),
            (self.toggle_quick_settings, TOGGLE_QUICK_SETTINGS),
            (self.toggle_clipboard, TOGGLE_CLIPBOARD),
            (self.toggle_emoji, TOGGLE_EMOJI),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }

    /// The one-shot command the flags ask for (they exclude each other).
    fn oneshot(&self) -> Option<oneshot::Task> {
        [
            (self.capture_region, oneshot::Task::CaptureRegion),
            (self.ocr_region, oneshot::Task::OcrRegion),
            (self.pick_color, oneshot::Task::PickColor),
        ]
        .into_iter()
        .find_map(|(on, task)| on.then_some(task))
    }

    /// The `show-osd` parameter of `--show-osd` and `--osd-icon`.
    fn osd_param(&self) -> Option<String> {
        let text = self.show_osd.as_deref()?;
        Some(osd::encode(
            self.osd_icon.as_deref().unwrap_or(osd::DEFAULT_ICON),
            text,
        ))
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
    emoji: Rc<EmojiFlyout>,
    osd: Rc<Osd>,
    _look: Rc<LiveLook>,
    _user_css: Option<Rc<style::CssWatcher>>,
    _system: Rc<system::SystemStatus>,
    _sni: Rc<sni::SniWatcher>,
    _notifications: Rc<notifications::NotificationDaemon>,
    _taskbars: Rc<taskbar::Taskbars>,
}

impl Shell {
    fn toggle(&self, action: &str) {
        match action {
            TOGGLE_START => self.menu.toggle(),
            TOGGLE_QUICK_SETTINGS => self.quick.toggle(),
            TOGGLE_CLIPBOARD => self.clip.toggle(),
            TOGGLE_EMOJI => self.emoji.toggle(),
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
        // taskbars of monitors plugged in later get the look too
        let me = Rc::downgrade(&look);
        app.connect_window_added(move |_, w| {
            if let Some(l) = me.upgrade() {
                l.look().apply(w);
            }
        });
        let me = Rc::downgrade(&look);
        deskd::watch_appearance(move || {
            if let Some(l) = me.upgrade() {
                l.sync();
            }
        });
        look
    }

    fn look(&self) -> style::Look {
        style::current(self.skin.get(), self.deskd_dark.get())
    }

    /// Put the current look on every window of the shell.
    fn apply(&self) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let look = self.look();
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
    let user_css = style::install(&display);
    let system = system::SystemStatus::new();
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
    let quick = QuickSettings::new(app, &system);
    let clip = ClipboardFlyout::new(app);
    let emoji = EmojiFlyout::new(app);
    let e = Rc::downgrade(&emoji);
    clip.set_switch(move || {
        if let Some(e) = e.upgrade() {
            e.open();
        }
    });
    let c = Rc::downgrade(&clip);
    emoji.set_switch(move || {
        if let Some(c) = c.upgrade() {
            c.open();
        }
    });
    let q = Rc::downgrade(&quick);
    let osd = Osd::new(app, move || q.upgrade().is_some_and(|q| q.is_open()));
    let sni = sni::SniWatcher::new();
    let notifications = notifications::NotificationDaemon::new(app);
    let look = LiveLook::new(app, args.skin);
    let taskbars = taskbar::Taskbars::new(
        app,
        &menu,
        &quick,
        &clip,
        &sni,
        &notifications,
        &pins,
        &apps,
    );
    taskbars.start();
    look.apply();
    look.sync();
    Shell {
        menu,
        quick,
        clip,
        emoji,
        osd,
        _look: look,
        _user_css: user_css,
        _system: system,
        _sni: sni,
        _notifications: notifications,
        _taskbars: taskbars,
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

/// The `org.gtk.Actions.Activate` arguments of an action with an optional parameter.
fn activate_args(action: &str, param: Option<&glib::Variant>) -> glib::Variant {
    let params: Vec<glib::Variant> = param.map(|p| p.to_variant()).into_iter().collect();
    glib::Variant::tuple_from_iter([
        action.to_variant(),
        glib::Variant::array_from_iter_with_type(glib::VariantTy::VARIANT, params),
        glib::VariantDict::new(None).end(),
    ])
}

/// Activate parameterless `actions` in the running shell over D-Bus, without GTK or
/// gtk4-layer-shell. `false` when no shell runs (or it activated nothing): this process becomes
/// the shell.
fn remote_toggle(actions: &[&str]) -> bool {
    let actions: Vec<(&str, Option<glib::Variant>)> = actions.iter().map(|a| (*a, None)).collect();
    remote_activate(&actions)
}

/// Activate `actions` (with their parameter) in the running shell over D-Bus. `false` when no
/// shell runs or it activated nothing.
pub fn remote_activate(actions: &[(&str, Option<glib::Variant>)]) -> bool {
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
    for (action, param) in actions {
        match bus.call_sync(
            Some(APP_ID),
            APP_PATH,
            "org.gtk.Actions",
            "Activate",
            Some(&activate_args(action, param.as_ref())),
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
    // a message for the display of the running shell: never start a shell for it
    if let Some(param) = args.osd_param() {
        remote_activate(&[(SHOW_OSD, Some(param.to_variant()))]);
        return glib::ExitCode::SUCCESS;
    }
    // a one-shot command: no GTK, no window, only the translations of its messages
    if let Some(task) = args.oneshot() {
        desk_settings::i18n::init(desk_settings::i18n::language_from_gsettings);
        return oneshot::run(task);
    }
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
    for name in [
        TOGGLE_START,
        TOGGLE_QUICK_SETTINGS,
        TOGGLE_CLIPBOARD,
        TOGGLE_EMOJI,
    ] {
        let action = gio::SimpleAction::new(name, None);
        let shell = Rc::clone(&shell);
        action.connect_activate(move |_, _| {
            if let Some(s) = shell.borrow().as_ref() {
                s.toggle(name);
            }
        });
        app.add_action(&action);
    }
    let action = gio::SimpleAction::new(SHOW_OSD, Some(glib::VariantTy::STRING));
    let s = Rc::clone(&shell);
    action.connect_activate(move |_, param| {
        if let (Some(shell), Some(text)) = (s.borrow().as_ref(), param.and_then(|p| p.str())) {
            shell.osd.show_param(text);
        }
    });
    app.add_action(&action);
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
        let a = Args::try_parse_from(["desk-shell", "--toggle-emoji"]).unwrap();
        assert_eq!(a.toggle_actions(), [TOGGLE_EMOJI]);
        assert_eq!(a.oneshot(), None);
    }

    #[test]
    fn one_shot_flags_parse_and_exclude_each_other() {
        for (flag, task) in [
            ("--capture-region", oneshot::Task::CaptureRegion),
            ("--ocr-region", oneshot::Task::OcrRegion),
            ("--pick-color", oneshot::Task::PickColor),
        ] {
            let a = Args::try_parse_from(["desk-shell", flag]).unwrap();
            assert_eq!(a.oneshot(), Some(task), "{flag}");
            assert!(a.toggle_actions().is_empty() && a.osd_param().is_none());
        }
        assert!(Args::try_parse_from(["desk-shell", "--capture-region", "--pick-color"]).is_err());
        assert!(Args::try_parse_from(["desk-shell", "--ocr-region", "--show-osd", "x"]).is_err());
        let a = Args::try_parse_from(["desk-shell", "--show-osd", "Text copied"]).unwrap();
        assert_eq!(
            a.osd_param().as_deref(),
            Some("dialog-information-symbolic\tText copied")
        );
        assert_eq!(a.oneshot(), None);
        let a = Args::try_parse_from([
            "desk-shell",
            "--show-osd",
            "#FF0000",
            "--osd-icon",
            "color-select-symbolic",
        ])
        .unwrap();
        assert_eq!(
            a.osd_param().as_deref(),
            Some("color-select-symbolic\t#FF0000")
        );
        // the icon alone means nothing
        assert!(Args::try_parse_from(["desk-shell", "--osd-icon", "x"]).is_err());
    }

    #[test]
    fn remote_activation_has_the_org_gtk_actions_signature() {
        let v = activate_args(TOGGLE_START, None);
        assert_eq!(v.type_().as_str(), "(sava{sv})");
        assert_eq!(v.child_value(0).str(), Some("toggle-start"));
        assert_eq!(v.child_value(1).n_children(), 0);
        let v = activate_args(SHOW_OSD, Some(&"icon\ttext".to_variant()));
        assert_eq!(v.type_().as_str(), "(sava{sv})");
        let param = v.child_value(1).child_value(0).as_variant().unwrap();
        assert_eq!(param.str(), Some("icon\ttext"));
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
