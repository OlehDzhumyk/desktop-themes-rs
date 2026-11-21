// SPDX-License-Identifier: GPL-3.0-or-later
//! GSettings `io.github.OlehDzhumyk.Desk.Clipboard` → [`Config`] рушія.
//!
//! Об'єкти `gio::Settings` живуть лише в головному потоці (там крутиться цикл GLib); з потоків
//! zbus зміни надсилаються через `glib::MainContext::invoke`.

use desk_proto::CLIPBOARD_SCHEMA_ID;
use gio::glib;
use gio::prelude::*;

use crate::clipboard::Config;

/// Схема встановлена? (Без неї `gio::Settings::new` аварійно завершує процес.)
pub fn schema_installed(id: &str) -> bool {
    gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup(id, true))
        .is_some()
}

pub fn clipboard_settings() -> Option<gio::Settings> {
    schema_installed(CLIPBOARD_SCHEMA_ID).then(|| gio::Settings::new(CLIPBOARD_SCHEMA_ID))
}

pub fn read_config(s: &gio::Settings) -> Config {
    Config {
        enabled: s.boolean("enabled"),
        history_size: s.uint("history-size").max(1) as usize,
        images: s.boolean("images"),
        image_max_bytes: s.uint("image-max-kb") as usize * 1024,
        persist: s.boolean("persist"),
        persist_images: s.boolean("persist-images"),
        private: s.boolean("private-mode"),
        excluded_apps: s.strv("excluded-apps").iter().map(|g| g.to_string()).collect(),
    }
}

/// Записати приватний режим у GSettings із будь-якого потоку.
pub fn set_private_async(value: bool) {
    glib::MainContext::default().invoke(move || {
        if let Some(s) = clipboard_settings() {
            let _ = s.set_boolean("private-mode", value);
        }
    });
}
