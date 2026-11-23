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
pub const EMOJI_PATH: &str = "/io/github/OlehDzhumyk/Desk/Emoji";
pub const EMOJI_IFACE: &str = "io.github.OlehDzhumyk.Desk.Emoji";
pub const EMOJI_INTERFACE: &str = EMOJI_IFACE;
pub const MIXER_PATH: &str = "/io/github/OlehDzhumyk/Desk/Mixer";
pub const MIXER_IFACE: &str = "io.github.OlehDzhumyk.Desk.Mixer";
/// Version of the Mixer interface (its `Version` property): bumped on incompatible changes only.
pub const MIXER_VERSION: u32 = 1;
pub const APPEARANCE_PATH: &str = "/io/github/OlehDzhumyk/Desk/Appearance";
pub const APPEARANCE_INTERFACE: &str = "io.github.OlehDzhumyk.Desk.Appearance";
pub const FOCUS_PATH: &str = "/io/github/OlehDzhumyk/Desk/Focus";
pub const FOCUS_INTERFACE: &str = "io.github.OlehDzhumyk.Desk.Focus";
pub const COLOR_PICKER_PATH: &str = "/io/github/OlehDzhumyk/Desk/ColorPicker";
pub const COLOR_PICKER_IFACE: &str = "io.github.OlehDzhumyk.Desk.ColorPicker";
pub const OCR_PATH: &str = "/io/github/OlehDzhumyk/Desk/Ocr";
pub const OCR_IFACE: &str = "io.github.OlehDzhumyk.Desk.Ocr";
pub const CAPTURE_PATH: &str = "/io/github/OlehDzhumyk/Desk/Capture";
pub const CAPTURE_IFACE: &str = "io.github.OlehDzhumyk.Desk.Capture";

/// GSettings: загальна схема deskd (скін) і схема буфера обміну.
pub const SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk";
pub const CLIPBOARD_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Clipboard";
pub const EMOJI_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Emoji";
pub const MIXER_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Mixer";
pub const APPEARANCE_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Appearance";
pub const FOCUS_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Focus";
pub const COLOR_PICKER_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.ColorPicker";
pub const OCR_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Ocr";
pub const CAPTURE_SCHEMA_ID: &str = "io.github.OlehDzhumyk.Desk.Capture";
pub const SCHEMA_XML: &str = include_str!("../../../data/io.github.OlehDzhumyk.Desk.gschema.xml");

pub const DESK_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.xml");
pub const CLIPBOARD_XML: &str =
    include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Clipboard.xml");
pub const EMOJI_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Emoji.xml");
pub const MIXER_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Mixer.xml");
pub const APPEARANCE_XML: &str =
    include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Appearance.xml");
pub const FOCUS_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Focus.xml");
pub const COLOR_PICKER_XML: &str =
    include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.ColorPicker.xml");
pub const OCR_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Ocr.xml");
pub const CAPTURE_XML: &str =
    include_str!("../../../data/dbus/io.github.OlehDzhumyk.Desk.Capture.xml");

/// Offered by the bridge with the clipboard content it set itself (paste from the history), so
/// deskd does not record its own paste again. Apps may offer it too; it only stops recording.
pub const INTERNAL_MIME: &str = "application/x-desk-internal";

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

/// Emoji and symbol picker: item type and client proxy.
pub mod emoji {
    use serde::{Deserialize, Serialize};

    /// One emoji, Unicode symbol or text face (`(sssssb)`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, zbus::Type)]
    pub struct EmojiItem {
        pub id: String,
        pub glyph: String,
        pub name: String,
        pub category: String,
        pub keywords: String,
        pub supports_tone: bool,
    }

    /// Client proxy of the emoji engine.
    #[zbus::proxy(
        interface = "io.github.OlehDzhumyk.Desk.Emoji",
        default_service = "io.github.OlehDzhumyk.Desk",
        default_path = "/io/github/OlehDzhumyk/Desk/Emoji"
    )]
    pub trait Emoji {
        fn search(&self, query: &str, lang: &str, limit: u32) -> zbus::Result<Vec<EmojiItem>>;
        fn category(&self, category: &str, limit: u32) -> zbus::Result<Vec<EmojiItem>>;
        fn recents(&self) -> zbus::Result<Vec<EmojiItem>>;
        fn pick(&self, id: &str, tone: &str) -> zbus::Result<String>;
        fn clear_recents(&self) -> zbus::Result<()>;
    }
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

/// Appearance (auto-dark) client proxy in its own module so its `Changed` signal does not clash.
pub mod appearance {
    /// Client proxy of the automatic light/dark appearance engine.
    #[zbus::proxy(
        interface = "io.github.OlehDzhumyk.Desk.Appearance",
        default_service = "io.github.OlehDzhumyk.Desk",
        default_path = "/io/github/OlehDzhumyk/Desk/Appearance"
    )]
    pub trait Appearance {
        fn status(&self) -> zbus::Result<(String, String, String, String, bool, bool)>;
        fn set_scheme(&self, scheme: &str) -> zbus::Result<()>;
        fn recompute(&self) -> zbus::Result<()>;
        /// Live "accent from wallpaper": (enabled, seed, accent, monochrome, uri, error, palette).
        #[allow(clippy::type_complexity)]
        fn wallpaper_accent(
            &self,
        ) -> zbus::Result<(bool, String, String, bool, String, String, String)>;
        #[zbus(signal)]
        fn changed(&self) -> zbus::Result<()>;
    }
}

/// Focus modes client proxy in its own module so its signals do not clash.
pub mod focus {
    /// `Status()`: mode, name, icon, source, until, break_until, next_mode, next_start,
    /// allowed_apps.
    pub type Status = (
        String,
        String,
        String,
        String,
        i64,
        i64,
        String,
        i64,
        Vec<String>,
    );

    /// Client proxy of the focus modes engine.
    #[zbus::proxy(
        interface = "io.github.OlehDzhumyk.Desk.Focus",
        default_service = "io.github.OlehDzhumyk.Desk",
        default_path = "/io/github/OlehDzhumyk/Desk/Focus"
    )]
    pub trait Focus {
        fn status(&self) -> zbus::Result<Status>;
        fn list_modes(&self) -> zbus::Result<String>;
        fn start(&self, mode: &str, until: i64) -> zbus::Result<()>;
        fn start_session(&self, minutes: u32) -> zbus::Result<()>;
        fn stop(&self) -> zbus::Result<()>;
        fn save_mode(&self, mode: &str) -> zbus::Result<()>;
        fn delete_mode(&self, id: &str) -> zbus::Result<()>;
        fn recompute(&self) -> zbus::Result<()>;
        #[zbus(signal)]
        fn changed(&self) -> zbus::Result<()>;
        #[zbus(signal)]
        fn session_ended(&self) -> zbus::Result<()>;
    }
}

/// Colour picker: history item type and client proxy (its own module so its `Changed` signal does
/// not clash).
pub mod color_picker {
    use serde::{Deserialize, Serialize};

    /// One remembered pick, newest first in `List` (`(sssx)`).
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, zbus::Type)]
    pub struct ColorPick {
        /// `#RRGGBB`, upper case: the identity of the pick (`Remove` takes it).
        pub hex: String,
        /// The colour in the `format` setting in force.
        pub formatted: String,
        /// Nearest CSS colour name, for accessible swatch names.
        pub name: String,
        /// When it was last picked (unix, seconds).
        pub picked_at: i64,
    }

    /// Client proxy of the colour picker engine.
    #[zbus::proxy(
        interface = "io.github.OlehDzhumyk.Desk.ColorPicker",
        default_service = "io.github.OlehDzhumyk.Desk",
        default_path = "/io/github/OlehDzhumyk/Desk/ColorPicker"
    )]
    pub trait ColorPicker {
        /// `(formatted, hex, nearest_name)`.
        fn record(&self, r: u8, g: u8, b: u8) -> zbus::Result<(String, String, String)>;
        fn list(&self) -> zbus::Result<Vec<ColorPick>>;
        fn remove(&self, hex: &str) -> zbus::Result<bool>;
        fn clear(&self) -> zbus::Result<()>;
        fn format(&self, r: u8, g: u8, b: u8, format: &str) -> zbus::Result<String>;
        fn contrast(&self) -> zbus::Result<f64>;
        #[zbus(signal)]
        fn changed(&self) -> zbus::Result<()>;
    }
}

/// OCR text extractor: error names and client proxy (its own module, like the other engines).
pub mod ocr {
    /// Prefix of the D-Bus error names of `Recognize` (`<prefix>.TooLarge`, ...).
    pub const ERROR_PREFIX: &str = "io.github.OlehDzhumyk.Desk.Ocr.Error";
    /// Length of the `preview` that `Recognize` returns, in characters (plus `…` when cut).
    pub const PREVIEW_CHARS: usize = 60;

    /// Client proxy of the OCR engine.
    #[zbus::proxy(
        interface = "io.github.OlehDzhumyk.Desk.Ocr",
        default_service = "io.github.OlehDzhumyk.Desk",
        default_path = "/io/github/OlehDzhumyk/Desk/Ocr"
    )]
    pub trait Ocr {
        /// `(text, preview, missing_langs)`.
        fn recognize(
            &self,
            png: &[u8],
            width: u32,
            height: u32,
        ) -> zbus::Result<(String, String, Vec<String>)>;
        fn available_languages(&self) -> zbus::Result<Vec<String>>;
        fn is_available(&self) -> zbus::Result<bool>;
    }
}

/// Screenshots: client proxy of the capture engine.
pub mod capture {
    /// Client proxy of the capture engine (`data/dbus/io.github.OlehDzhumyk.Desk.Capture.xml`).
    #[zbus::proxy(
        interface = "io.github.OlehDzhumyk.Desk.Capture",
        default_service = "io.github.OlehDzhumyk.Desk",
        default_path = "/io/github/OlehDzhumyk/Desk/Capture"
    )]
    pub trait Capture {
        /// Save a PNG capture; returns the new file's path.
        fn save_screenshot(&self, png_bytes: &[u8]) -> zbus::Result<String>;
        /// Draw annotations (JSON) into a PNG file and replace it; returns the saved path.
        fn apply_annotations(&self, path: &str, annotations_json: &str) -> zbus::Result<String>;
    }
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
    fn emoji_signature_matches_xml() {
        assert_eq!(emoji::EmojiItem::SIGNATURE.to_string(), "(sssssb)");
        assert!(EMOJI_XML.contains("type=\"a(sssssb)\""));
        let m = xml_members(EMOJI_XML);
        for name in ["Search", "Category", "Recents", "Pick", "ClearRecents"] {
            assert!(m.contains(&("method".into(), name.into())), "{name}");
        }
    }

    #[test]
    fn color_picker_signature_matches_xml() {
        assert_eq!(color_picker::ColorPick::SIGNATURE.to_string(), "(sssx)");
        assert!(COLOR_PICKER_XML.contains("type=\"a(sssx)\""));
        assert!(COLOR_PICKER_XML.contains(&format!("name=\"{COLOR_PICKER_IFACE}\"")));
    }

    #[test]
    fn ocr_xml_declares_the_interface_and_errors() {
        assert!(OCR_XML.contains(&format!("name=\"{OCR_IFACE}\"")));
        let m = xml_members(OCR_XML);
        for name in ["Recognize", "AvailableLanguages", "IsAvailable"] {
            assert!(m.contains(&("method".into(), name.into())), "{name}");
        }
        for e in [
            "Disabled",
            "InvalidImage",
            "TooLarge",
            "Busy",
            "MissingEngine",
            "NoLanguages",
            "Timeout",
            "Failed",
        ] {
            assert!(
                OCR_XML.contains(&format!("{}.{e}", ocr::ERROR_PREFIX)),
                "{e}"
            );
        }
    }

    #[test]
    fn capture_xml_names_the_interface_and_methods() {
        assert!(CAPTURE_XML.contains(&format!("name=\"{CAPTURE_IFACE}\"")));
        let m = xml_members(CAPTURE_XML);
        assert_eq!(
            m,
            [
                ("method".to_string(), "ApplyAnnotations".to_string()),
                ("method".to_string(), "SaveScreenshot".to_string())
            ]
        );
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
