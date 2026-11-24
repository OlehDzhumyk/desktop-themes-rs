// SPDX-License-Identifier: GPL-3.0-or-later
//! Built-in `org.freedesktop.Notifications` daemon for `desk-shell`.
//!
//! Owns `org.freedesktop.Notifications` on the session bus without stealing (degrading silently
//! when `mako`, `dunst`, or another daemon already owns it), renders Windows 11 Fluent
//! `Layer::Overlay` toast popups in the bottom-right corner above the 48 px taskbar, respects
//! `deskd`'s `Desk.Focus` Do Not Disturb mode (suppressing non-critical toasts while still
//! recording them in history and letting critical notifications through), and feeds the
//! Notification History section inside the Clock/Calendar flyout.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use desk_settings::tr;
use gtk::gio;
use gtk::glib::{self, Variant};
use gtk::prelude::*;

use crate::deskd;
use crate::layer_shell::{self, Edge, KeyboardMode, Layer, Placement};
use crate::taskbar::{icon_of, name_widget};

const NOTIF_BUS_NAME: &str = "org.freedesktop.Notifications";
const NOTIF_OBJECT_PATH: &str = "/org/freedesktop/Notifications";
const NOTIF_IFACE: &str = "org.freedesktop.Notifications";

/// Capabilities returned by `GetCapabilities`.
pub const CAPABILITIES: [&str; 5] = [
    "actions",
    "body",
    "body-markup",
    "icon-static",
    "persistence",
];

/// Freedesktop `NotificationClosed` reason: expired.
pub const REASON_EXPIRED: u32 = 1;
/// Freedesktop `NotificationClosed` reason: dismissed by the user.
pub const REASON_DISMISSED: u32 = 2;
/// Freedesktop `NotificationClosed` reason: closed by `CloseNotification`.
pub const REASON_CLOSED: u32 = 3;

/// Urgency levels in the freedesktop notifications specification.
#[cfg(test)]
pub const URGENCY_LOW: u8 = 0;
pub const URGENCY_NORMAL: u8 = 1;
pub const URGENCY_CRITICAL: u8 = 2;

/// Default auto-dismiss timeout (ms) when `expire_timeout < 0`.
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;
/// Minimum clamped auto-dismiss timeout (ms) when `expire_timeout > 0`.
pub const MIN_TIMEOUT_MS: u64 = 1_500;
/// Maximum clamped auto-dismiss timeout (ms) when `expire_timeout > 0`.
pub const MAX_TIMEOUT_MS: u64 = 30_000;
/// Maximum number of notifications retained in history.
pub const MAX_HISTORY: usize = 50;
/// Maximum number of simultaneous toast cards shown on screen.
const MAX_VISIBLE_TOASTS: usize = 4;

const TOAST_PLACEMENT: Placement = Placement {
    namespace: "desk-shell-notifications",
    layer: Layer::Overlay,
    anchors: &[Edge::Bottom, Edge::Right],
    // 12 px above the 48 px taskbar and 12 px from the right screen edge.
    margins: &[(Edge::Bottom, 60), (Edge::Right, 12)],
    exclusive_zone: false,
    keyboard: KeyboardMode::None,
};

const NOTIF_XML: &str = r#"
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="GetCapabilities">
      <arg name="capabilities" type="as" direction="out"/>
    </method>
    <method name="GetServerInformation">
      <arg name="name" type="s" direction="out"/>
      <arg name="vendor" type="s" direction="out"/>
      <arg name="version" type="s" direction="out"/>
      <arg name="spec_version" type="s" direction="out"/>
    </method>
    <method name="Notify">
      <arg name="app_name" type="s" direction="in"/>
      <arg name="replaces_id" type="u" direction="in"/>
      <arg name="app_icon" type="s" direction="in"/>
      <arg name="summary" type="s" direction="in"/>
      <arg name="body" type="s" direction="in"/>
      <arg name="actions" type="as" direction="in"/>
      <arg name="hints" type="a{sv}" direction="in"/>
      <arg name="expire_timeout" type="i" direction="in"/>
      <arg name="id" type="u" direction="out"/>
    </method>
    <method name="CloseNotification">
      <arg name="id" type="u" direction="in"/>
    </method>
    <signal name="NotificationClosed">
      <arg name="id" type="u"/>
      <arg name="reason" type="u"/>
    </signal>
    <signal name="ActionInvoked">
      <arg name="id" type="u"/>
      <arg name="action_key" type="s"/>
    </signal>
  </interface>
</node>
"#;

/// A stored or displayed notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    /// Action `(key, label)` pairs.
    pub actions: Vec<(String, String)>,
    pub urgency: u8,
    pub transient: bool,
    /// `HH:MM` timestamp formatted when received.
    pub time: String,
}

/// Input parameters to [`NotificationStore::push`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingNotification {
    pub app_name: String,
    pub replaces_id: u32,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub urgency: u8,
    pub transient: bool,
    pub expire_timeout: i32,
    pub time: String,
}

/// Outcome of handling an incoming `Notify` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushOutcome {
    pub notification: Notification,
    /// Whether a toast popup should be displayed (false when suppressed by DND).
    pub show_toast: bool,
    /// Auto-dismiss duration in ms, or `None` when persistent until dismissed.
    pub timeout_ms: Option<u64>,
}

/// Clamp a freedesktop `expire_timeout` (ms) into an auto-dismiss duration.
///
/// * `0` -> `None` (never auto-expires)
/// * `< 0` -> server default (`DEFAULT_TIMEOUT_MS`), or `None` for critical notifications
/// * `> 0` -> clamped to `MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS`
pub fn clamp_timeout_ms(expire_timeout: i32, urgency: u8) -> Option<u64> {
    if expire_timeout == 0 {
        return None;
    }
    if expire_timeout < 0 {
        return if urgency >= URGENCY_CRITICAL {
            None
        } else {
            Some(DEFAULT_TIMEOUT_MS)
        };
    }
    let ms = u64::try_from(expire_timeout).unwrap_or(MAX_TIMEOUT_MS);
    Some(ms.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS))
}

/// Parse flat `actions` (`[key0, label0, key1, label1, ...]`) into `(key, label)` pairs.
pub fn parse_actions(raw: &[String]) -> Vec<(String, String)> {
    raw.as_chunks::<2>()
        .0
        .iter()
        .filter(|pair| !pair[0].is_empty() && !pair[1].is_empty())
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect()
}

/// Extract `(urgency, transient)` from a freedesktop `hints` `a{sv}` variant.
fn parse_hints(hints: &Variant) -> (u8, bool) {
    let dict = glib::VariantDict::new(Some(hints));
    let urgency = dict
        .lookup::<u8>("urgency")
        .ok()
        .flatten()
        .or_else(|| {
            dict.lookup::<i32>("urgency")
                .ok()
                .flatten()
                .and_then(|v| u8::try_from(v).ok())
        })
        .or_else(|| {
            dict.lookup::<u32>("urgency")
                .ok()
                .flatten()
                .and_then(|v| u8::try_from(v).ok())
        })
        .unwrap_or(URGENCY_NORMAL)
        .min(URGENCY_CRITICAL);
    let transient = dict
        .lookup::<bool>("transient")
        .ok()
        .flatten()
        .unwrap_or(false);
    (urgency, transient)
}

/// Pure notification history and ID allocator (unit-tested without D-Bus or GTK).
#[derive(Clone, Debug)]
pub struct NotificationStore {
    next_id: u32,
    history: Vec<Notification>,
    unread: usize,
    max_history: usize,
}

impl Default for NotificationStore {
    fn default() -> Self {
        Self::with_capacity(MAX_HISTORY)
    }
}

impl NotificationStore {
    pub fn with_capacity(max_history: usize) -> Self {
        Self {
            next_id: 1,
            history: Vec::new(),
            unread: 0,
            max_history: max_history.max(1),
        }
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// Record an incoming notification, honouring `replaces_id`, `transient`, `max_history`,
    /// and Do Not Disturb (`dnd_active`).
    pub fn push(&mut self, incoming: IncomingNotification, dnd_active: bool) -> PushOutcome {
        let id = if incoming.replaces_id > 0 {
            if incoming.replaces_id >= self.next_id {
                self.next_id = incoming.replaces_id.wrapping_add(1).max(1);
            }
            incoming.replaces_id
        } else {
            self.alloc_id()
        };

        let notif = Notification {
            id,
            app_name: incoming.app_name,
            app_icon: incoming.app_icon,
            summary: incoming.summary,
            body: incoming.body,
            actions: incoming.actions,
            urgency: incoming.urgency,
            transient: incoming.transient,
            time: incoming.time,
        };

        if !notif.transient {
            if let Some(pos) = self.history.iter().position(|n| n.id == id) {
                self.history.remove(pos);
            } else {
                self.unread = self.unread.saturating_add(1);
            }
            self.history.insert(0, notif.clone());
            if self.history.len() > self.max_history {
                self.history.truncate(self.max_history);
                self.unread = self.unread.min(self.history.len());
            }
        }

        let show_toast = !dnd_active || notif.urgency >= URGENCY_CRITICAL;
        let timeout_ms = clamp_timeout_ms(incoming.expire_timeout, notif.urgency);

        PushOutcome {
            notification: notif,
            show_toast,
            timeout_ms,
        }
    }

    /// Remove a single notification from history; returns `true` if it was present.
    pub fn dismiss(&mut self, id: u32) -> bool {
        let Some(pos) = self.history.iter().position(|n| n.id == id) else {
            return false;
        };
        self.history.remove(pos);
        self.unread = self.unread.min(self.history.len());
        true
    }

    /// Clear all stored notifications and return their IDs.
    pub fn clear(&mut self) -> Vec<u32> {
        let ids = self.history.iter().map(|n| n.id).collect();
        self.history.clear();
        self.unread = 0;
        ids
    }

    /// Mark all notifications as read (called when the user opens the notification history).
    pub fn mark_all_read(&mut self) {
        self.unread = 0;
    }

    pub fn history(&self) -> &[Notification] {
        &self.history
    }

    pub fn unread_count(&self) -> usize {
        self.unread
    }
}

struct ActiveToast {
    widget: gtk::Box,
    timer: Option<glib::SourceId>,
}

type HistoryListener = Box<dyn Fn(&[Notification], usize)>;

#[derive(Default)]
struct DaemonState {
    bus: Option<gio::DBusConnection>,
    registration_id: Option<gio::RegistrationId>,
    store: NotificationStore,
    active_toasts: HashMap<u32, ActiveToast>,
    toast_order: Vec<u32>,
    listeners: Vec<HistoryListener>,
}

/// Built-in `org.freedesktop.Notifications` daemon, toast overlay, and history store.
pub struct NotificationDaemon {
    toast_window: gtk::Window,
    toast_box: gtk::Box,
    placed: bool,
    dnd_active: Cell<bool>,
    state: RefCell<DaemonState>,
    _owner_id: RefCell<Option<gio::OwnerId>>,
}

impl NotificationDaemon {
    /// Start the daemon on the session bus (without stealing) and prepare the toast overlay.
    pub fn new(app: &gtk::Application) -> Rc<Self> {
        let toast_window = gtk::Window::builder()
            .application(app)
            .title(tr!("Notifications"))
            .resizable(false)
            .focusable(false)
            .can_focus(false)
            .decorated(false)
            .build();
        let toast_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        toast_box.add_css_class("desk-shell-toast-stack");
        toast_window.set_child(Some(&toast_box));
        let placed = layer_shell::place(&toast_window, &TOAST_PLACEMENT);

        let daemon = Rc::new(Self {
            toast_window,
            toast_box,
            placed,
            dnd_active: Cell::new(false),
            state: RefCell::default(),
            _owner_id: RefCell::default(),
        });

        daemon.follow_focus();

        let d_acquired = Rc::downgrade(&daemon);
        let d_lost = Rc::downgrade(&daemon);
        let owner_id = gio::bus_own_name(
            gio::BusType::Session,
            NOTIF_BUS_NAME,
            gio::BusNameOwnerFlags::NONE,
            move |bus, _| {
                if let Some(d) = d_acquired.upgrade() {
                    d.export_on(&bus);
                }
            },
            |_, _| {},
            move |_, _| {
                // Another notification daemon (mako/dunst/etc.) owns the name: degrade silently.
                if let Some(d) = d_lost.upgrade() {
                    d.teardown();
                }
            },
        );
        *daemon._owner_id.borrow_mut() = Some(owner_id);
        daemon
    }

    fn follow_focus(self: &Rc<Self>) {
        let sync = {
            let me = Rc::downgrade(self);
            Rc::new(move || {
                let me2 = me.clone();
                deskd::focus_status(move |active| {
                    if let (Some(d), Some(active)) = (me2.upgrade(), active) {
                        d.dnd_active.set(active);
                    }
                });
            })
        };
        sync();
        deskd::watch_focus(move || sync());
    }

    /// Register a listener invoked whenever notification history or unread count changes.
    pub fn subscribe(&self, cb: impl Fn(&[Notification], usize) + 'static) {
        let (items, unread) = {
            let st = self.state.borrow();
            (st.store.history().to_vec(), st.store.unread_count())
        };
        cb(&items, unread);
        self.state.borrow_mut().listeners.push(Box::new(cb));
    }

    /// Mark all notifications as read (clears the taskbar clock unread dot).
    pub fn mark_all_read(&self) {
        let changed = {
            let mut st = self.state.borrow_mut();
            let had_unread = st.store.unread_count() > 0;
            st.store.mark_all_read();
            had_unread
        };
        if changed {
            self.notify_listeners();
        }
    }

    /// Dismiss a single notification from history (and any active toast) by user action.
    pub fn dismiss_history_item(&self, id: u32) {
        self.remove_toast(id);
        let removed = self.state.borrow_mut().store.dismiss(id);
        if removed {
            self.emit_closed(id, REASON_DISMISSED);
            self.notify_listeners();
        }
    }

    /// Clear all notifications from history and active toasts.
    pub fn clear_all(&self) {
        let toast_ids: Vec<u32> = self.state.borrow().toast_order.clone();
        for id in toast_ids {
            self.remove_toast(id);
        }
        let ids = self.state.borrow_mut().store.clear();
        for id in &ids {
            self.emit_closed(*id, REASON_DISMISSED);
        }
        if !ids.is_empty() {
            self.notify_listeners();
        }
    }

    fn export_on(self: &Rc<Self>, bus: &gio::DBusConnection) {
        let Ok(node) = gio::DBusNodeInfo::for_xml(NOTIF_XML) else {
            return;
        };
        let Some(iface) = node.lookup_interface(NOTIF_IFACE) else {
            return;
        };

        let d_method = Rc::downgrade(self);
        let reg = bus
            .register_object(NOTIF_OBJECT_PATH, &iface)
            .method_call(move |_, _, _, _, method, params, invocation| {
                let Some(d) = d_method.upgrade() else {
                    invocation.return_value(None);
                    return;
                };
                match method {
                    "GetCapabilities" => {
                        let caps: Vec<&str> = CAPABILITIES.to_vec();
                        invocation.return_value(Some(&(caps,).to_variant()));
                    }
                    "GetServerInformation" => {
                        let info = (
                            "desk-shell",
                            "desktop-themes-rs",
                            env!("CARGO_PKG_VERSION"),
                            "1.2",
                        );
                        invocation.return_value(Some(&info.to_variant()));
                    }
                    "Notify" => {
                        let id = d.handle_notify(&params);
                        invocation.return_value(Some(&(id,).to_variant()));
                    }
                    "CloseNotification" => {
                        if let Some((id,)) = params.get::<(u32,)>() {
                            d.close_notification(id);
                        }
                        invocation.return_value(None);
                    }
                    _ => {
                        invocation.return_error(gio::DBusError::UnknownMethod, "Unknown method");
                    }
                }
            })
            .build();

        let Ok(reg_id) = reg else {
            return;
        };
        let mut st = self.state.borrow_mut();
        st.bus = Some(bus.clone());
        st.registration_id = Some(reg_id);
    }

    fn teardown(&self) {
        let toast_ids: Vec<u32> = self.state.borrow().toast_order.clone();
        for id in toast_ids {
            self.remove_toast(id);
        }
        let mut st = self.state.borrow_mut();
        let reg_id = st.registration_id.take();
        if let (Some(bus), Some(reg_id)) = (st.bus.as_ref(), reg_id) {
            let _ = bus.unregister_object(reg_id);
        }
        st.bus = None;
    }

    fn handle_notify(self: &Rc<Self>, params: &Variant) -> u32 {
        if !params.is_container() || params.n_children() < 8 {
            return 0;
        }
        let app_name = params.child_value(0).str().unwrap_or_default().to_owned();
        let replaces_id = params.child_value(1).get::<u32>().unwrap_or(0);
        let app_icon = params.child_value(2).str().unwrap_or_default().to_owned();
        let summary = params.child_value(3).str().unwrap_or_default().to_owned();
        let body = params.child_value(4).str().unwrap_or_default().to_owned();
        let raw_actions = params
            .child_value(5)
            .get::<Vec<String>>()
            .unwrap_or_default();
        let hints = params.child_value(6);
        let expire_timeout = params.child_value(7).get::<i32>().unwrap_or(-1);

        let (urgency, transient) = parse_hints(&hints);
        let actions = parse_actions(&raw_actions);
        let time = glib::DateTime::now_local()
            .ok()
            .and_then(|t| t.format("%H:%M").ok())
            .map_or_default(String::from);

        let incoming = IncomingNotification {
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            urgency,
            transient,
            expire_timeout,
            time,
        };

        let outcome = self
            .state
            .borrow_mut()
            .store
            .push(incoming, self.dnd_active.get());

        if outcome.show_toast && self.placed {
            self.show_toast(&outcome.notification, outcome.timeout_ms);
        }
        self.notify_listeners();
        outcome.notification.id
    }

    fn close_notification(&self, id: u32) {
        let had_toast = self.remove_toast(id);
        let had_history = self.state.borrow_mut().store.dismiss(id);
        if had_toast || had_history {
            self.emit_closed(id, REASON_CLOSED);
            self.notify_listeners();
        }
    }

    fn show_toast(self: &Rc<Self>, notif: &Notification, timeout_ms: Option<u64>) {
        // Replace an existing toast with the same ID if present.
        self.remove_toast(notif.id);

        // Enforce maximum simultaneous visible toasts by evicting the oldest.
        let oldest = {
            let st = self.state.borrow();
            if st.toast_order.len() >= MAX_VISIBLE_TOASTS {
                st.toast_order.first().copied()
            } else {
                None
            }
        };
        if let Some(old_id) = oldest {
            self.remove_toast(old_id);
        }

        let card = self.build_toast_card(notif);
        self.toast_box.append(&card);

        let timer = timeout_ms.map(|ms| {
            let me = Rc::downgrade(self);
            let id = notif.id;
            glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || {
                if let Some(d) = me.upgrade() {
                    if let Some(entry) = d.state.borrow_mut().active_toasts.get_mut(&id) {
                        // Timer has fired: do not attempt to remove its SourceId again.
                        entry.timer = None;
                    }
                    if d.remove_toast(id) {
                        d.emit_closed(id, REASON_EXPIRED);
                    }
                }
            })
        });

        {
            let mut st = self.state.borrow_mut();
            st.toast_order.push(notif.id);
            st.active_toasts.insert(
                notif.id,
                ActiveToast {
                    widget: card,
                    timer,
                },
            );
        }
        self.toast_window.set_visible(true);
    }

    fn build_toast_card(self: &Rc<Self>, notif: &Notification) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
        card.add_css_class("desk-shell-toast");
        if notif.urgency >= URGENCY_CRITICAL {
            card.add_css_class("toast-critical");
        }
        card.set_size_request(340, -1);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("toast-header");
        let icon = if notif.app_icon.is_empty() {
            gtk::Image::from_icon_name("preferences-system-notifications-symbolic")
        } else {
            gtk::Image::from_gicon(&icon_of(&notif.app_icon))
        };
        icon.set_pixel_size(16);
        header.append(&icon);

        let app_label = gtk::Label::builder()
            .label(if notif.app_name.is_empty() {
                tr!("Notification")
            } else {
                notif.app_name.clone()
            })
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        app_label.add_css_class("toast-app");
        header.append(&app_label);

        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("toast-close");
        name_widget(&close, &tr!("Dismiss notification"));
        {
            let me = Rc::downgrade(self);
            let id = notif.id;
            close.connect_clicked(move |_| {
                if let Some(d) = me.upgrade()
                    && d.remove_toast(id)
                {
                    d.emit_closed(id, REASON_DISMISSED);
                }
            });
        }
        header.append(&close);
        card.append(&header);

        if !notif.summary.is_empty() {
            let summary = gtk::Label::builder()
                .label(&notif.summary)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .max_width_chars(42)
                .build();
            summary.add_css_class("toast-summary");
            card.append(&summary);
        }

        if !notif.body.is_empty() {
            let body = gtk::Label::builder()
                .label(&notif.body)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .max_width_chars(46)
                .lines(4)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            body.add_css_class("toast-body");
            card.append(&body);
        }

        if !notif.actions.is_empty() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.add_css_class("toast-actions");
            for (key, label) in &notif.actions {
                let btn = gtk::Button::builder().label(label).hexpand(true).build();
                let me = Rc::downgrade(self);
                let id = notif.id;
                let key = key.clone();
                btn.connect_clicked(move |_| {
                    if let Some(d) = me.upgrade() {
                        d.emit_action_invoked(id, &key);
                        if d.remove_toast(id) {
                            d.emit_closed(id, REASON_DISMISSED);
                        }
                    }
                });
                row.append(&btn);
            }
            card.append(&row);
        }

        card
    }

    fn remove_toast(&self, id: u32) -> bool {
        let removed = {
            let mut st = self.state.borrow_mut();
            st.toast_order.retain(|x| *x != id);
            st.active_toasts.remove(&id)
        };
        let Some(mut toast) = removed else {
            return false;
        };
        if let Some(timer) = toast.timer.take() {
            timer.remove();
        }
        self.toast_box.remove(&toast.widget);
        if self.state.borrow().active_toasts.is_empty() {
            self.toast_window.set_visible(false);
        }
        true
    }

    fn emit_closed(&self, id: u32, reason: u32) {
        if let Some(bus) = self.state.borrow().bus.as_ref() {
            let _ = bus.emit_signal(
                None,
                NOTIF_OBJECT_PATH,
                NOTIF_IFACE,
                "NotificationClosed",
                Some(&(id, reason).to_variant()),
            );
        }
    }

    fn emit_action_invoked(&self, id: u32, action_key: &str) {
        if let Some(bus) = self.state.borrow().bus.as_ref() {
            let _ = bus.emit_signal(
                None,
                NOTIF_OBJECT_PATH,
                NOTIF_IFACE,
                "ActionInvoked",
                Some(&(id, action_key).to_variant()),
            );
        }
    }

    fn notify_listeners(&self) {
        let (items, unread) = {
            let st = self.state.borrow();
            (st.store.history().to_vec(), st.store.unread_count())
        };
        let st = self.state.borrow();
        for cb in &st.listeners {
            cb(&items, unread);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming(summary: &str) -> IncomingNotification {
        IncomingNotification {
            app_name: "TestApp".into(),
            replaces_id: 0,
            app_icon: String::new(),
            summary: summary.into(),
            body: "Body text".into(),
            actions: Vec::new(),
            urgency: URGENCY_NORMAL,
            transient: false,
            expire_timeout: -1,
            time: "12:00".into(),
        }
    }

    #[test]
    fn replaces_id_updates_in_place_without_inflating_unread() {
        let mut store = NotificationStore::default();
        let first = store.push(incoming("First"), false);
        assert_eq!(first.notification.id, 1);
        assert_eq!(store.history().len(), 1);
        assert_eq!(store.unread_count(), 1);

        let mut upd = incoming("Updated");
        upd.replaces_id = 1;
        let second = store.push(upd, false);
        assert_eq!(second.notification.id, 1);
        assert_eq!(store.history().len(), 1);
        assert_eq!(store.history()[0].summary, "Updated");
        assert_eq!(store.unread_count(), 1);

        // Next allocated id stays strictly greater than any replaced id
        let mut jump = incoming("Explicit 10");
        jump.replaces_id = 10;
        assert_eq!(store.push(jump, false).notification.id, 10);
        assert_eq!(store.push(incoming("After 10"), false).notification.id, 11);
    }

    #[test]
    fn transient_notifications_skip_history_and_unread_count() {
        let mut store = NotificationStore::default();
        let mut t = incoming("Volume");
        t.transient = true;
        let out = store.push(t, false);
        assert!(out.show_toast);
        assert!(store.history().is_empty());
        assert_eq!(store.unread_count(), 0);
    }

    #[test]
    fn history_is_capped_and_keeps_newest_first() {
        let mut store = NotificationStore::with_capacity(3);
        for i in 1..=5 {
            store.push(incoming(&format!("Msg {i}")), false);
        }
        let summaries: Vec<&str> = store.history().iter().map(|n| n.summary.as_str()).collect();
        assert_eq!(summaries, vec!["Msg 5", "Msg 4", "Msg 3"]);
        assert_eq!(store.unread_count(), 3);
    }

    #[test]
    fn dnd_suppresses_non_critical_toasts_but_allows_critical() {
        let mut store = NotificationStore::default();

        let low = {
            let mut n = incoming("Low");
            n.urgency = URGENCY_LOW;
            store.push(n, true)
        };
        assert!(!low.show_toast);

        let normal = store.push(incoming("Normal"), true);
        assert!(!normal.show_toast);

        let critical = {
            let mut n = incoming("Battery critical");
            n.urgency = URGENCY_CRITICAL;
            store.push(n, true)
        };
        assert!(critical.show_toast);

        // All three are still recorded in history
        assert_eq!(store.history().len(), 3);
        assert_eq!(store.unread_count(), 3);
    }

    #[test]
    fn timeout_clamping_handles_zero_negative_and_bounds() {
        assert_eq!(clamp_timeout_ms(0, URGENCY_NORMAL), None);
        assert_eq!(clamp_timeout_ms(0, URGENCY_CRITICAL), None);
        assert_eq!(clamp_timeout_ms(-1, URGENCY_LOW), Some(DEFAULT_TIMEOUT_MS));
        assert_eq!(
            clamp_timeout_ms(-1, URGENCY_NORMAL),
            Some(DEFAULT_TIMEOUT_MS)
        );
        assert_eq!(clamp_timeout_ms(-1, URGENCY_CRITICAL), None);
        assert_eq!(clamp_timeout_ms(100, URGENCY_NORMAL), Some(MIN_TIMEOUT_MS));
        assert_eq!(clamp_timeout_ms(4000, URGENCY_NORMAL), Some(4000));
        assert_eq!(
            clamp_timeout_ms(999_999, URGENCY_NORMAL),
            Some(MAX_TIMEOUT_MS)
        );
    }

    #[test]
    fn dismiss_mark_read_and_clear_update_store_and_unread() {
        let mut store = NotificationStore::default();
        let a = store.push(incoming("A"), false).notification.id;
        let b = store.push(incoming("B"), false).notification.id;
        assert_eq!(store.unread_count(), 2);

        assert!(store.dismiss(a));
        assert!(!store.dismiss(999));
        assert_eq!(store.history().len(), 1);
        assert_eq!(store.unread_count(), 1);

        store.mark_all_read();
        assert_eq!(store.unread_count(), 0);

        let cleared = store.clear();
        assert_eq!(cleared, vec![b]);
        assert!(store.history().is_empty());
        assert_eq!(store.unread_count(), 0);
    }

    #[test]
    fn parse_actions_pairs_keys_and_labels_ignoring_incomplete() {
        let raw: Vec<String> = ["default", "Open", "reply", "Reply", "orphan"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            parse_actions(&raw),
            vec![
                ("default".into(), "Open".into()),
                ("reply".into(), "Reply".into())
            ]
        );
    }

    #[test]
    fn notification_xml_and_placement_are_valid() {
        let node = gio::DBusNodeInfo::for_xml(NOTIF_XML).unwrap();
        let iface = node.lookup_interface(NOTIF_IFACE).unwrap();
        assert!(iface.lookup_method("GetCapabilities").is_some());
        assert!(iface.lookup_method("GetServerInformation").is_some());
        assert!(iface.lookup_method("Notify").is_some());
        assert!(iface.lookup_method("CloseNotification").is_some());
        assert!(iface.lookup_signal("NotificationClosed").is_some());
        assert!(iface.lookup_signal("ActionInvoked").is_some());

        assert_eq!(TOAST_PLACEMENT.layer, Layer::Overlay);
        assert_eq!(TOAST_PLACEMENT.keyboard, KeyboardMode::None);
        assert_eq!(TOAST_PLACEMENT.anchors, &[Edge::Bottom, Edge::Right]);
        assert!(TOAST_PLACEMENT.margins.contains(&(Edge::Bottom, 60)));
        const { assert!(!TOAST_PLACEMENT.exclusive_zone) };
    }
}
