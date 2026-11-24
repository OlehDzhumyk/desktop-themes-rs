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
}

type ListItem = (u64, String, String, String, String, bool, i64, u64);

/// The rows of a `List` answer (`(a(tssssbxt))`); `None` for another type.
pub fn clip_rows(reply: &Variant) -> Option<Vec<ClipRow>> {
    let (items,) = reply.get::<(Vec<ListItem>,)>()?;
    Some(
        items
            .into_iter()
            .map(|(id, kind, text, ..)| ClipRow { id, kind, text })
            .collect(),
    )
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

/// The other scheme: `dark` → `light`, anything else → `dark`.
pub fn other_scheme(scheme: &str) -> &'static str {
    if scheme == "dark" { "light" } else { "dark" }
}

fn call(
    path: &'static str,
    iface: &'static str,
    method: &'static str,
    args: Option<Variant>,
    done: impl FnOnce(Variant) + 'static,
) {
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, move |bus| {
        let bus = match bus {
            Ok(b) => b,
            Err(e) => return eprintln!("desk-shell: no session bus: {e}"),
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
                Ok(v) => done(v),
                Err(e) => eprintln!("desk-shell: deskd {iface}.{method}: {e}"),
            },
        );
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

/// Switch light ↔ dark (`Appearance.Status`, then `SetScheme`).
pub fn toggle_dark_mode() {
    call(
        proto::APPEARANCE_PATH,
        proto::APPEARANCE_INTERFACE,
        "Status",
        None,
        |reply| {
            let scheme = reply.child_value(1).str().unwrap_or("").to_owned();
            call(
                proto::APPEARANCE_PATH,
                proto::APPEARANCE_INTERFACE,
                "SetScheme",
                Some((other_scheme(&scheme),).to_variant()),
                |_| {},
            );
        },
    );
}

const PROPS: &str = "org.freedesktop.DBus.Properties";

/// Flip the clipboard `Private` property.
pub fn toggle_clipboard_private() {
    call(
        proto::CLIPBOARD_PATH,
        PROPS,
        "Get",
        Some((proto::CLIPBOARD_IFACE, "Private").to_variant()),
        |reply| {
            let on = reply
                .child_value(0)
                .as_variant()
                .and_then(|v| v.get::<bool>())
                .unwrap_or(false);
            call(
                proto::CLIPBOARD_PATH,
                PROPS,
                "Set",
                Some((proto::CLIPBOARD_IFACE, "Private", (!on).to_variant()).to_variant()),
                |_| {},
            );
        },
    );
}

/// The most recent clipboard items.
pub fn clipboard_list(limit: u32, done: impl FnOnce(Vec<ClipRow>) + 'static) {
    call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "List",
        Some(("", limit).to_variant()),
        move |reply| done(clip_rows(&reply).unwrap_or_default()),
    );
}

/// Make an item the most recent one and put its content on the clipboard of this session.
pub fn clipboard_activate(id: u64) {
    call(
        proto::CLIPBOARD_PATH,
        proto::CLIPBOARD_IFACE,
        "Activate",
        Some((id,).to_variant()),
        move |reply| {
            let Some((mime, data)) = reply.get::<(String, Vec<u8>)>() else {
                return;
            };
            let Some(display) = gtk::gdk::Display::default() else {
                return;
            };
            let clipboard = display.clipboard();
            if mime.starts_with("text/") {
                clipboard.set_text(&String::from_utf8_lossy(&data));
            } else {
                match gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(data)) {
                    Ok(texture) => clipboard.set_texture(&texture),
                    Err(e) => eprintln!("desk-shell: clipboard item {id}: {e}"),
                }
            }
        },
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
                    text: "hello".into()
                },
                ClipRow {
                    id: 3,
                    kind: "image".into(),
                    text: "PNG".into()
                },
            ]
        );
        assert!(clip_rows(&("wrong",).to_variant()).is_none());
    }

    #[test]
    fn previews_are_one_short_line() {
        assert_eq!(preview("  a\n\tb  c ", 10), "a b c");
        assert_eq!(preview("abcdefghij", 5), "abcd…");
        assert_eq!(preview("abcde", 5), "abcde");
        assert_eq!(preview("", 5), "");
    }

    #[test]
    fn schemes_toggle() {
        assert_eq!(other_scheme("dark"), "light");
        assert_eq!(other_scheme("light"), "dark");
        assert_eq!(other_scheme(""), "dark");
    }
}
