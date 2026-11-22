// SPDX-License-Identifier: GPL-3.0-or-later
//! Спільні назви D-Bus, визначення інтерфейсів і типи для `deskd` та його клієнтів
//! (GUI `themes`, міст оболонки `desk-bridge@desktop-themes-rs`).
//!
//! XML інтерфейсів лежить у `data/dbus/` і вбудовується сюди ж ([`CLIPBOARD_XML`]); міст оболонки
//! отримує ті самі файли під час збирання.

use serde::{Deserialize, Serialize};

pub const BUS_NAME: &str = "io.github.OlehDzhumyk.Desk";
pub const DESK_PATH: &str = "/io/github/OlehDzhumyk/Desk";
pub const DESK_IFACE: &str = "io.github.OlehDzhumyk.Desk";
pub const CLIPBOARD_PATH: &str = "/io/github/OlehDzhumyk/Desk/Clipboard";
pub const CLIPBOARD_IFACE: &str = "io.github.OlehDzhumyk.Desk.Clipboard";

/// GSettings: загальна схема deskd (скін) і схема буфера обміну.
pub const SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk";
pub const CLIPBOARD_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Clipboard";
pub const SCHEMA_XML: &str = include_str!("../../../data/io.github.OlehDzhumyk.Desk.gschema.xml");

pub const DESK_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.xml");
pub const CLIPBOARD_XML: &str =
    include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Clipboard.xml");

/// UUID розширення-моста в GNOME Shell.
pub const BRIDGE_UUID: &str = "desk-bridge@desktop-themes-rs";

/// Скіни моста: як виглядають і де з'являються панелі функцій.
pub mod skin {
    pub const DEFAULT: &str = "default";
    pub const MACOS: &str = "macos";
    pub const WINDOWS11: &str = "windows11";
    pub const ALL: [&str; 3] = [DEFAULT, MACOS, WINDOWS11];
}

/// Вид запису буфера обміну.
pub mod kind {
    pub const TEXT: &str = "text";
    pub const IMAGE: &str = "image";
}

/// Запис історії буфера обміну, як його повертає `List` (`(tssssbxt)`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, zbus::Type)]
pub struct ClipItem {
    pub id: u64,
    /// [`kind::TEXT`] або [`kind::IMAGE`]
    pub kind: String,
    /// Текст запису (для зображень — короткий опис, напр. «Зображення PNG 640×480»).
    pub text: String,
    pub mime: String,
    /// Файл зображення (порожньо для тексту).
    pub image: String,
    pub pinned: bool,
    /// Час останнього копіювання/вставлення (unix, секунди).
    pub time: i64,
    /// Розмір вмісту в байтах.
    pub size: u64,
}

/// Клієнтський проксі самої служби (версія, список функцій).
#[zbus::proxy(
    interface = "io.github.OlehDzhumyk.Desk",
    default_service = "io.github.OlehDzhumyk.Desk",
    default_path = "/io/github/OlehDzhumyk/Desk"
)]
pub trait Desk {
    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn features(&self) -> zbus::Result<Vec<String>>;
}

/// Клієнтський проксі рушія буфера обміну.
#[zbus::proxy(
    interface = "io.github.OlehDzhumyk.Desk.Clipboard",
    default_service = "io.github.OlehDzhumyk.Desk",
    default_path = "/io/github/OlehDzhumyk/Desk/Clipboard"
)]
pub trait Clipboard {
    fn push(
        &self,
        mime: &str,
        data: &[u8],
        offered: &[&str],
        source_app: &str,
    ) -> zbus::Result<u64>;
    fn list(&self, query: &str, limit: u32) -> zbus::Result<Vec<ClipItem>>;
    fn get(&self, id: u64) -> zbus::Result<(String, Vec<u8>)>;
    fn activate(&self, id: u64) -> zbus::Result<(String, Vec<u8>)>;
    fn pin(&self, id: u64, pinned: bool) -> zbus::Result<()>;
    fn delete(&self, id: u64) -> zbus::Result<()>;
    fn clear(&self, keep_pinned: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn private(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_private(&self, value: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn enabled(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn count(&self) -> zbus::Result<u32>;
    #[zbus(signal)]
    fn changed(&self) -> zbus::Result<()>;
}

/// Імена членів інтерфейсу з XML (`<method name=…>`, `<property name=…>`, `<signal name=…>`),
/// для перевірки, що реалізація й XML не розійшлися.
pub fn xml_members(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for kind in ["method", "property", "signal"] {
        let pat = format!("<{kind} name=\"");
        let mut rest = xml;
        while let Some(i) = rest.find(&pat) {
            rest = &rest[i + pat.len()..];
            if let Some(j) = rest.find('"') {
                out.push((kind.to_string(), rest[..j].to_string()));
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::Type;

    #[test]
    fn item_signature_matches_xml() {
        assert_eq!(ClipItem::SIGNATURE.to_string(), "(tssssbxt)");
        assert!(CLIPBOARD_XML.contains("type=\"a(tssssbxt)\""));
    }

    #[test]
    fn xml_parsed() {
        let m = xml_members(CLIPBOARD_XML);
        assert!(m.contains(&("method".into(), "Push".into())));
        assert!(m.contains(&("property".into(), "Private".into())));
        assert!(m.contains(&("signal".into(), "Changed".into())));
        assert_eq!(m.iter().filter(|(k, _)| k == "method").count(), 7);
    }

    #[test]
    fn schema_has_skin_choices() {
        for s in skin::ALL {
            assert!(
                SCHEMA_XML.contains(&format!("<choice value=\"{s}\"/>")),
                "{s}"
            );
        }
        assert!(SCHEMA_XML.contains(&format!("id=\"{CLIPBOARD_SCHEMA_ID}\"")));
    }
}
