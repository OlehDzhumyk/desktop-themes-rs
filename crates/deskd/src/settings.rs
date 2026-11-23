// SPDX-License-Identifier: GPL-3.0-or-later
//! GSettings `io.github.OlehDzhumyk.Desk.Clipboard` → [`Config`] рушія, через типізовані
//! аксесори, згенеровані з реєстру налаштувань (`desk-settings`).
//!
//! Об'єкти `gio::Settings` живуть лише в головному потоці (там крутиться цикл GLib); з потоків
//! zbus зміни надсилаються через `glib::MainContext::invoke`.

use desk_settings::accessors::{AccentSource, AppearanceSettings, ClipboardSettings, Mode};
use gio::glib;
use gio::prelude::*;

use crate::appearance::solar::{AppearanceMode, Scheme};
use crate::clipboard::Config;

pub fn clipboard_settings() -> Option<ClipboardSettings> {
    ClipboardSettings::new()
}

pub fn read_config(s: &ClipboardSettings) -> Config {
    Config {
        enabled: s.enabled(),
        history_size: s.history_size().max(1) as usize,
        images: s.images(),
        image_max_bytes: s.image_max_kb() as usize * 1024,
        image_budget_bytes: s.image_budget_mb() as usize * 1024 * 1024,
        persist: s.persist(),
        persist_images: s.persist_images(),
        private: s.private_mode(),
        excluded_apps: s.excluded_apps(),
    }
}

/// Записати приватний режим у GSettings із будь-якого потоку.
pub fn set_private_async(value: bool) {
    glib::MainContext::default().invoke(move || {
        if let Some(s) = clipboard_settings() {
            let _ = s.set_private_mode(value);
        }
    });
}

/// GSettings `io.github.OlehDzhumyk.Desk.Mixer` (None while the schema is not installed).
pub fn mixer_settings() -> Option<desk_settings::accessors::MixerSettings> {
    desk_settings::accessors::MixerSettings::new()
}

/// Whether the mixer is on, and the engine's part of its settings.
pub fn read_mixer(
    s: &desk_settings::accessors::MixerSettings,
) -> (bool, crate::mixer::model::Config) {
    (
        s.enabled(),
        crate::mixer::model::Config {
            remember: s.remember_app_volume(),
            hide_apps: s.hide_apps(),
        },
    )
}

const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
const COLOR_SCHEME_KEY: &str = "color-scheme";

/// GSettings `io.github.OlehDzhumyk.Desk.Appearance` (None while the schema is not installed).
pub fn appearance_settings() -> Option<AppearanceSettings> {
    AppearanceSettings::new()
}

/// Read the appearance engine configuration from GSettings.
pub fn read_appearance(s: &AppearanceSettings) -> crate::appearance::Config {
    let mode = match s.mode() {
        Mode::Manual => AppearanceMode::Manual,
        Mode::Light => AppearanceMode::Light,
        Mode::Dark => AppearanceMode::Dark,
        Mode::AutoSun => AppearanceMode::AutoSun,
        Mode::AutoTime => AppearanceMode::AutoTime,
    };
    crate::appearance::Config {
        mode,
        dark_from: s.dark_from(),
        light_from: s.light_from(),
        location: s.location(),
        twilight_offset: s.twilight_offset(),
        override_until_next: s.override_until_next(),
    }
}

/// Read the live "accent from wallpaper" configuration from GSettings.
pub fn read_wallpaper(s: &AppearanceSettings) -> crate::appearance::wallpaper::Config {
    use crate::appearance::wallpaper::{Config, Source};
    Config {
        enabled: s.accent_from_wallpaper(),
        source: match s.accent_source() {
            AccentSource::Active => Source::Active,
            AccentSource::Light => Source::Light,
            AccentSource::Dark => Source::Dark,
        },
    }
}

/// GSettings `org.gnome.desktop.interface` (None when unavailable).
pub fn interface_settings() -> Option<gio::Settings> {
    desk_settings::schema_installed(INTERFACE_SCHEMA).then(|| gio::Settings::new(INTERFACE_SCHEMA))
}

/// Read the current `color-scheme` from `org.gnome.desktop.interface`.
pub fn read_color_scheme(s: &gio::Settings) -> Scheme {
    Scheme::parse(&s.string(COLOR_SCHEME_KEY)).unwrap_or(Scheme::Light)
}

/// Write `org.gnome.desktop.interface color-scheme` from any thread via the GLib main loop.
pub fn set_color_scheme_async(scheme: Scheme) {
    glib::MainContext::default().invoke(move || {
        if let Some(s) = interface_settings() {
            let _ = s.set_string(COLOR_SCHEME_KEY, scheme.as_gsettings_str());
        }
    });
}
