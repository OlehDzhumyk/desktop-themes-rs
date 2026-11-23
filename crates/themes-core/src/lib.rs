// SPDX-License-Identifier: GPL-3.0-or-later
//! themes-core — бібліотека менеджера тем робочого столу GNOME (рівень користувача).
//!
//! Бібліотека містить усю логіку; CLI, GUI і D-Bus-служба лише викликають її.

pub mod apply;
pub mod commands;
pub mod crash;
pub mod dconf;
pub mod doctor;
pub mod error;
pub mod flatpak;
pub mod gtklinks;
pub mod hooks;
pub mod lock;
pub mod modules;
pub mod overlay;
pub mod paths;
pub mod plan;
pub mod shell;
pub mod shortcuts;
pub mod snapshot;
pub mod state;
pub mod theme;
pub mod tokens;
pub mod txn;
pub mod util;
pub mod values;

pub use error::{Error, Result};
pub use modules::Module;
pub use paths::Ctx;

/// Розширення, яке має бути увімкнене в БУДЬ-ЯКІЙ темі: міст до оболонки (швидкі налаштування
/// «Тема», буфер обміну та інші функції deskd).
pub const PERMANENT_EXTENSIONS: &[&str] = &["desk-bridge@desktop-themes-rs"];
/// Розширення, які замінив міст або які вилучено (прибираються з `enabled-extensions` під час міграції/застосування).
pub const RETIRED_EXTENSIONS: &[&str] = &[
    "themes-quick@desktop-themes-rs",
    "logomenu@aryan_k",
    "liquid-glass@thinkingcoding1231.gmail.com",
];
pub const APP_ID: &str = "io.github.OlehDzhumyk.Themes";
pub const DBUS_NAME: &str = "io.github.OlehDzhumyk.Themes";
pub const DBUS_PATH: &str = "/io/github/OlehDzhumyk/Themes";
pub const EXT_KEY: &str = "/org/gnome/shell/enabled-extensions";
pub const DIS_KEY: &str = "/org/gnome/shell/disabled-extensions";
pub const SHELL_THEME_KEY: &str = "/org/gnome/shell/extensions/user-theme/name";
pub const BUILTIN_THEME: &str = "fedora";
