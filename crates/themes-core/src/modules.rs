// SPDX-License-Identifier: GPL-3.0-or-later
//! Модулі теми: вигляд, поведінка, звуки, програми.

use desk_settings::tr;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Module {
    Look,
    Behavior,
    Sounds,
    Apps,
}

impl Module {
    /// Порядок застосування: якщо ключ задають кілька модулів, перемагає пізніший.
    pub const ALL: [Module; 4] = [Module::Look, Module::Behavior, Module::Sounds, Module::Apps];

    pub fn id(self) -> &'static str {
        match self {
            Module::Look => "look",
            Module::Behavior => "behavior",
            Module::Sounds => "sounds",
            Module::Apps => "apps",
        }
    }

    /// Назва модуля мовою інтерфейсу.
    pub fn label(self) -> String {
        match self {
            Module::Look => tr!("Look"),
            Module::Behavior => tr!("Behaviour"),
            Module::Sounds => tr!("Sounds"),
            Module::Apps => tr!("Apps"),
        }
    }

    pub fn description(self) -> String {
        match self {
            Module::Look => tr!("GTK and shell themes, icons, fonts, wallpapers, extensions"),
            Module::Behavior => {
                tr!("Keyboard shortcuts, Alt+Tab, touchpad, animations, window placement")
            }
            Module::Sounds => tr!("Sound theme and system sounds"),
            Module::Apps => tr!("Application settings (the Ptyxis terminal palette and others)"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Module::Look => "applications-graphics-symbolic",
            Module::Behavior => "input-keyboard-symbolic",
            Module::Sounds => "audio-speakers-symbolic",
            Module::Apps => "utilities-terminal-symbolic",
        }
    }

    pub fn parse(s: &str) -> Result<Module> {
        match s.trim().to_lowercase().as_str() {
            "look" | "вигляд" => Ok(Module::Look), // i18n-ok: input alias
            "behavior" | "behaviour" | "поведінка" => Ok(Module::Behavior), // i18n-ok: input alias
            "sounds" | "sound" | "звуки" => Ok(Module::Sounds), // i18n-ok: input alias
            "apps" | "app" | "програми" => Ok(Module::Apps), // i18n-ok: input alias
            other => Err(Error(tr!(
                "unknown module “{module}” (available: look, behavior, sounds, apps)",
                module = other
            ))),
        }
    }

    /// "look,sounds" → [Look, Sounds] у канонічному порядку, без повторів.
    pub fn parse_list(s: &str) -> Result<Vec<Module>> {
        let mut v = Vec::new();
        for part in s.split([',', ' ']).filter(|p| !p.trim().is_empty()) {
            let m = Module::parse(part)?;
            if !v.contains(&m) {
                v.push(m);
            }
        }
        v.sort();
        Ok(v)
    }

    /// До якого модуля належить *.dconf-файл (невідомі — до «вигляду», для сумісності).
    pub fn for_dconf_file(name: &str) -> Module {
        match name {
            "behavior.dconf" => Module::Behavior,
            "sounds.dconf" => Module::Sounds,
            "apps.dconf" => Module::Apps,
            _ => Module::Look,
        }
    }
}

impl std::fmt::Display for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_mapping() {
        assert_eq!(Module::for_dconf_file("settings.dconf"), Module::Look);
        assert_eq!(Module::for_dconf_file("extensions.dconf"), Module::Look);
        assert_eq!(Module::for_dconf_file("whatever.dconf"), Module::Look);
        assert_eq!(Module::for_dconf_file("behavior.dconf"), Module::Behavior);
        assert_eq!(Module::for_dconf_file("sounds.dconf"), Module::Sounds);
        assert_eq!(Module::for_dconf_file("apps.dconf"), Module::Apps);
    }

    #[test]
    fn list_parsing() {
        assert_eq!(
            Module::parse_list("sounds,look,look").unwrap(),
            vec![Module::Look, Module::Sounds]
        );
        assert_eq!(
            Module::parse_list("behavior").unwrap(),
            vec![Module::Behavior]
        );
        assert!(Module::parse_list("look,colors").is_err());
    }

    #[test]
    fn serde_names() {
        assert_eq!(
            serde_json::to_string(&Module::Behavior).unwrap(),
            "\"behavior\""
        );
        let m: std::collections::BTreeMap<Module, String> =
            serde_json::from_str(r#"{"apps":"x","look":"y"}"#).unwrap();
        assert_eq!(m[&Module::Look], "y");
    }
}
