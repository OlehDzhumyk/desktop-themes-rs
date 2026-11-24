// SPDX-License-Identifier: GPL-3.0-or-later
//! What activating a search result or a button does, and starting programs.
//!
//! Applications start through their `gio::AppInfo` (the desktop entry with its `Exec`, D-Bus
//! activation and startup notification). [`Activation`] and [`action_command`] are pure and tested; the rest starts processes with the
//! environment of [`layer_shell::child_env_now`] (no `LD_PRELOAD` leaks into applications).

use desk_search::{ResultKind, SearchResult};
use gtk::prelude::*;
use gtk::{gdk, gio};

use crate::{deskd, layer_shell};

/// What a result does when activated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Activation {
    /// Start the application with this desktop id.
    Launch(String),
    /// Put the text on the clipboard (calculator answers, emoji).
    Copy(String),
    /// Run a built-in system action (`lock`, `suspend`, ...).
    Action(String),
    /// Open the default browser with a web search for the text.
    Web(String),
    /// Open the file at this absolute path in its default application.
    File(String),
    /// Put this clipboard history item (deskd) back on the clipboard.
    Clipboard(u64),
    /// Nothing this shell can do (a broken result).
    Ignore,
}

impl Activation {
    pub fn of(r: &SearchResult) -> Activation {
        let value = r.activate.clone();
        match r.kind {
            ResultKind::App => Activation::Launch(value),
            ResultKind::Calculator | ResultKind::Emoji => Activation::Copy(value),
            ResultKind::Action => Activation::Action(value),
            ResultKind::Web => Activation::Web(value),
            ResultKind::File if value.starts_with('/') => Activation::File(value),
            ResultKind::Clipboard => value
                .parse()
                .map_or(Activation::Ignore, Activation::Clipboard),
            ResultKind::File => Activation::Ignore,
        }
    }
}

/// How a system action is performed on a desktop without GNOME Shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionCommand {
    /// Run this program with these arguments (systemd-logind through `loginctl`/`systemctl`).
    Run(Vec<String>),
    /// Toggle the light/dark scheme through deskd.
    ToggleDarkMode,
    /// Toggle the private mode of the clipboard history through deskd.
    ToggleClipboardPrivate,
}

/// The command of a system action. `session` is `XDG_SESSION_ID` (log out needs it).
pub fn action_command(id: &str, session: Option<&str>) -> Option<ActionCommand> {
    let run = |args: &[&str]| {
        Some(ActionCommand::Run(
            args.iter().map(|a| (*a).to_owned()).collect(),
        ))
    };
    match id {
        "lock" => run(&["loginctl", "lock-session"]),
        "suspend" => run(&["systemctl", "suspend"]),
        "logout" => {
            let session = session.filter(|s| !s.is_empty())?;
            run(&["loginctl", "terminate-session", session])
        }
        "restart" => run(&["systemctl", "reboot"]),
        "power-off" => run(&["systemctl", "poweroff"]),
        "dark-mode" => Some(ActionCommand::ToggleDarkMode),
        "clipboard-private" => Some(ActionCommand::ToggleClipboardPrivate),
        _ => None,
    }
}

/// The search address for a web result (DuckDuckGo, no tracking parameters).
pub fn web_search_uri(query: &str) -> String {
    let encoded = gtk::glib::Uri::escape_string(query, None, false);
    format!("https://duckduckgo.com/?q={encoded}")
}

fn launch_context() -> Option<gio::AppLaunchContext> {
    let ctx = gdk::Display::default()?.app_launch_context();
    for (name, value) in layer_shell::child_env_now() {
        match value {
            Some(v) => ctx.setenv(name, v),
            None => ctx.unsetenv(name),
        }
    }
    Some(ctx.upcast())
}

/// Start the application with desktop id `id`. Returns `false` when it is not installed.
pub fn launch_app(id: &str) -> bool {
    let info = gio::AppInfo::all()
        .into_iter()
        .find(|a| a.id().is_some_and(|i| i == id));
    let Some(info) = info else {
        eprintln!("desk-shell: no application {id}");
        return false;
    };
    if let Err(e) = info.launch(&[], launch_context().as_ref()) {
        eprintln!("desk-shell: cannot start {id}: {e}");
        return false;
    }
    true
}

/// Open `uri` in its default handler.
pub fn open_uri(uri: &str) {
    if let Err(e) = gio::AppInfo::launch_default_for_uri(uri, launch_context().as_ref()) {
        eprintln!("desk-shell: cannot open {uri}: {e}");
    }
}

/// Start a program, detached (its exit is reaped by a GLib child watch).
pub fn spawn(argv: &[&str]) {
    let Some((program, args)) = argv.split_first() else {
        return;
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    for (name, value) in layer_shell::child_env_now() {
        match value {
            Some(v) => cmd.env(name, v),
            None => cmd.env_remove(name),
        };
    }
    match cmd.spawn() {
        Ok(child) => {
            let pid = gtk::glib::Pid(i32::try_from(child.id()).unwrap_or(0));
            gtk::glib::child_watch_add_local(pid, |_, _| {});
        }
        Err(e) => eprintln!("desk-shell: cannot start {program}: {e}"),
    }
}

/// The `themes` binary: next to this one when it exists (development builds), else from `PATH`.
pub fn themes_binary() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("themes")))
        .filter(|p| p.is_file())
        .map_or_else(|| "themes".to_owned(), |p| p.to_string_lossy().into_owned())
}

/// Open the settings hub (`themes gui`).
pub fn open_settings_hub() {
    spawn(&[&themes_binary(), "gui"]);
}

/// Run a system action by its id.
pub fn run_action(id: &str) {
    let session = std::env::var("XDG_SESSION_ID").ok();
    match action_command(id, session.as_deref()) {
        Some(ActionCommand::Run(argv)) => {
            spawn(&argv.iter().map(String::as_str).collect::<Vec<_>>());
        }
        Some(ActionCommand::ToggleDarkMode) => deskd::toggle_dark_mode(),
        Some(ActionCommand::ToggleClipboardPrivate) => deskd::toggle_clipboard_private(),
        None => eprintln!("desk-shell: unknown action {id}"),
    }
}

/// Put `text` on the clipboard.
pub fn copy_text(text: &str) {
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(text);
    }
}

/// Perform an activation; `result_id` is recorded in deskd's usage counts.
pub fn activate(a: &Activation, result_id: &str) {
    match a {
        Activation::Launch(id) => {
            if launch_app(id) {
                deskd::record_launch(result_id);
            }
        }
        Activation::Copy(text) => {
            copy_text(text);
            // emoji count for the ranking; calculator answers do not
            if result_id != "calc" {
                deskd::record_launch(result_id);
            }
        }
        Activation::Action(id) => {
            run_action(id);
            deskd::record_launch(result_id);
        }
        Activation::Web(query) => open_uri(&web_search_uri(query)),
        Activation::File(path) => {
            open_uri(&gio::File::for_path(path).uri());
            deskd::record_launch(result_id);
        }
        Activation::Clipboard(id) => {
            deskd::clipboard_activate(*id, || {});
            deskd::record_launch(result_id);
        }
        Activation::Ignore => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(kind: ResultKind, activate: &str) -> SearchResult {
        SearchResult {
            id: "x".into(),
            kind,
            title: String::new(),
            subtitle: String::new(),
            icon: String::new(),
            activate: activate.into(),
            score: 0,
        }
    }

    #[test]
    fn results_map_to_activations() {
        let cases = [
            (ResultKind::App, Activation::Launch("a.desktop".into())),
            (ResultKind::Calculator, Activation::Copy("a.desktop".into())),
            (ResultKind::Emoji, Activation::Copy("a.desktop".into())),
            (ResultKind::Action, Activation::Action("a.desktop".into())),
            (ResultKind::Web, Activation::Web("a.desktop".into())),
            (ResultKind::File, Activation::Ignore),
            (ResultKind::Clipboard, Activation::Ignore),
        ];
        for (kind, want) in cases {
            assert_eq!(Activation::of(&result(kind, "a.desktop")), want, "{kind:?}");
        }
        // deskd's file and clipboard results: an absolute path, a decimal item id
        assert_eq!(
            Activation::of(&result(ResultKind::File, "/home/user/a b.txt")),
            Activation::File("/home/user/a b.txt".into())
        );
        assert_eq!(
            Activation::of(&result(ResultKind::Clipboard, "42")),
            Activation::Clipboard(42)
        );
    }

    #[test]
    fn every_built_in_action_has_a_command() {
        for a in desk_search::SYSTEM_ACTIONS {
            assert!(action_command(a.id, Some("3")).is_some(), "{}", a.id);
        }
        assert_eq!(
            action_command("logout", Some("c2")),
            Some(ActionCommand::Run(vec![
                "loginctl".into(),
                "terminate-session".into(),
                "c2".into()
            ]))
        );
        // without a session id log out does nothing rather than end every session
        assert_eq!(action_command("logout", None), None);
        assert_eq!(action_command("logout", Some("")), None);
        assert_eq!(action_command("rm -rf", None), None);
    }

    #[test]
    fn web_queries_are_escaped() {
        assert_eq!(
            web_search_uri("a b&c=д"),
            "https://duckduckgo.com/?q=a%20b%26c%3D%D0%B4"
        );
    }
}
