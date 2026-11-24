// SPDX-License-Identifier: GPL-3.0-or-later
//! The Windows 11 taskbar: a 48 px layer surface along the bottom edge of every monitor
//! (`Layer::Top`, exclusive zone), with the Start button, the Search pill, the pinned
//! applications and the other running applications in the centre, and the clipboard history
//! button, the quick settings pill, the clock and the date on the right.
//!
//! The running windows come from [`crate::toplevels`] (wlr foreign toplevel management): a bar
//! under an icon shows that the application runs (short) or has the focus (long, accent colour);
//! a click brings the window forward or minimizes it, and a right click (or `Menu`,
//! `Shift+F10`) opens Restore/Minimize, Maximize/Unmaximize and Close. A taskbar is added and
//! removed with each monitor; without layer-shell (GNOME, `--windowed`) one preview window opens.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::mpsc;

use desk_search::DesktopEntry;
use desk_settings::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::clipboard_flyout::ClipboardFlyout;
use crate::layer_shell::{Edge, KeyboardMode, Layer, Placement};
use crate::notifications::{Notification, NotificationDaemon};
use crate::quick_settings::QuickSettings;
use crate::running::{self, Indicator};
use crate::sni::{SniItem, SniWatcher, TRAY_ICON_PX};
use crate::toplevels::{Command, Toplevel, Toplevels, Update};
use crate::{deskd, launch, layer_shell, pinned, start_menu::StartMenu};

/// The height of the bar.
pub const HEIGHT: i32 = 48;

const PLACEMENT: Placement = Placement {
    namespace: "desk-shell-taskbar",
    layer: Layer::Top,
    anchors: &[Edge::Left, Edge::Right, Edge::Bottom],
    margins: &[],
    exclusive_zone: true,
    keyboard: KeyboardMode::None,
};

/// `HH:MM` and `DD.MM.YYYY` of `t`.
pub fn clock_texts(t: &glib::DateTime) -> (String, String) {
    let time = t.format("%H:%M").map_or_default(String::from);
    let date = t.format("%d.%m.%Y").map_or_default(String::from);
    (time, date)
}

/// Seconds until the next minute starts (1..=60), so the clock changes on time.
pub fn secs_to_next_minute(second: i32) -> u32 {
    60 - u32::try_from(second.clamp(0, 59)).unwrap_or(0)
}

/// Give an icon-only widget a name for screen readers and a tooltip.
pub fn name_widget(w: &impl IsA<gtk::Widget>, name: &str) {
    w.set_tooltip_text(Some(name));
    w.upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label(name)]);
}

/// A theme icon name or an absolute icon path as a [`gio::Icon`].
pub fn icon_of(icon: &str) -> gio::Icon {
    if icon.starts_with('/') {
        gio::FileIcon::new(&gio::File::for_path(icon)).upcast()
    } else if icon.is_empty() {
        gio::ThemedIcon::new("application-x-executable").upcast()
    } else {
        gio::ThemedIcon::new(icon).upcast()
    }
}

fn start_button(menu: &Rc<StartMenu>) -> gtk::ToggleButton {
    let tiles = gtk::Grid::builder()
        .row_spacing(2)
        .column_spacing(2)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    for (col, row) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tile.add_css_class("desk-shell-start-tile");
        tiles.attach(&tile, col, row, 1, 1);
    }
    let button = gtk::ToggleButton::builder().child(&tiles).build();
    name_widget(&button, &tr!("Start menu"));
    let m = Rc::clone(menu);
    button.connect_clicked(move |_| m.toggle());
    menu.set_start_button(&button);
    button
}

fn search_pill(menu: &Rc<StartMenu>) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.append(&gtk::Image::from_icon_name("system-search-symbolic"));
    content.append(&gtk::Label::new(Some(&tr!("Search"))));
    let pill = gtk::Button::builder().child(&content).build();
    pill.add_css_class("desk-shell-search-pill");
    pill.update_property(&[gtk::accessible::Property::Label(&tr!("Search"))]);
    let m = Rc::clone(menu);
    pill.connect_clicked(move |_| m.show());
    pill
}

fn clipboard_button(clip: &Rc<ClipboardFlyout>) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::builder()
        .icon_name("edit-paste-symbolic")
        .build();
    name_widget(&button, &tr!("Clipboard history"));
    let c = Rc::clone(clip);
    button.connect_clicked(move |_| c.toggle());
    clip.set_button(&button);
    button
}

/// The Windows 11 system tray pill: Wi-Fi (when present), output volume, and battery (when
/// present); a click opens the quick settings.
fn quick_settings_pill(quick: &Rc<QuickSettings>) -> gtk::ToggleButton {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let wifi = gtk::Image::builder()
        .icon_name("network-wireless-symbolic")
        .visible(false)
        .build();
    let volume = gtk::Image::from_icon_name("audio-volume-high-symbolic");
    let battery = gtk::Image::builder()
        .icon_name("battery-level-100-symbolic")
        .visible(false)
        .build();
    content.append(&wifi);
    content.append(&volume);
    content.append(&battery);
    content.append(&gtk::Image::from_icon_name("emblem-system-symbolic"));
    let button = gtk::ToggleButton::builder().child(&content).build();
    button.add_css_class("desk-shell-tray-pill");
    name_widget(&button, &tr!("Quick settings"));
    let q = Rc::clone(quick);
    button.connect_clicked(move |_| q.toggle());
    quick.attach_pill(&button, &volume, &wifi, &battery);
    button
}

/// Build one compact 16 px SNI tray button with left/right/middle click handlers.
fn sni_button(sni: &Rc<SniWatcher>, item: &SniItem) -> gtk::Button {
    let image = if !item.icon_name.is_empty() {
        gtk::Image::from_gicon(&icon_of(&item.icon_name))
    } else if let Some(tex) = item.texture() {
        gtk::Image::from_paintable(Some(&tex))
    } else {
        gtk::Image::from_icon_name("application-x-executable-symbolic")
    };
    image.set_pixel_size(TRAY_ICON_PX);
    let button = gtk::Button::builder().child(&image).build();
    button.add_css_class("sni-item");
    name_widget(&button, item.display_tooltip());

    // Left-click: Activate(0, 0), or ContextMenu(0, 0) when ItemIsMenu == true
    {
        let s = Rc::clone(sni);
        let item_c = item.clone();
        button.connect_clicked(move |_| s.activate(&item_c));
    }
    // Right-click: ContextMenu(0, 0)
    {
        let gesture = gtk::GestureClick::builder()
            .button(gdk::BUTTON_SECONDARY)
            .build();
        let s = Rc::clone(sni);
        let item_c = item.clone();
        gesture.connect_pressed(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            s.context_menu(&item_c);
        });
        button.add_controller(gesture);
    }
    // Middle-click: SecondaryActivate(0, 0)
    {
        let gesture = gtk::GestureClick::builder()
            .button(gdk::BUTTON_MIDDLE)
            .build();
        let s = Rc::clone(sni);
        let item_c = item.clone();
        gesture.connect_pressed(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            s.secondary_activate(&item_c);
        });
        button.add_controller(gesture);
    }
    button
}

/// Container for visible SNI tray icons; collapses cleanly when empty.
fn sni_tray(sni: &Rc<SniWatcher>) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    container.add_css_class("desk-shell-sni-tray");
    container.set_visible(false);
    let weak_box = container.downgrade();
    let sni_ref = Rc::clone(sni);
    sni.subscribe(move |items| {
        let Some(bx) = weak_box.upgrade() else {
            return;
        };
        while let Some(child) = bx.first_child() {
            bx.remove(&child);
        }
        for item in items {
            bx.append(&sni_button(&sni_ref, item));
        }
        bx.set_visible(!items.is_empty());
    });
    container
}

/// Build one row in the notification history list inside the clock popover.
fn notif_history_row(notif: &Notification, notifications: &Rc<NotificationDaemon>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    row.add_css_class("notif-item");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let icon = if notif.app_icon.is_empty() {
        gtk::Image::from_icon_name("preferences-system-notifications-symbolic")
    } else {
        gtk::Image::from_gicon(&icon_of(&notif.app_icon))
    };
    icon.set_pixel_size(14);
    header.append(&icon);

    let app = gtk::Label::builder()
        .label(if notif.app_name.is_empty() {
            tr!("Notification")
        } else {
            notif.app_name.clone()
        })
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    app.add_css_class("dim");
    header.append(&app);

    if !notif.time.is_empty() {
        let ts = gtk::Label::new(Some(&notif.time));
        ts.add_css_class("dim");
        header.append(&ts);
    }

    let dismiss = gtk::Button::from_icon_name("window-close-symbolic");
    dismiss.add_css_class("notif-dismiss");
    name_widget(&dismiss, &tr!("Dismiss notification"));
    {
        let n = Rc::clone(notifications);
        let id = notif.id;
        dismiss.connect_clicked(move |_| n.dismiss_history_item(id));
    }
    header.append(&dismiss);
    row.append(&header);

    if !notif.summary.is_empty() {
        let summary = gtk::Label::builder()
            .label(&notif.summary)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(38)
            .build();
        summary.add_css_class("notif-summary");
        row.append(&summary);
    }

    if !notif.body.is_empty() {
        let body = gtk::Label::builder()
            .label(&notif.body)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(40)
            .lines(3)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        body.add_css_class("dim");
        row.append(&body);
    }

    row
}

/// A clock that updates at the start of every minute; a click shows the notification history and
/// a calendar. An unread dot appears when unread notifications exist.
fn clock(notifications: &Rc<NotificationDaemon>) -> gtk::MenuButton {
    let time = gtk::Label::builder().xalign(1.0).build();
    let date = gtk::Label::builder().xalign(1.0).build();
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_valign(gtk::Align::Center);
    column.append(&time);
    column.append(&date);

    let unread_dot = gtk::Box::builder()
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .visible(false)
        .can_target(false)
        .build();
    unread_dot.add_css_class("clock-unread-dot");

    let overlay = gtk::Overlay::builder().child(&column).build();
    overlay.add_overlay(&unread_dot);

    // Popover content: Notification history + Clear all + Calendar
    let pop_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
    pop_box.set_size_request(320, -1);

    let notif_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    notif_header.add_css_class("notif-header");
    let title = gtk::Label::builder()
        .label(tr!("Notifications"))
        .xalign(0.0)
        .hexpand(true)
        .build();
    title.add_css_class("section-title");
    let clear_btn = gtk::Button::builder()
        .label(tr!("Clear all"))
        .visible(false)
        .build();
    clear_btn.add_css_class("notif-clear");
    {
        let n = Rc::clone(notifications);
        clear_btn.connect_clicked(move |_| n.clear_all());
    }
    notif_header.append(&title);
    notif_header.append(&clear_btn);
    pop_box.append(&notif_header);

    let empty_label = gtk::Label::builder()
        .label(tr!("No notifications"))
        .halign(gtk::Align::Center)
        .build();
    empty_label.add_css_class("notif-empty");
    pop_box.append(&empty_label);

    let list_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(240)
        .propagate_natural_height(true)
        .child(&list_box)
        .visible(false)
        .build();
    pop_box.append(&scrolled);

    pop_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    pop_box.append(&gtk::Calendar::new());

    let popover = gtk::Popover::builder().child(&pop_box).build();
    popover.add_css_class("desk-shell-notif-popover");
    {
        let n = Rc::clone(notifications);
        popover.connect_visible_notify(move |p| {
            if p.is_visible() {
                n.mark_all_read();
            }
        });
    }

    {
        let w_dot = unread_dot.downgrade();
        let w_list = list_box.downgrade();
        let w_scrolled = scrolled.downgrade();
        let w_empty = empty_label.downgrade();
        let w_clear = clear_btn.downgrade();
        let n_ref = Rc::clone(notifications);
        notifications.subscribe(move |items, unread| {
            if let Some(dot) = w_dot.upgrade() {
                dot.set_visible(unread > 0);
            }
            let Some(list) = w_list.upgrade() else {
                return;
            };
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            for item in items {
                list.append(&notif_history_row(item, &n_ref));
            }
            let has_items = !items.is_empty();
            if let Some(s) = w_scrolled.upgrade() {
                s.set_visible(has_items);
            }
            if let Some(e) = w_empty.upgrade() {
                e.set_visible(!has_items);
            }
            if let Some(c) = w_clear.upgrade() {
                c.set_visible(has_items);
            }
        });
    }

    let button = gtk::MenuButton::builder()
        .child(&overlay)
        .popover(&popover)
        .build();
    button.add_css_class("desk-shell-clock");
    tick(&button, &time, &date);
    button
}

fn tick(button: &gtk::MenuButton, time: &gtk::Label, date: &gtk::Label) {
    let Ok(now) = glib::DateTime::now_local() else {
        return;
    };
    let (t, d) = clock_texts(&now);
    time.set_label(&t);
    date.set_label(&d);
    name_widget(button, &format!("{t} {d}"));
    let (b, tl, dl) = (button.downgrade(), time.clone(), date.clone());
    glib::timeout_add_seconds_local_once(secs_to_next_minute(now.second()), move || {
        if let Some(b) = b.upgrade() {
            tick(&b, &tl, &dl);
        }
    });
}

fn tray(
    quick: &Rc<QuickSettings>,
    clip: &Rc<ClipboardFlyout>,
    sni: &Rc<SniWatcher>,
    notifications: &Rc<NotificationDaemon>,
) -> gtk::Box {
    let tray = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tray.append(&sni_tray(sni));
    tray.append(&clipboard_button(clip));
    tray.append(&quick_settings_pill(quick));
    tray.append(&clock(notifications));
    tray
}

/// A taskbar button of an application: the icon, the bar under it, and its windows now.
struct AppButton {
    button: gtk::Button,
    indicator: gtk::Box,
    name: String,
    windows: Rc<RefCell<Vec<Toplevel>>>,
}

impl AppButton {
    /// `desktop_id`: what a click launches while no window is open.
    fn new(
        icon: &gio::Icon,
        name: &str,
        desktop_id: Option<String>,
        owner: &Weak<Taskbars>,
    ) -> AppButton {
        let indicator = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .visible(false)
            .can_target(false)
            .build();
        indicator.add_css_class("app-indicator");
        let overlay = gtk::Overlay::builder()
            .child(&gtk::Image::from_gicon(icon))
            .build();
        overlay.add_overlay(&indicator);
        let button = gtk::Button::builder().child(&overlay).build();
        button.add_css_class("pin");
        name_widget(&button, name);
        let windows: Rc<RefCell<Vec<Toplevel>>> = Rc::default();

        let (list, o, launch_id) = (Rc::clone(&windows), owner.clone(), desktop_id.clone());
        button.connect_clicked(move |_| match running::click(&list.borrow()) {
            Some(cmd) => {
                if let Some(t) = o.upgrade() {
                    t.send(cmd);
                }
            }
            None => {
                if let Some(id) = &launch_id
                    && launch::launch_app(id)
                {
                    deskd::record_launch(&format!("app:{id}"));
                }
            }
        });

        // the window / pin menu: a right click, or Menu / Shift+F10 on the focused button
        let (list, o) = (Rc::clone(&windows), owner.clone());
        let open_menu = Rc::new(move |b: &gtk::Button| {
            window_menu(b, &list.borrow(), desktop_id.as_deref(), &o);
        });
        let gesture = gtk::GestureClick::builder()
            .button(gdk::BUTTON_SECONDARY)
            .build();
        let menu = Rc::clone(&open_menu);
        gesture.connect_pressed(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            if let Some(b) = g.widget().and_then(|w| w.downcast::<gtk::Button>().ok()) {
                menu(&b);
            }
        });
        button.add_controller(gesture);
        let keys = gtk::ShortcutController::new();
        keys.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Menu|<Shift>F10"),
            Some(gtk::CallbackAction::new(move |w, _| {
                if let Some(b) = w.downcast_ref::<gtk::Button>() {
                    open_menu(b);
                }
                glib::Propagation::Stop
            })),
        ));
        button.add_controller(keys);

        AppButton {
            button,
            indicator,
            name: name.to_owned(),
            windows,
        }
    }

    /// Show `windows` (those of this application, maybe none).
    fn show(&self, windows: Vec<Toplevel>) {
        let indicator = running::indicator(&windows);
        self.indicator.set_visible(indicator != Indicator::None);
        for w in [
            self.indicator.upcast_ref::<gtk::Widget>(),
            self.button.upcast_ref(),
        ] {
            if indicator == Indicator::Active {
                w.add_css_class("active");
            } else {
                w.remove_css_class("active");
            }
        }
        name_widget(&self.button, &running::tooltip(&self.name, &windows));
        *self.windows.borrow_mut() = windows;
    }
}

/// The context menu of a taskbar button: window actions (for the focused, else newest window)
/// and Pin / Unpin when the button has a known desktop entry.
fn window_menu(
    button: &gtk::Button,
    windows: &[Toplevel],
    desktop_id: Option<&str>,
    owner: &Weak<Taskbars>,
) {
    let target = running::target(windows);
    let pin_entry = desktop_id.and_then(|id| {
        let t = owner.upgrade()?;
        let is_pinned = t.is_pinned(id);
        (is_pinned || t.has_app(id)).then(|| (id.to_owned(), is_pinned))
    });
    if target.is_none() && pin_entry.is_none() {
        return;
    }

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::builder()
        .child(&column)
        .has_arrow(false)
        .build();
    popover.add_css_class("desk-shell-window-menu");

    if let Some(w) = target {
        let (id, state) = (w.id, w.state);
        let items = [
            (
                if state.minimized {
                    tr!("Restore")
                } else {
                    tr!("Minimize")
                },
                Command::ToggleMinimize(id),
            ),
            (
                if state.maximized {
                    tr!("Unmaximize")
                } else {
                    tr!("Maximize")
                },
                Command::ToggleMaximize(id),
            ),
            (tr!("Close"), Command::Close(id)),
        ];
        for (label, cmd) in items {
            let item = gtk::Button::builder().label(label).build();
            item.add_css_class("flat");
            let (o, p) = (owner.clone(), popover.downgrade());
            item.connect_clicked(move |_| {
                if let Some(t) = o.upgrade() {
                    t.send(cmd);
                }
                if let Some(p) = p.upgrade() {
                    p.popdown();
                }
            });
            column.append(&item);
        }
    }

    if let Some((id, is_pinned)) = pin_entry {
        if target.is_some() {
            column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        let label = if is_pinned { tr!("Unpin") } else { tr!("Pin") };
        let item = gtk::Button::builder().label(label).build();
        item.add_css_class("flat");
        let (o, p) = (owner.clone(), popover.downgrade());
        item.connect_clicked(move |_| {
            if let Some(p) = p.upgrade() {
                p.popdown();
            }
            let (o, p, id) = (o.clone(), p.clone(), id.clone());
            // Run on idle so the popover unparents before rebuilding removes this button.
            glib::idle_add_local_once(move || {
                if let Some(p) = p.upgrade()
                    && p.parent().is_some()
                {
                    p.unparent();
                }
                if let Some(t) = o.upgrade() {
                    t.toggle_pin(&id);
                }
            });
        });
        column.append(&item);
    }

    popover.set_parent(button);
    popover.connect_closed(|p| {
        // not inside the signal: the popover may still be handling the click
        let p = p.clone();
        glib::idle_add_local_once(move || {
            if p.parent().is_some() {
                p.unparent();
            }
        });
    });
    popover.popup();
}

/// The taskbar of one monitor.
struct Bar {
    window: gtk::ApplicationWindow,
    /// `None`: the single preview window.
    monitor: Option<gdk::Monitor>,
    /// Container for pinned application buttons, in pin order.
    pin_box: gtk::Box,
    /// One per pinned application, in pin order.
    pins: RefCell<Vec<AppButton>>,
    /// The buttons of running applications that are not pinned.
    running: gtk::Box,
    unpinned: RefCell<Vec<(String, AppButton)>>,
}

thread_local! {
    /// The taskbars that receive the window updates (on the GTK thread).
    static CURRENT: RefCell<Weak<Taskbars>> = const { RefCell::new(Weak::new()) };
}

/// Every taskbar of the shell (one per monitor) and the running windows they show.
pub struct Taskbars {
    app: gtk::Application,
    menu: Rc<StartMenu>,
    quick: Rc<QuickSettings>,
    clip: Rc<ClipboardFlyout>,
    sni: Rc<SniWatcher>,
    notifications: Rc<NotificationDaemon>,
    pins: RefCell<Vec<DesktopEntry>>,
    /// Every installed application, for the icon and name of unpinned windows.
    apps: Vec<DesktopEntry>,
    me: Weak<Taskbars>,
    bars: RefCell<Vec<Bar>>,
    /// The open windows, in opening order.
    windows: RefCell<Vec<Toplevel>>,
    toplevels: RefCell<Option<Toplevels>>,
    updates: RefCell<Option<mpsc::Receiver<Update>>>,
}

impl Taskbars {
    #[allow(
        clippy::too_many_arguments,
        reason = "the taskbar aggregates the shell's flyouts, tray watcher and notifications"
    )]
    pub fn new(
        app: &gtk::Application,
        menu: &Rc<StartMenu>,
        quick: &Rc<QuickSettings>,
        clip: &Rc<ClipboardFlyout>,
        sni: &Rc<SniWatcher>,
        notifications: &Rc<NotificationDaemon>,
        pins: &[&DesktopEntry],
        apps: &[DesktopEntry],
    ) -> Rc<Taskbars> {
        Rc::new_cyclic(|me| Taskbars {
            app: app.clone(),
            menu: Rc::clone(menu),
            quick: Rc::clone(quick),
            clip: Rc::clone(clip),
            sni: Rc::clone(sni),
            notifications: Rc::clone(notifications),
            pins: RefCell::new(pins.iter().map(|e| (*e).clone()).collect()),
            apps: apps.to_vec(),
            me: me.clone(),
            bars: RefCell::default(),
            windows: RefCell::default(),
            toplevels: RefCell::default(),
            updates: RefCell::default(),
        })
    }

    /// Open a taskbar on every monitor (and follow monitor changes) and the running windows;
    /// without layer-shell, one preview window and no window list.
    pub fn start(self: &Rc<Self>) {
        let display = gdk::Display::default();
        match display.filter(|_| layer_shell::api().is_some()) {
            Some(display) => {
                let monitors = display.monitors();
                self.sync_monitors(&monitors);
                let me = Rc::downgrade(self);
                monitors.connect_items_changed(move |list, _, _, _| {
                    if let Some(t) = me.upgrade() {
                        t.sync_monitors(list);
                    }
                });
                self.follow_windows();
            }
            None => self.add_bar(None),
        }
    }

    /// One taskbar per monitor of `list`: add the missing ones, close those of gone monitors.
    fn sync_monitors(&self, list: &gio::ListModel) {
        let monitors: Vec<gdk::Monitor> = (0..list.n_items())
            .filter_map(|i| list.item(i).and_then(|o| o.downcast().ok()))
            .collect();
        let mut gone = Vec::new();
        self.bars.borrow_mut().retain(|b| {
            let keep = b.monitor.as_ref().is_some_and(|m| monitors.contains(m));
            if !keep {
                gone.push(b.window.clone());
            }
            keep
        });
        for window in gone {
            window.destroy();
        }
        let new: Vec<gdk::Monitor> = monitors
            .into_iter()
            .filter(|m| {
                !self
                    .bars
                    .borrow()
                    .iter()
                    .any(|b| b.monitor.as_ref() == Some(m))
            })
            .collect();
        for m in new {
            self.add_bar(Some(m));
        }
    }

    fn add_bar(&self, monitor: Option<gdk::Monitor>) {
        // Do not set `.resizable(false)`: in GTK 4 Wayland (`gdk/wayland/gdktoplevel-wayland.c`),
        // `resizable = false` sets `GDK_HINT_MAX_SIZE = natural_width`, which clamps labwc's
        // `configure(output_width, 48)` for the Left|Right|Bottom-anchored layer surface down to
        // the widgets' natural width instead of spanning the full monitor.
        let window = gtk::ApplicationWindow::builder()
            .application(&self.app)
            .title(tr!("Taskbar"))
            .build();
        let pin_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let running = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let center = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        center.append(&start_button(&self.menu));
        center.append(&search_pill(&self.menu));
        center.append(&pin_box);
        center.append(&running);
        let bar = gtk::CenterBox::new();
        bar.set_hexpand(true);
        bar.add_css_class("desk-shell-taskbar");
        bar.set_center_widget(Some(&center));
        bar.set_end_widget(Some(&tray(
            &self.quick,
            &self.clip,
            &self.sni,
            &self.notifications,
        )));
        window.set_child(Some(&bar));
        if layer_shell::place(&window, &PLACEMENT) {
            if let Some(m) = &monitor {
                layer_shell::set_monitor(&window, m);
            }
        } else {
            // a preview window on desktops without wlr-layer-shell
            window.set_default_size(1280, HEIGHT);
        }
        let bar = Bar {
            window,
            monitor,
            pin_box,
            pins: RefCell::default(),
            running,
            unpinned: RefCell::default(),
        };
        self.rebuild_bar_pins(&bar);
        self.show_windows(&bar);
        bar.window.present();
        self.bars.borrow_mut().push(bar);
    }

    /// Rebuild `bar.pin_box` and `bar.pins` from `self.pins`.
    fn rebuild_bar_pins(&self, bar: &Bar) {
        while let Some(child) = bar.pin_box.first_child() {
            bar.pin_box.remove(&child);
        }
        let new_pins: Vec<AppButton> = self
            .pins
            .borrow()
            .iter()
            .map(|e| {
                let b = AppButton::new(
                    &icon_of(&e.icon),
                    e.display_name(),
                    Some(e.id.clone()),
                    &self.me,
                );
                bar.pin_box.append(&b.button);
                b
            })
            .collect();
        *bar.pins.borrow_mut() = new_pins;
    }

    fn is_pinned(&self, desktop_id: &str) -> bool {
        self.pins.borrow().iter().any(|e| e.id == desktop_id)
    }

    fn has_app(&self, desktop_id: &str) -> bool {
        self.apps.iter().any(|e| e.id == desktop_id)
    }

    /// Pin or unpin `desktop_id`, save `pinned.json`, and refresh every monitor bar.
    fn toggle_pin(&self, desktop_id: &str) {
        let mut ids: Vec<String> = self.pins.borrow().iter().map(|e| e.id.clone()).collect();
        let added = pinned::toggle_pin(&mut ids, desktop_id);
        let new_pins: Vec<DesktopEntry> = pinned::resolve_user_pins(&ids, &self.apps)
            .into_iter()
            .cloned()
            .collect();
        if added && !new_pins.iter().any(|e| e.id == desktop_id) {
            return;
        }
        let saved_ids: Vec<String> = new_pins.iter().map(|e| e.id.clone()).collect();
        *self.pins.borrow_mut() = new_pins;
        if let Some(path) = pinned::pinned_file_path()
            && let Err(e) = pinned::save_user_pins(&path, &saved_ids)
        {
            eprintln!("desk-shell: cannot save {}: {e}", path.display());
        }
        for bar in self.bars.borrow().iter() {
            self.rebuild_bar_pins(bar);
            self.show_windows(bar);
        }
    }

    /// Start the window list; its updates arrive on the GTK thread.
    fn follow_windows(self: &Rc<Self>) {
        CURRENT.with(|c| *c.borrow_mut() = Rc::downgrade(self));
        let (tx, rx) = mpsc::channel();
        let started = Toplevels::start(move |u| {
            if tx.send(u).is_ok() {
                glib::MainContext::default().invoke(|| {
                    let current = CURRENT.with(|c| c.borrow().upgrade());
                    if let Some(t) = current {
                        t.drain_updates();
                    }
                });
            }
        });
        *self.updates.borrow_mut() = Some(rx);
        *self.toplevels.borrow_mut() = started;
    }

    fn drain_updates(&self) {
        let mut changed = false;
        if let Some(rx) = self.updates.borrow().as_ref() {
            let mut windows = self.windows.borrow_mut();
            while let Ok(u) = rx.try_recv() {
                running::apply(&mut windows, u);
                changed = true;
            }
        }
        if changed {
            for bar in self.bars.borrow().iter() {
                self.show_windows(bar);
            }
        }
    }

    fn send(&self, cmd: Command) {
        if let Some(t) = self.toplevels.borrow().as_ref() {
            t.send(cmd);
        }
    }

    /// Put the current windows on the buttons of `bar`.
    fn show_windows(&self, bar: &Bar) {
        let windows = self.windows.borrow();
        let pins = self.pins.borrow();
        let groups = running::group_entries(&pins, &windows, crate::APP_ID);
        drop(pins);
        for (button, list) in bar.pins.borrow().iter().zip(groups.pinned) {
            button.show(list);
        }
        let mut unpinned = bar.unpinned.borrow_mut();
        let same_order = unpinned.len() == groups.unpinned.len()
            && unpinned
                .iter()
                .zip(&groups.unpinned)
                .all(|((a, _), (b, _))| a == b);
        if same_order {
            for ((_, button), (_, list)) in unpinned.iter().zip(groups.unpinned) {
                button.show(list);
            }
            return;
        }
        let mut old = std::mem::take(&mut *unpinned);
        while let Some(child) = bar.running.first_child() {
            bar.running.remove(&child);
        }
        for (key, list) in groups.unpinned {
            let button = match old.iter().position(|(k, _)| *k == key) {
                Some(i) => old.swap_remove(i).1,
                None => self.unpinned_button(&list[0]),
            };
            button.show(list);
            bar.running.append(&button.button);
            unpinned.push((key, button));
        }
    }

    /// The button of an application that is not pinned: the icon and name of its desktop entry,
    /// else an icon named after the `app_id`, else a generic one.
    fn unpinned_button(&self, w: &Toplevel) -> AppButton {
        let entry = running::entry_for(&self.apps, &w.app_id);
        let icon: gio::Icon = match entry {
            Some(e) => icon_of(&e.icon),
            None if !w.app_id.is_empty() => {
                gio::ThemedIcon::from_names(&[w.app_id.as_str(), running::FALLBACK_ICON]).upcast()
            }
            None => gio::ThemedIcon::new(running::FALLBACK_ICON).upcast(),
        };
        let name = match entry {
            Some(e) => e.display_name().to_owned(),
            None if !w.app_id.is_empty() => w.app_id.clone(),
            None if !w.title.trim().is_empty() => w.title.clone(),
            None => tr!("Window"),
        };
        AppButton::new(&icon, &name, entry.map(|e| e.id.clone()), &self.me)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_shows_hours_minutes_and_the_date() {
        let t = glib::DateTime::from_local(2026, 3, 7, 9, 5, 30.0).unwrap();
        assert_eq!(
            clock_texts(&t),
            ("09:05".to_owned(), "07.03.2026".to_owned())
        );
    }

    #[test]
    fn the_clock_wakes_at_the_next_minute() {
        assert_eq!(secs_to_next_minute(0), 60);
        assert_eq!(secs_to_next_minute(59), 1);
        assert_eq!(secs_to_next_minute(30), 30);
        // leap seconds and bad values stay in range
        assert_eq!(secs_to_next_minute(60), 1);
        assert_eq!(secs_to_next_minute(-3), 60);
    }

    #[test]
    fn icons_accept_names_and_paths() {
        let named = icon_of("org.gnome.Nautilus");
        assert!(named.is::<gio::ThemedIcon>());
        assert!(icon_of("/usr/share/pixmaps/x.png").is::<gio::FileIcon>());
        let fallback = icon_of("").downcast::<gio::ThemedIcon>().unwrap();
        assert_eq!(fallback.names()[0], "application-x-executable");
    }
}
