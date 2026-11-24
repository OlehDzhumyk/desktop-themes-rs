// SPDX-License-Identifier: GPL-3.0-or-later
//! System tray StatusNotifierItem (SNI) watcher and host for `desk-shell`.
//!
//! Owns `org.kde.StatusNotifierWatcher` on the session bus without stealing an existing owner
//! (degrading silently with no error output when another watcher already runs) and exports
//! `/StatusNotifierWatcher`. Registered items are queried asynchronously over D-Bus
//! (`org.kde.StatusNotifierItem`) and rendered as compact 16 px buttons in the taskbar tray.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::glib::{self, Variant};
use gtk::prelude::*;

const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_IFACE: &str = "org.kde.StatusNotifierWatcher";
const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const DEFAULT_ITEM_PATH: &str = "/StatusNotifierItem";

const TIMEOUT_MS: i32 = 3000;
/// Target icon size (px) in the taskbar tray.
pub const TRAY_ICON_PX: i32 = 16;

const WATCHER_XML: &str = r#"
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem">
      <arg name="service" type="s" direction="in"/>
    </method>
    <method name="RegisterStatusNotifierHost">
      <arg name="service" type="s" direction="in"/>
    </method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered">
      <arg name="service" type="s"/>
    </signal>
    <signal name="StatusNotifierItemUnregistered">
      <arg name="service" type="s"/>
    </signal>
    <signal name="StatusNotifierHostRegistered"/>
  </interface>
</node>
"#;

/// A single `IconPixmap` entry from `org.kde.StatusNotifierItem` (network-order ARGB32).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pixmap {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>,
}

/// Snapshot of a registered SNI item for rendering in the taskbar tray.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SniItem {
    /// Canonical watcher ID (`<bus_name><object_path>`).
    pub id: String,
    pub bus_name: String,
    pub object_path: String,
    pub icon_name: String,
    pub icon_pixmap: Option<Pixmap>,
    pub title: String,
    pub tooltip: String,
    pub status: String,
    pub item_is_menu: bool,
}

impl SniItem {
    /// Whether this item should be shown in the taskbar tray (`Status != "Passive"`).
    pub fn is_visible(&self) -> bool {
        status_is_visible(&self.status)
    }

    /// Best tooltip / accessible label for the tray button.
    pub fn display_tooltip(&self) -> &str {
        if !self.tooltip.is_empty() {
            &self.tooltip
        } else if !self.title.is_empty() {
            &self.title
        } else {
            &self.bus_name
        }
    }

    /// Build a `gdk::MemoryTexture` from the cached ARGB32 pixmap, if present and valid.
    pub fn texture(&self) -> Option<gdk::MemoryTexture> {
        let pm = self.icon_pixmap.as_ref()?;
        let rgba = argb32_to_rgba(&pm.data)?;
        let expected = usize::try_from(pm.width)
            .ok()?
            .checked_mul(usize::try_from(pm.height).ok()?)?
            .checked_mul(4)?;
        if rgba.len() != expected || expected == 0 {
            return None;
        }
        let stride = usize::try_from(pm.width).ok()?.checked_mul(4)?;
        let bytes = glib::Bytes::from_owned(rgba);
        Some(gdk::MemoryTexture::new(
            pm.width,
            pm.height,
            gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            stride,
        ))
    }
}

/// Parse the `service` argument of `RegisterStatusNotifierItem` together with the D-Bus `sender`.
///
/// Clients pass either an object path (`/StatusNotifierItem`), a bus name
/// (`org.kde.StatusNotifierItem-123-1` or `:1.42`), or `<bus_name><object_path>`.
pub fn parse_item_service(service: &str, sender: Option<&str>) -> Option<(String, String)> {
    let service = service.trim();
    if service.is_empty() {
        return None;
    }
    if service.starts_with('/') {
        let bus = sender?.trim();
        if bus.is_empty() {
            return None;
        }
        return Some((bus.to_owned(), service.to_owned()));
    }
    if let Some((bus, rest)) = service.split_once('/') {
        if bus.is_empty() || rest.is_empty() {
            return None;
        }
        return Some((bus.to_owned(), format!("/{rest}")));
    }
    Some((service.to_owned(), DEFAULT_ITEM_PATH.to_owned()))
}

/// Canonical identifier used in `RegisteredStatusNotifierItems` and watcher signals.
pub fn canonical_item_id(bus_name: &str, object_path: &str) -> String {
    format!("{bus_name}{object_path}")
}

/// Whether an SNI `Status` string should be displayed (`Passive` items are hidden).
pub fn status_is_visible(status: &str) -> bool {
    !status.trim().eq_ignore_ascii_case("passive")
}

/// Convert network-order (big-endian) ARGB32 pixel bytes to RGBA8888.
///
/// Returns `None` if `argb.len()` is not a multiple of 4.
pub fn argb32_to_rgba(argb: &[u8]) -> Option<Vec<u8>> {
    if !argb.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(argb.len());
    for px in argb.as_chunks::<4>().0 {
        out.extend_from_slice(&[px[1], px[2], px[3], px[0]]);
    }
    Some(out)
}

/// Select the valid pixmap whose dimension is closest to `target_px`.
///
/// On ties in distance, prefers a pixmap at least `target_px` wide/tall so downscaling wins over
/// upscaling.
pub fn select_best_pixmap(pixmaps: &[Pixmap], target_px: i32) -> Option<&Pixmap> {
    pixmaps
        .iter()
        .filter(|p| {
            let Ok(w) = usize::try_from(p.width) else {
                return false;
            };
            let Ok(h) = usize::try_from(p.height) else {
                return false;
            };
            w > 0
                && h > 0
                && w.checked_mul(h)
                    .and_then(|n| n.checked_mul(4))
                    .is_some_and(|expected| p.data.len() == expected)
        })
        .min_by_key(|p| {
            let size = p.width.max(p.height);
            let dist = (size - target_px).abs();
            // Prefer >= target_px on equal distance (0 beats 1).
            let smaller_penalty = i32::from(size < target_px);
            (dist, smaller_penalty, size)
        })
}

/// Combine SNI `Title` and `ToolTip` `(title, description)` into a single tooltip string.
pub fn format_tooltip(title: &str, tooltip_title: &str, tooltip_desc: &str) -> String {
    let tt = tooltip_title.trim();
    let td = tooltip_desc.trim();
    match (tt.is_empty(), td.is_empty()) {
        (false, false) if tt != td => format!("{tt}\n{td}"),
        (false, _) => tt.to_owned(),
        (true, false) => td.to_owned(),
        (true, true) => title.trim().to_owned(),
    }
}

/// Parse an `IconPixmap` variant (`a(iiay)`) into [`Pixmap`] entries.
fn parse_pixmaps(v: &Variant) -> Vec<Pixmap> {
    let Some(raw) = v.get::<Vec<(i32, i32, Vec<u8>)>>() else {
        return Vec::new();
    };
    raw.into_iter()
        .map(|(width, height, data)| Pixmap {
            width,
            height,
            data,
        })
        .collect()
}

/// Parse an SNI `ToolTip` variant (`(sa(iiay)ss)`) into `(title, description)`.
fn parse_tooltip(v: &Variant) -> Option<(String, String)> {
    if !v.is_container() || v.n_children() < 4 {
        return None;
    }
    let title = v.child_value(2).str()?.to_owned();
    let desc = v.child_value(3).str()?.to_owned();
    Some((title, desc))
}

/// Apply a `Properties.GetAll` reply (`(a{sv})`) onto an [`SniItem`].
fn apply_properties(item: &mut SniItem, reply: &Variant) {
    if !reply.is_container() || reply.n_children() == 0 {
        return;
    }
    let dict = glib::VariantDict::new(Some(&reply.child_value(0)));
    if let Ok(Some(icon)) = dict.lookup::<String>("IconName") {
        item.icon_name = icon;
    }
    if let Some(pm_var) = dict.lookup_value("IconPixmap", None) {
        let list = parse_pixmaps(&pm_var);
        item.icon_pixmap = select_best_pixmap(&list, TRAY_ICON_PX).cloned();
    }
    if let Ok(Some(title)) = dict.lookup::<String>("Title") {
        item.title = title;
    }
    if let Ok(Some(status)) = dict.lookup::<String>("Status") {
        item.status = status;
    }
    if let Ok(Some(is_menu)) = dict.lookup::<bool>("ItemIsMenu") {
        item.item_is_menu = is_menu;
    }
    if let Some(tt_var) = dict.lookup_value("ToolTip", None)
        && let Some((tt_title, tt_desc)) = parse_tooltip(&tt_var)
    {
        item.tooltip = format_tooltip(&item.title, &tt_title, &tt_desc);
    } else if item.tooltip.is_empty() && !item.title.is_empty() {
        item.tooltip = item.title.clone();
    }
}

struct RegisteredEntry {
    item: SniItem,
    /// Unique D-Bus name of the caller that registered this item (for disconnect cleanup).
    owner: Option<String>,
    /// Signal subscriptions on the item (`NewIcon`, `NewTitle`, `NewToolTip`, `NewStatus`).
    _subs: Vec<gio::SignalSubscription>,
}

type Listener = Box<dyn Fn(&[SniItem])>;

#[derive(Default)]
struct WatcherState {
    bus: Option<gio::DBusConnection>,
    registration_id: Option<gio::RegistrationId>,
    name_watch: Option<gio::SignalSubscription>,
    items: Vec<RegisteredEntry>,
    hosts: HashSet<String>,
    listeners: Vec<Listener>,
}

/// Live `org.kde.StatusNotifierWatcher` and host owned by `desk-shell`.
pub struct SniWatcher {
    state: RefCell<WatcherState>,
    _owner_id: RefCell<Option<gio::OwnerId>>,
}

impl SniWatcher {
    /// Start owning `org.kde.StatusNotifierWatcher` on the session bus without stealing.
    pub fn new() -> Rc<Self> {
        let watcher = Rc::new(Self {
            state: RefCell::default(),
            _owner_id: RefCell::default(),
        });
        let w_acquired = Rc::downgrade(&watcher);
        let w_lost = Rc::downgrade(&watcher);
        let owner_id = gio::bus_own_name(
            gio::BusType::Session,
            WATCHER_NAME,
            gio::BusNameOwnerFlags::NONE,
            move |bus, _| {
                if let Some(w) = w_acquired.upgrade() {
                    w.export_on(&bus);
                }
            },
            |_, _| {},
            move |_, _| {
                // Another watcher owns the name (or bus closed): degrade silently.
                if let Some(w) = w_lost.upgrade() {
                    w.teardown();
                }
            },
        );
        *watcher._owner_id.borrow_mut() = Some(owner_id);
        watcher
    }

    /// Snapshot of currently visible SNI items.
    pub fn visible_items(&self) -> Vec<SniItem> {
        self.state
            .borrow()
            .items
            .iter()
            .filter(|e| e.item.is_visible())
            .map(|e| e.item.clone())
            .collect()
    }

    /// Register a callback invoked whenever the visible SNI item list changes.
    pub fn subscribe(&self, cb: impl Fn(&[SniItem]) + 'static) {
        let snapshot = self.visible_items();
        cb(&snapshot);
        self.state.borrow_mut().listeners.push(Box::new(cb));
    }

    /// Call `Activate(0, 0)` on an SNI item (or `ContextMenu(0, 0)` when `ItemIsMenu`).
    pub fn activate(&self, item: &SniItem) {
        let method = if item.item_is_menu {
            "ContextMenu"
        } else {
            "Activate"
        };
        self.call_item_method(&item.bus_name, &item.object_path, method);
    }

    /// Call `ContextMenu(0, 0)` on an SNI item.
    pub fn context_menu(&self, item: &SniItem) {
        self.call_item_method(&item.bus_name, &item.object_path, "ContextMenu");
    }

    /// Call `SecondaryActivate(0, 0)` on an SNI item.
    pub fn secondary_activate(&self, item: &SniItem) {
        self.call_item_method(&item.bus_name, &item.object_path, "SecondaryActivate");
    }

    fn call_item_method(&self, bus_name: &str, object_path: &str, method: &str) {
        let Some(bus) = self.state.borrow().bus.clone() else {
            return;
        };
        bus.call(
            Some(bus_name),
            object_path,
            ITEM_IFACE,
            method,
            Some(&(0_i32, 0_i32).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
            |_| {},
        );
    }

    fn export_on(self: &Rc<Self>, bus: &gio::DBusConnection) {
        let Ok(node) = gio::DBusNodeInfo::for_xml(WATCHER_XML) else {
            return;
        };
        let Some(iface) = node.lookup_interface(WATCHER_IFACE) else {
            return;
        };

        let w_method = Rc::downgrade(self);
        let w_prop = Rc::downgrade(self);
        let reg = bus
            .register_object(WATCHER_PATH, &iface)
            .method_call(move |_, sender, _, _, method, params, invocation| {
                let Some(w) = w_method.upgrade() else {
                    invocation.return_value(None);
                    return;
                };
                match method {
                    "RegisterStatusNotifierItem" => {
                        let service = params.get::<(String,)>().map_or_default(|(s,)| s);
                        w.register_item(&service, sender);
                        invocation.return_value(None);
                    }
                    "RegisterStatusNotifierHost" => {
                        let service = params.get::<(String,)>().map_or_default(|(s,)| s);
                        w.register_host(&service);
                        invocation.return_value(None);
                    }
                    _ => {
                        invocation.return_error(gio::DBusError::UnknownMethod, "Unknown method");
                    }
                }
            })
            .property(move |_, _, _, _, prop| {
                let Some(w) = w_prop.upgrade() else {
                    return 0_i32.to_variant();
                };
                match prop {
                    "RegisteredStatusNotifierItems" => {
                        let ids: Vec<String> = w
                            .state
                            .borrow()
                            .items
                            .iter()
                            .map(|e| e.item.id.clone())
                            .collect();
                        ids.to_variant()
                    }
                    "IsStatusNotifierHostRegistered" => true.to_variant(),
                    "ProtocolVersion" => 0_i32.to_variant(),
                    _ => "".to_variant(),
                }
            })
            .build();

        let Ok(reg_id) = reg else {
            return;
        };

        let w_owner = Rc::downgrade(self);
        let name_watch = bus.subscribe_to_signal(
            Some(DBUS_NAME),
            Some(DBUS_NAME),
            Some("NameOwnerChanged"),
            Some(DBUS_PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |sig| {
                if let Some((name, old_owner, new_owner)) =
                    sig.parameters.get::<(String, String, String)>()
                    && new_owner.is_empty()
                    && let Some(w) = w_owner.upgrade()
                {
                    w.on_owner_vanished(&name, &old_owner);
                }
            },
        );

        let mut st = self.state.borrow_mut();
        st.bus = Some(bus.clone());
        st.registration_id = Some(reg_id);
        st.name_watch = Some(name_watch);
    }

    fn teardown(&self) {
        let mut st = self.state.borrow_mut();
        let reg_id = st.registration_id.take();
        if let (Some(bus), Some(reg_id)) = (st.bus.as_ref(), reg_id) {
            let _ = bus.unregister_object(reg_id);
        }
        st.name_watch = None;
        let had_items = !st.items.is_empty();
        st.items.clear();
        st.hosts.clear();
        st.bus = None;
        drop(st);
        if had_items {
            self.notify_listeners();
        }
    }

    fn register_host(&self, service: &str) {
        if service.trim().is_empty() {
            return;
        }
        let bus = {
            let mut st = self.state.borrow_mut();
            st.hosts.insert(service.to_owned());
            st.bus.clone()
        };
        if let Some(bus) = bus {
            let _ = bus.emit_signal(
                None,
                WATCHER_PATH,
                WATCHER_IFACE,
                "StatusNotifierHostRegistered",
                None,
            );
        }
    }

    fn register_item(self: &Rc<Self>, service: &str, sender: Option<&str>) {
        let Some((bus_name, object_path)) = parse_item_service(service, sender) else {
            return;
        };
        let id = canonical_item_id(&bus_name, &object_path);
        let bus = {
            let st = self.state.borrow();
            if st.items.iter().any(|e| e.item.id == id) {
                // Already registered: refresh its properties in case it re-registered.
                drop(st);
                self.refresh_item(&id, &bus_name, &object_path);
                return;
            }
            st.bus.clone()
        };
        let Some(bus) = bus else {
            return;
        };

        let mut subs = Vec::new();
        for signal in ["NewIcon", "NewTitle", "NewToolTip", "NewStatus"] {
            let w = Rc::downgrade(self);
            let id_c = id.clone();
            let bn_c = bus_name.clone();
            let op_c = object_path.clone();
            let sub = bus.subscribe_to_signal(
                Some(&bus_name),
                Some(ITEM_IFACE),
                Some(signal),
                Some(&object_path),
                None,
                gio::DBusSignalFlags::NONE,
                move |_| {
                    if let Some(w) = w.upgrade() {
                        w.refresh_item(&id_c, &bn_c, &op_c);
                    }
                },
            );
            subs.push(sub);
        }

        let item = SniItem {
            id: id.clone(),
            bus_name: bus_name.clone(),
            object_path: object_path.clone(),
            icon_name: String::new(),
            icon_pixmap: None,
            title: String::new(),
            tooltip: String::new(),
            status: "Active".to_owned(),
            item_is_menu: false,
        };
        self.state.borrow_mut().items.push(RegisteredEntry {
            item,
            owner: sender.map(ToOwned::to_owned),
            _subs: subs,
        });

        let _ = bus.emit_signal(
            None,
            WATCHER_PATH,
            WATCHER_IFACE,
            "StatusNotifierItemRegistered",
            Some(&(id.as_str(),).to_variant()),
        );

        self.refresh_item(&id, &bus_name, &object_path);
    }

    fn refresh_item(self: &Rc<Self>, id: &str, bus_name: &str, object_path: &str) {
        let Some(bus) = self.state.borrow().bus.clone() else {
            return;
        };
        let w = Rc::downgrade(self);
        let id_owned = id.to_owned();
        bus.call(
            Some(bus_name),
            object_path,
            PROPS_IFACE,
            "GetAll",
            Some(&(ITEM_IFACE,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
            move |reply| {
                let Ok(v) = reply else { return };
                let Some(w) = w.upgrade() else { return };
                let updated = {
                    let mut st = w.state.borrow_mut();
                    if let Some(entry) = st.items.iter_mut().find(|e| e.item.id == id_owned) {
                        apply_properties(&mut entry.item, &v);
                        true
                    } else {
                        false
                    }
                };
                if updated {
                    w.notify_listeners();
                }
            },
        );
    }

    fn on_owner_vanished(&self, name: &str, old_owner: &str) {
        let (removed_ids, bus) = {
            let mut st = self.state.borrow_mut();
            let mut removed = Vec::new();
            st.items.retain(|e| {
                let gone = e.item.bus_name == name
                    || e.owner.as_deref() == Some(name)
                    || (!old_owner.is_empty()
                        && (e.item.bus_name == old_owner || e.owner.as_deref() == Some(old_owner)));
                if gone {
                    removed.push(e.item.id.clone());
                }
                !gone
            });
            (removed, st.bus.clone())
        };
        if removed_ids.is_empty() {
            return;
        }
        if let Some(bus) = bus {
            for id in &removed_ids {
                let _ = bus.emit_signal(
                    None,
                    WATCHER_PATH,
                    WATCHER_IFACE,
                    "StatusNotifierItemUnregistered",
                    Some(&(id.as_str(),).to_variant()),
                );
            }
        }
        self.notify_listeners();
    }

    fn notify_listeners(&self) {
        let snapshot = self.visible_items();
        // Borrow immutably while invoking listeners; listeners only update GTK widgets.
        let st = self.state.borrow();
        for cb in &st.listeners {
            cb(&snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_item_service_handles_paths_bus_names_and_combined_ids() {
        assert_eq!(
            parse_item_service("/StatusNotifierItem", Some(":1.42")),
            Some((":1.42".into(), "/StatusNotifierItem".into()))
        );
        assert_eq!(
            parse_item_service("/org/ayatana/NotificationItem/app", Some(":1.99")),
            Some((":1.99".into(), "/org/ayatana/NotificationItem/app".into()))
        );
        // Object path without sender cannot be resolved
        assert_eq!(parse_item_service("/StatusNotifierItem", None), None);
        assert_eq!(parse_item_service("/StatusNotifierItem", Some("")), None);

        // Well-known or unique bus name defaults to /StatusNotifierItem
        assert_eq!(
            parse_item_service("org.kde.StatusNotifierItem-123-1", Some(":1.5")),
            Some((
                "org.kde.StatusNotifierItem-123-1".into(),
                "/StatusNotifierItem".into()
            ))
        );
        assert_eq!(
            parse_item_service(":1.77", None),
            Some((":1.77".into(), "/StatusNotifierItem".into()))
        );

        // Combined <bus>/<path>
        assert_eq!(
            parse_item_service(":1.77/Custom/Path", None),
            Some((":1.77".into(), "/Custom/Path".into()))
        );

        // Empty or malformed
        assert_eq!(parse_item_service("", Some(":1.1")), None);
        assert_eq!(parse_item_service("   ", Some(":1.1")), None);
        assert_eq!(parse_item_service("bus/", Some(":1.1")), None);
    }

    #[test]
    fn argb32_to_rgba_converts_network_byte_order_pixels() {
        // Two pixels: opaque red (FF, FF, 00, 00) and half-alpha blue (80, 00, 00, FF)
        let argb = [0xFF, 0xFF, 0x00, 0x00, 0x80, 0x00, 0x00, 0xFF];
        let rgba = argb32_to_rgba(&argb).unwrap();
        assert_eq!(rgba, vec![0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF, 0x80]);

        // Empty input succeeds
        assert_eq!(argb32_to_rgba(&[]), Some(Vec::new()));

        // Non-multiple-of-4 is rejected
        assert_eq!(argb32_to_rgba(&[1, 2, 3]), None);
    }

    #[test]
    fn select_best_pixmap_picks_closest_valid_size_preferring_downscale_on_ties() {
        let mk = |w: i32, h: i32| Pixmap {
            width: w,
            height: h,
            data: vec![
                0u8;
                usize::try_from(w.max(0)).unwrap_or(0)
                    * usize::try_from(h.max(0)).unwrap_or(0)
                    * 4
            ],
        };
        let invalid_len = Pixmap {
            width: 16,
            height: 16,
            data: vec![0u8; 10],
        };
        let invalid_dim = Pixmap {
            width: 0,
            height: 16,
            data: Vec::new(),
        };
        let p12 = mk(12, 12);
        let p20 = mk(20, 20);
        let p32 = mk(32, 32);

        // 12 and 20 are both distance 4 from 16; 20 wins (prefer downscaling over upscaling)
        let list = [
            invalid_len,
            invalid_dim,
            p12.clone(),
            p32.clone(),
            p20.clone(),
        ];
        let best = select_best_pixmap(&list, 16).unwrap();
        assert_eq!((best.width, best.height), (20, 20));

        // Exact match wins
        let p16 = mk(16, 16);
        let list2 = [p12, p16, p20, p32];
        let best2 = select_best_pixmap(&list2, 16).unwrap();
        assert_eq!((best2.width, best2.height), (16, 16));

        // All invalid -> None
        assert!(select_best_pixmap(&[], 16).is_none());
    }

    #[test]
    fn status_visibility_hides_only_passive_items() {
        assert!(status_is_visible("Active"));
        assert!(status_is_visible("NeedsAttention"));
        assert!(status_is_visible(""));
        assert!(!status_is_visible("Passive"));
        assert!(!status_is_visible("passive"));
        assert!(!status_is_visible(" PASSIVE "));
    }

    #[test]
    fn format_tooltip_combines_title_and_description_with_fallbacks() {
        assert_eq!(format_tooltip("App", "Title", "Desc"), "Title\nDesc");
        assert_eq!(format_tooltip("App", "Same", "Same"), "Same");
        assert_eq!(format_tooltip("App", "Only title", ""), "Only title");
        assert_eq!(format_tooltip("App", "", "Only desc"), "Only desc");
        assert_eq!(format_tooltip("Fallback App", "", ""), "Fallback App");
    }

    #[test]
    fn watcher_introspection_xml_is_valid() {
        let node = gio::DBusNodeInfo::for_xml(WATCHER_XML).unwrap();
        let iface = node.lookup_interface(WATCHER_IFACE).unwrap();
        assert!(iface.lookup_method("RegisterStatusNotifierItem").is_some());
        assert!(iface.lookup_method("RegisterStatusNotifierHost").is_some());
        assert!(
            iface
                .lookup_property("RegisteredStatusNotifierItems")
                .is_some()
        );
        assert!(
            iface
                .lookup_property("IsStatusNotifierHostRegistered")
                .is_some()
        );
        assert!(iface.lookup_property("ProtocolVersion").is_some());
        assert!(
            iface
                .lookup_signal("StatusNotifierItemRegistered")
                .is_some()
        );
        assert!(
            iface
                .lookup_signal("StatusNotifierItemUnregistered")
                .is_some()
        );
        assert!(
            iface
                .lookup_signal("StatusNotifierHostRegistered")
                .is_some()
        );
    }
}
