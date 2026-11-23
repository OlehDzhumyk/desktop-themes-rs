// SPDX-License-Identifier: GPL-3.0-or-later
//! GSettings `io.github.OlehDzhumyk.Desk.Clipboard` → [`Config`] рушія, через типізовані
//! аксесори, згенеровані з реєстру налаштувань (`desk-settings`).
//!
//! Об'єкти `gio::Settings` живуть лише в головному потоці (там крутиться цикл GLib); з потоків
//! zbus зміни надсилаються через `glib::MainContext::invoke`.

use desk_settings::accessors::ClipboardSettings;
use gio::glib;

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

/// GSettings `io.github.OlehDzhumyk.Desk.Emoji` (None while the schema is not installed).
pub fn emoji_settings() -> Option<desk_settings::accessors::EmojiSettings> {
    desk_settings::accessors::EmojiSettings::new()
}

/// Emoji engine settings: `(remember_recents, default_skin_tone)`.
pub fn read_emoji(s: &desk_settings::accessors::EmojiSettings) -> (bool, String) {
    (s.remember_recents(), s.skin_tone().as_str().to_string())
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
