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
pub const MIXER_PATH: &str = "/io/github/OlehDzhumyk/Desk/Mixer";
pub const MIXER_IFACE: &str = "io.github.OlehDzhumyk.Desk.Mixer";
/// Version of the Mixer interface (its `Version` property): bumped on incompatible changes only.
pub const MIXER_VERSION: u32 = 1;

/// GSettings: загальна схема deskd (скін) і схема буфера обміну.
pub const SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk";
pub const CLIPBOARD_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Clipboard";
pub const MIXER_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Mixer";
pub const SCHEMA_XML: &str = include_str!("../../../data/io.github.OlehDzhumyk.Desk.gschema.xml");

pub const DESK_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.xml");
pub const CLIPBOARD_XML: &str =
    include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Clipboard.xml");
pub const MIXER_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Mixer.xml");

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

/// Mixer: what `ListDevices` takes, what `Changed` reports, and the client proxy (its own module:
/// the generated `Changed` signal types would clash with the clipboard's).
pub mod mixer {
    pub const OUTPUT: &str = "output";
    pub const INPUT: &str = "input";
    pub const CHANGED_DEVICES: &str = "devices";
    pub const CHANGED_APPS: &str = "apps";
    pub const CHANGED_DEFAULTS: &str = "defaults";
    pub const CHANGED_AVAILABLE: &str = "available";

    use super::{MixerApp, MixerDevice, MixerStream};

    /// Client proxy of the mixer engine.
    #[zbus::proxy(
        interface = "io.github.OlehDzhumyk.Desk.Mixer",
        default_service = "io.github.OlehDzhumyk.Desk",
        default_path = "/io/github/OlehDzhumyk/Desk/Mixer"
    )]
    pub trait Mixer {
        fn list_devices(&self, kind: &str) -> zbus::Result<Vec<MixerDevice>>;
        fn list_streams(&self) -> zbus::Result<Vec<MixerStream>>;
        fn list_apps(&self, include_silent: bool) -> zbus::Result<Vec<MixerApp>>;
        fn set_volume(&self, id: u32, volume: f64) -> zbus::Result<()>;
        fn set_mute(&self, id: u32, mute: bool) -> zbus::Result<()>;
        fn set_app_volume(&self, app: &str, volume: f64) -> zbus::Result<()>;
        fn set_app_mute(&self, app: &str, mute: bool) -> zbus::Result<()>;
        fn set_default(&self, kind: &str, id: u32) -> zbus::Result<()>;
        fn move_stream(&self, stream: u32, device: u32) -> zbus::Result<()>;
        fn move_app(&self, app: &str, device: u32) -> zbus::Result<()>;
        #[zbus(property)]
        fn version(&self) -> zbus::Result<u32>;
        #[zbus(property)]
        fn available(&self) -> zbus::Result<bool>;
        #[zbus(property)]
        fn default_output(&self) -> zbus::Result<u32>;
        #[zbus(property)]
        fn default_input(&self) -> zbus::Result<u32>;
        #[zbus(signal)]
        fn changed(&self, what: Vec<String>) -> zbus::Result<()>;
    }
}

/// An output or input device of the mixer, as `ListDevices` returns it (`(ussssdbb)`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, zbus::Type)]
pub struct MixerDevice {
    pub id: u32,
    /// PipeWire `node.name` (stable across restarts).
    pub name: String,
    /// What to show (`node.description`).
    pub description: String,
    pub icon: String,
    /// `bluetooth`, `usb`, `pci` or empty.
    pub bus: String,
    /// Cubic scale, 1.0 = 100 %.
    pub volume: f64,
    pub mute: bool,
    pub is_default: bool,
}

/// A playback stream of an application (`(ussssdbbu)`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, zbus::Type)]
pub struct MixerStream {
    pub id: u32,
    /// Application key: the Flatpak app id, `application.id`, the binary or the name.
    pub app: String,
    pub name: String,
    /// Icon name or file (a `GIcon` string).
    pub icon: String,
    /// `media.name` (track or tab title).
    pub media: String,
    pub volume: f64,
    pub mute: bool,
    pub playing: bool,
    /// Output device it plays on, 0 = not linked yet.
    pub device: u32,
}

/// One application row of the mixer: its streams merged (`(ssssdbbuau)`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, zbus::Type)]
pub struct MixerApp {
    pub key: String,
    pub name: String,
    pub icon: String,
    pub media: String,
    pub volume: f64,
    /// All of its streams are muted.
    pub mute: bool,
    /// At least one stream is playing.
    pub playing: bool,
    pub device: u32,
    pub streams: Vec<u32>,
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
    fn mixer_signatures_match_xml() {
        for (sig, xml) in [
            (MixerDevice::SIGNATURE.to_string(), "type=\"a(ussssdbb)\""),
            (MixerStream::SIGNATURE.to_string(), "type=\"a(ussssdbbu)\""),
            (MixerApp::SIGNATURE.to_string(), "type=\"a(ssssdbbuau)\""),
        ] {
            assert!(MIXER_XML.contains(xml), "{xml}");
            assert_eq!(format!("type=\"a{sig}\""), xml);
        }
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
