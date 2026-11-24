// SPDX-License-Identifier: GPL-3.0-or-later
//! The screenshot button: an interactive capture through the XDG Desktop Portal
//! (`org.freedesktop.portal.Screenshot`), which every wlroots, KDE and GNOME desktop ships, then
//! the screenshot editor of `themes capture-edit`. No compositor-specific tool is needed.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio;
use gtk::glib::{self, Variant, VariantDict};
use gtk::prelude::*;

use crate::launch;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SCREENSHOT_IFACE: &str = "org.freedesktop.portal.Screenshot";
const REQUEST_IFACE: &str = "org.freedesktop.portal.Request";

/// The object path of the portal request for `token` (Request documentation of the portal).
pub fn request_path(unique_name: &str, token: &str) -> String {
    let sender = unique_name.trim_start_matches(':').replace('.', "_");
    format!("{PORTAL_PATH}/request/{sender}/{token}")
}

/// The saved file of a successful `Response` (`(ua{sv})`): code 0 and a `file://` `uri`.
pub fn response_path(reply: &Variant) -> Option<std::path::PathBuf> {
    if reply.type_().as_str() != "(ua{sv})" || reply.child_value(0).get::<u32>()? != 0 {
        return None;
    }
    let results = VariantDict::new(Some(&reply.child_value(1)));
    let uri = results.lookup_value("uri", None)?.get::<String>()?;
    gio::File::for_uri(&uri).path()
}

/// Ask the portal for an interactive screenshot and open it in the editor.
pub fn take_screenshot() {
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, |bus| {
        let bus = match bus {
            Ok(b) => b,
            Err(e) => return eprintln!("desk-shell: no session bus: {e}"),
        };
        let token = format!("desk_shell_{}", glib::random_int());
        let path = request_path(bus.unique_name().as_deref().unwrap_or(""), &token);
        // the subscription lives until the first response (dropping it unsubscribes)
        let sub: Rc<RefCell<Option<gio::SignalSubscription>>> = Rc::default();
        let sub2 = Rc::clone(&sub);
        let subscription = bus.subscribe_to_signal(
            Some(PORTAL),
            Some(REQUEST_IFACE),
            Some("Response"),
            Some(&path),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                drop(sub2.borrow_mut().take());
                match response_path(signal.parameters) {
                    Some(file) => {
                        let file = file.to_string_lossy().into_owned();
                        launch::spawn(&[&launch::themes_binary(), "capture-edit", &file]);
                    }
                    None => eprintln!("desk-shell: the screenshot was cancelled"),
                }
            },
        );
        *sub.borrow_mut() = Some(subscription);
        let options = VariantDict::new(None);
        options.insert("handle_token", &token);
        options.insert("interactive", true);
        bus.call(
            Some(PORTAL),
            PORTAL_PATH,
            SCREENSHOT_IFACE,
            "Screenshot",
            Some(&("", options.end()).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            move |reply| {
                if let Err(e) = reply {
                    eprintln!("desk-shell: screenshot portal: {e}");
                    drop(sub.borrow_mut().take());
                }
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_paths_follow_the_portal_rule() {
        assert_eq!(
            request_path(":1.42", "desk_shell_7"),
            "/org/freedesktop/portal/desktop/request/1_42/desk_shell_7"
        );
    }

    /// A `Response` body: `(u a{sv})`.
    fn response(code: u32, uri: Option<&str>) -> Variant {
        let results = VariantDict::new(None);
        if let Some(uri) = uri {
            results.insert("uri", uri);
        }
        Variant::tuple_from_iter([code.to_variant(), results.end()])
    }

    #[test]
    fn responses_give_the_file_only_on_success() {
        assert_eq!(
            response_path(&response(0, Some("file:///home/user/Pictures/Shot.png"))).unwrap(),
            std::path::Path::new("/home/user/Pictures/Shot.png")
        );
        // cancelled (1) or failed (2)
        assert!(response_path(&response(1, Some("file:///x.png"))).is_none());
        assert!(response_path(&response(2, None)).is_none());
        assert!(response_path(&response(0, None)).is_none());
        assert!(response_path(&("bad",).to_variant()).is_none());
    }
}
