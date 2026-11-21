// SPDX-License-Identifier: GPL-3.0-or-later
//! themes — менеджер тем робочого столу GNOME (рівень користувача).
//!
//! Бібліотека містить усю логіку; CLI, GUI і D-Bus-служба лише викликають її.

pub mod apply;
pub mod dconf;
pub mod doctor;
pub mod error;
pub mod flatpak;
pub mod gtklinks;
pub mod hooks;
pub mod lock;
pub mod modules;
pub mod paths;
pub mod plan;
pub mod shell;
pub mod snapshot;
pub mod state;
pub mod theme;
pub mod util;

pub use error::{Error, Result};
pub use modules::Module;
pub use paths::Ctx;

/// Розширення, яке має бути увімкнене в БУДЬ-ЯКІЙ темі (швидкі налаштування «Тема»).
pub const PERMANENT_EXTENSIONS: &[&str] = &["themes-quick@desktop-themes-rs"];
pub const APP_ID: &str = "io.github.OlehDzhumyk.Themes";
pub const DBUS_NAME: &str = "io.github.OlehDzhumyk.Themes";
pub const DBUS_PATH: &str = "/io/github/OlehDzhumyk/Themes";
pub const EXT_KEY: &str = "/org/gnome/shell/enabled-extensions";
pub const DIS_KEY: &str = "/org/gnome/shell/disabled-extensions";
pub const SHELL_THEME_KEY: &str = "/org/gnome/shell/extensions/user-theme/name";
pub const BUILTIN_THEME: &str = "fedora";
