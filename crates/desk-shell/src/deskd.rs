// SPDX-License-Identifier: GPL-3.0-or-later
//! Calls to `deskd` on the session bus (GIO, asynchronous, never blocking the shell).
//!
//! deskd owns the logic and the storage (ADR 0002); this shell only forwards. When deskd is not
//! running, every call logs one line and the shell keeps working.

use desk_proto as proto;
use gtk::gio;
use gtk::glib::{self, Variant};
use gtk::prelude::*;

/// One clipboard history item, as `List` returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipRow {
    pub id: u64,
    pub kind: String,
    pub text: String,
    pub pinned: bool,
}

/// One audio device, as `Mixer.ListDevices` returns it.
#[derive(Clone, Debug, PartialEq)]
pub struct MixerDevice {
    pub id: u32,
    pub description: String,
    pub icon: String,
    pub volume: f64,
    pub muted: bool,
    pub is_default: bool,
}

/// One application that plays sound, as `Mixer.ListApps` returns it.
#[derive(Clone, Debug, PartialEq)]
pub struct MixerApp {
    pub key: String,
    pub name: String,
    pub icon: String,
    pub volume: f64,
    pub muted: bool,
}

type DeviceItem = (u32, String, String, String, String, f64, bool, bool);
type AppItem = (
    String,
    String,
    String,
    String,
    f64,
    bool,
    bool,
    u32,
    Vec<u32>,
);

type ListItem = (u64, String, String, String, String, bool, i64, u64);

/// The rows of a `List` answer (`(a(tssssbxt))`); `None` for another type.
pub fn clip_rows(reply: &Variant) -> Option<Vec<ClipRow>> {
    let (items,) = reply.get::<(Vec<ListItem>,)>()?;
    Some(
        items
            .into_iter()
            .map(|(id, kind, text, _, _, pinned, ..)| ClipRow {
                id,
                kind,
                text,
                pinned,
            })
            .collect(),
    )
}

/// The devices of a `ListDevices` answer (`(a(ussssdbb))`); `None` for another type.
pub fn mixer_devices(reply: &Variant) -> Option<Vec<MixerDevice>> {
    let (items,) = reply.get::<(Vec<DeviceItem>,)>()?;
    Some(
        items
            .into_iter()
            .map(
                |(id, _, description, icon, _, volume, muted, is_default)| MixerDevice {
                    id,
                    description,
                    icon,
                    volume,
                    muted,
                    is_default,
                },
            )
            .collect(),
    )
}

/// The applications of a `ListApps` answer (`(a(ssssdbbuau))`); `None` for another type.
pub fn mixer_apps(reply: &Variant) -> Option<Vec<MixerApp>> {
    let (items,) = reply.get::<(Vec<AppItem>,)>()?;
    Some(
        items
            .into_iter()
            .map(|(key, name, icon, _, volume, muted, ..)| MixerApp {
                key,
                name,
                icon,
                volume,
                muted,
            })
            .collect(),
    )
}

/// The default output: the device marked default, else the first one.
pub fn default_device(devices: &[MixerDevice]) -> Option<&MixerDevice> {
    devices
        .iter()
        .find(|d| d.is_default)
        .or_else(|| devices.first())
}

/// A slider volume (cubic, 1.0 = 100 %) as whole percent, never negative.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is finite and in 0.0..=150.0 before the cast"
)]
pub fn volume_percent(volume: f64) -> u32 {
    if volume.is_finite() && volume > 0.0 {
        (volume.min(1.5) * 100.0).round() as u32
    } else {
        0
    }
}

/// The symbolic icon for a volume.
pub fn volume_icon(volume: f64, muted: bool) -> &'static str {
    match volume_percent(volume) {
        _ if muted => "audio-volume-muted-symbolic",
        0 => "audio-volume-muted-symbolic",
        1..=33 => "audio-volume-low-symbolic",
        34..=66 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

/// A focus mode is active in a `Focus.Status` answer (its first value is not empty).
pub fn focus_active(reply: &Variant) -> Option<bool> {
    if !reply.is_container() || reply.n_children() == 0 {
        return None;
    }
    Some(!reply.child_value(0).str()?.is_empty())
}

/// The scheme of an `Appearance.Status` answer is dark.
pub fn scheme_is_dark(reply: &Variant) -> Option<bool> {
    if !reply.is_container() || reply.n_children() < 2 {
        return None;
    }
    Some(reply.child_value(1).str()? == "dark")
}

/// The boolean in a `Properties.Get` answer (`(v)`).
pub fn property_bool(reply: &Variant) -> Option<bool> {
    if !reply.is_container() || reply.n_children() == 0 {
        return None;
    }
    let value = reply.child_value(0);
    if !value.is_type(glib::VariantTy::VARIANT) {
        return None;
    }
    value.as_variant()?.get::<bool>()
}

/// One line of at most `max` characters for a menu: whitespace runs collapsed, `…` when cut.
pub fn preview(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// Call deskd; `done` gets the answer, or `None` (logged) when deskd or the bus is missing.
fn try_call(
    path: &'static str,
    iface: &'static str,
    method: &'static str,
    args: Option<Variant>,
    done: impl FnOnce(Option<Variant>) + 'static,
) {
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, move |bus| {
        let bus = match bus {
            Ok(b) => b,
            Err(e) => {
                eprintln!("desk-shell: no session bus: {e}");
                return done(None);
            }
        };
        bus.call(
            Some(proto::BUS_NAME),
            path,
            iface,
            method,
            args.as_ref(),
            None,
            gio::DBusCallFlags::NONE,
            5000,
            gio::Cancellable::NONE,
            move |reply| match reply {
                Ok(v) => done(Some(v)),
                Err(e) => {
                    eprintln!("desk-shell: deskd {iface}.{method}: {e}");
                    done(None);
                }
            },
        );
    });
}

/// Call deskd; `done` runs only on success.
fn call(
    path: &'static str,
    iface: &'static str,
    method: &'static str,
    args: Option<Variant>,
    done: impl FnOnce(Variant) + 'static,
) {
    try_call(path, iface, method, args, |reply| {
        if let Some(v) = reply {
            done(v);
        }
    });
}

/// Count a launch in the search usage (`Search.RecordLaunch`).
pub fn record_launch(item_id: &str) {
    call(
        proto::SEARCH_PATH,
        proto::SEARCH_IFACE,
        "RecordLaunch",
        Some((item_id,).to_variant()),
        |_| {},
    );
}

const PROPS: &str = "org.freedesktop.DBus.Properties";

/// Whether the scheme is dark now (`None`: deskd is not running).
pub fn dark_mode(done: impl FnOnce(Option<bool>) + 'static) {
    try_call(
        proto::APPEARANCE_PATH,
        proto::APPEARANCE_INTERFACE,
        "Status",
        None,
        |reply| done(reply.as_ref().and_then(scheme_is_dark)),
    );
}

/// Set the scheme to dark or light (`Appearance.SetScheme`).
pub fn set_dark_mode(dark: bool) {
    call(
        proto::APPEARANCE_PATH,
        proto::APPEARANCE_INTERFACE,
        "SetScheme",
        Some((if dark { "dark" } else { "light" },).to_variant()),
        |_| {},
    );
}

/// Switch light ↔ dark.
pub fn toggle_dark_mode() {
    dark_mode(|dark| {
        if let Some(dark) = dark {
            set_dark_mode(!dark);
        }
    });
}

/// Whether a focus mode is active (`None`: deskd is not running).
pub fn focus_status(done: impl FnOnce(Option<bool>) + 'static) {
    try_call(
        proto::FOCUS_PATH,
        proto::FOCUS_INTERFACE,
        "Status",
        None,
        |reply| done(reply.as_ref().and_then(focus_active)),
    );
}

/// Start do not disturb until turned off, or end the active focus mode.
pub fn set_focus(on: bool) {
    let (method, args) = if on {
        ("Start", Some(("dnd", 0_i64).to_variant()))
    } else {
        ("Stop", None)
    };
    call(
        proto::FOCUS_PATH,
        proto::FOCUS_INTERFACE,
        method,
        args,
        |_| {},
    );
}

/// Whether the clipboard private mode is on (`None`: deskd is not running).
pub fn clipboard_private(done: impl FnOnce(Option<bool>) + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        PROPS,
        "Get",
        Some((proto::CLIPBOARD_IFACE, "Private").to_variant()),
        |reply| done(reply.as_ref().and_then(property_bool)),
    );
}

/// Turn the clipboard private mode on or off.
pub fn set_clipboard_private(on: bool) {
    call(
        proto::CLIPBOARD_PATH,
        PROPS,
        "Set",
        Some((proto::CLIPBOARD_IFACE, "Private", on.to_variant()).to_variant()),
        |_| {},
    );
}

/// Flip the clipboard `Private` property.
pub fn toggle_clipboard_private() {
    clipboard_private(|on| {
        if let Some(on) = on {
            set_clipboard_private(!on);
        }
    });
}

/// The most recent clipboard items (`None`: deskd is not running).
pub fn clipboard_list(limit: u32, done: impl FnOnce(Option<Vec<ClipRow>>) + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "List",
        Some(("", limit).to_variant()),
        move |reply| done(reply.as_ref().and_then(clip_rows)),
    );
}

/// Pin or unpin an item; `done` runs after the answer (also on failure).
pub fn clipboard_pin(id: u64, pinned: bool, done: impl FnOnce() + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Pin",
        Some((id, pinned).to_variant()),
        move |_| done(),
    );
}

/// Delete an item; `done` runs after the answer (also on failure).
pub fn clipboard_delete(id: u64, done: impl FnOnce() + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Delete",
        Some((id,).to_variant()),
        move |_| done(),
    );
}

/// Delete every item that is not pinned; `done` runs after the answer (also on failure).
pub fn clipboard_clear(done: impl FnOnce() + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Clear",
        Some((true,).to_variant()),
        move |_| done(),
    );
}

/// Make an item the most recent one and put its content on the clipboard of this session;
/// `done` runs after that (also when deskd does not answer).
pub fn clipboard_activate(id: u64, done: impl FnOnce() + 'static) {
    try_call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Activate",
        Some((id,).to_variant()),
        move |reply| {
            if let Some((mime, data)) = reply.and_then(|r| r.get::<(String, Vec<u8>)>())
                && let Some(display) = gtk::gdk::Display::default()
            {
                let clipboard = display.clipboard();
                if mime.starts_with("text/") {
                    clipboard.set_text(&String::from_utf8_lossy(&data));
                } else {
                    match gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(data)) {
                        Ok(texture) => clipboard.set_texture(&texture),
                        Err(e) => eprintln!("desk-shell: clipboard item {id}: {e}"),
                    }
                }
            }
            done();
        },
    );
}

/// The output devices (`None`: deskd is not running).
pub fn mixer_outputs(done: impl FnOnce(Option<Vec<MixerDevice>>) + 'static) {
    try_call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "ListDevices",
        Some(("output",).to_variant()),
        move |reply| done(reply.as_ref().and_then(mixer_devices)),
    );
}

/// The applications that play sound now (`None`: deskd is not running).
pub fn mixer_playing_apps(done: impl FnOnce(Option<Vec<MixerApp>>) + 'static) {
    try_call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "ListApps",
        Some((false,).to_variant()),
        move |reply| done(reply.as_ref().and_then(mixer_apps)),
    );
}

/// Volume of a device (`Mixer.SetVolume`).
pub fn set_device_volume(id: u32, volume: f64) {
    let args = (id, volume).to_variant();
    call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "SetVolume",
        Some(args),
        |_| {},
    );
}

/// Mute of a device (`Mixer.SetMute`).
pub fn set_device_mute(id: u32, mute: bool) {
    let args = (id, mute).to_variant();
    call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "SetMute",
        Some(args),
        |_| {},
    );
}

/// Volume of every stream of an application (`Mixer.SetAppVolume`).
pub fn set_app_volume(key: &str, volume: f64) {
    let args = (key, volume).to_variant();
    call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "SetAppVolume",
        Some(args),
        |_| {},
    );
}

/// Mute of every stream of an application (`Mixer.SetAppMute`).
pub fn set_app_mute(key: &str, mute: bool) {
    let args = (key, mute).to_variant();
    call(
        proto::MIXER_PATH,
        proto::MIXER_IFACE,
        "SetAppMute",
        Some(args),
        |_| {},
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_replies_are_parsed() {
        let items: Vec<ListItem> = vec![
            (
                7,
                "text".into(),
                "hello".into(),
                "text/plain".into(),
                String::new(),
                false,
                1,
                5,
            ),
            (
                3,
                "image".into(),
                "PNG".into(),
                "image/png".into(),
                "/tmp/a.png".into(),
                true,
                2,
                9,
            ),
        ];
        let rows = clip_rows(&(items,).to_variant()).unwrap();
        assert_eq!(
            rows,
            [
                ClipRow {
                    id: 7,
                    kind: "text".into(),
                    text: "hello".into(),
                    pinned: false,
                },
                ClipRow {
                    id: 3,
                    kind: "image".into(),
                    text: "PNG".into(),
                    pinned: true,
                },
            ]
        );
        assert!(clip_rows(&("wrong",).to_variant()).is_none());
    }

    #[test]
    fn mixer_replies_are_parsed() {
        let devices: Vec<DeviceItem> = vec![
            (
                40,
                "alsa_output.usb".into(),
                "Headset".into(),
                "audio-headset".into(),
                "usb".into(),
                0.5,
                false,
                false,
            ),
            (
                41,
                "alsa_output.pci".into(),
                "Speakers".into(),
                "audio-speakers".into(),
                "pci".into(),
                0.8,
                true,
                true,
            ),
        ];
        let devices = mixer_devices(&(devices,).to_variant()).unwrap();
        assert_eq!(devices.len(), 2);
        let main = default_device(&devices).unwrap();
        assert_eq!((main.id, main.description.as_str()), (41, "Speakers"));
        assert!(main.muted);
        // without a default, the first device; without devices, none
        assert_eq!(default_device(&devices[..1]).unwrap().id, 40);
        assert!(default_device(&[]).is_none());
        assert!(mixer_devices(&("wrong",).to_variant()).is_none());

        let apps: Vec<AppItem> = vec![(
            "firefox".into(),
            "Firefox".into(),
            "firefox".into(),
            "Video".into(),
            0.25,
            false,
            true,
            41,
            vec![7, 8],
        )];
        assert_eq!(
            mixer_apps(&(apps,).to_variant()).unwrap(),
            [MixerApp {
                key: "firefox".into(),
                name: "Firefox".into(),
                icon: "firefox".into(),
                volume: 0.25,
                muted: false,
            }]
        );
        assert!(mixer_apps(&("wrong",).to_variant()).is_none());
    }

    #[test]
    fn volumes_show_as_percent_and_icons() {
        assert_eq!(volume_percent(0.5), 50);
        assert_eq!(volume_percent(1.0), 100);
        assert_eq!(volume_percent(0.666), 67);
        // out of range and broken values stay in 0..=150
        assert_eq!(volume_percent(9.0), 150);
        assert_eq!(volume_percent(-1.0), 0);
        assert_eq!(volume_percent(f64::NAN), 0);
        assert_eq!(volume_icon(0.0, false), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.9, true), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.2, false), "audio-volume-low-symbolic");
        assert_eq!(volume_icon(0.5, false), "audio-volume-medium-symbolic");
        assert_eq!(volume_icon(1.2, false), "audio-volume-high-symbolic");
    }

    #[test]
    fn status_replies_are_read() {
        let off = (
            "",
            "",
            "",
            "",
            0_i64,
            0_i64,
            "",
            0_i64,
            Vec::<String>::new(),
        )
            .to_variant();
        assert_eq!(focus_active(&off), Some(false));
        let on = (
            "dnd",
            "Do not disturb",
            "",
            "manual",
            0_i64,
            0_i64,
            "",
            0_i64,
            Vec::<String>::new(),
        )
            .to_variant();
        assert_eq!(focus_active(&on), Some(true));
        assert_eq!(focus_active(&(7_u32,).to_variant()), None);
        assert_eq!(focus_active(&7_u32.to_variant()), None);

        let dark = ("auto", "dark", "", "", false, false).to_variant();
        assert_eq!(scheme_is_dark(&dark), Some(true));
        let light = ("manual", "light", "", "", false, false).to_variant();
        assert_eq!(scheme_is_dark(&light), Some(false));
        assert_eq!(scheme_is_dark(&("x",).to_variant()), None);

        assert_eq!(
            property_bool(&(true.to_variant(),).to_variant()),
            Some(true)
        );
        assert_eq!(property_bool(&("s".to_variant(),).to_variant()), None);
        assert_eq!(property_bool(&(true,).to_variant()), None);
    }

    #[test]
    fn previews_are_one_short_line() {
        assert_eq!(preview("  a\n\tb  c ", 10), "a b c");
        assert_eq!(preview("abcdefghij", 5), "abcd…");
        assert_eq!(preview("abcde", 5), "abcde");
        assert_eq!(preview("", 5), "");
    }
}
