// SPDX-License-Identifier: GPL-3.0-or-later
//! The Windows 11 taskbar: a 48 px layer surface along the bottom edge (`Layer::Top`, exclusive
//! zone), with the Start button, the Search pill and the pinned applications in the centre, and
//! the quick actions, the clock and the date on the right.

use std::rc::Rc;

use desk_search::DesktopEntry;
use desk_settings::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::layer_shell::{Edge, KeyboardMode, Layer, Placement};
use crate::{capture, deskd, launch, layer_shell, start_menu::StartMenu};

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

/// How many clipboard items the clipboard button lists.
const CLIP_ITEMS: u32 = 8;
/// The longest clipboard preview, in characters.
const CLIP_PREVIEW: usize = 48;

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

fn pin_button(entry: &DesktopEntry) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(&gtk::Image::from_gicon(&icon_of(&entry.icon)))
        .build();
    button.add_css_class("pin");
    name_widget(&button, entry.display_name());
    let id = entry.id.clone();
    button.connect_clicked(move |_| {
        if launch::launch_app(&id) {
            deskd::record_launch(&format!("app:{id}"));
        }
    });
    button
}

fn tray_button(icon: &str, name: &str, on_click: impl Fn(&gtk::Button) + 'static) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    name_widget(&button, name);
    button.connect_clicked(on_click);
    button
}

/// Fill `list` with the recent clipboard items; a click restores one.
fn fill_clipboard(popover: &gtk::Popover, list: &gtk::Box) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let (popover, list) = (popover.clone(), list.clone());
    deskd::clipboard_list(CLIP_ITEMS, move |rows| {
        if rows.is_empty() {
            list.append(&gtk::Label::new(Some(&tr!(
                "The clipboard history is empty"
            ))));
        }
        for row in rows {
            let label = gtk::Label::builder()
                .label(deskd::preview(&row.text, CLIP_PREVIEW))
                .xalign(0.0)
                .build();
            let button = gtk::Button::builder().child(&label).build();
            button.add_css_class("flat");
            let p = popover.clone();
            button.connect_clicked(move |_| {
                deskd::clipboard_activate(row.id);
                p.popdown();
            });
            list.append(&button);
        }
    });
}

fn clipboard_button() -> gtk::MenuButton {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let popover = gtk::Popover::builder().child(&list).build();
    let button = gtk::MenuButton::builder()
        .icon_name("edit-paste-symbolic")
        .popover(&popover)
        .build();
    name_widget(&button, &tr!("Clipboard history"));
    let l = list.clone();
    popover.connect_show(move |p| fill_clipboard(p, &l));
    button
}

/// A clock that updates at the start of every minute; a click shows a calendar.
fn clock() -> gtk::MenuButton {
    let time = gtk::Label::builder().xalign(1.0).build();
    let date = gtk::Label::builder().xalign(1.0).build();
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.set_valign(gtk::Align::Center);
    column.append(&time);
    column.append(&date);
    let calendar = gtk::Popover::builder().child(&gtk::Calendar::new()).build();
    let button = gtk::MenuButton::builder()
        .child(&column)
        .popover(&calendar)
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

fn center(menu: &Rc<StartMenu>, pins: &[&DesktopEntry]) -> gtk::Box {
    let center = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    center.append(&start_button(menu));
    center.append(&search_pill(menu));
    for entry in pins {
        center.append(&pin_button(entry));
    }
    center
}

fn tray() -> gtk::Box {
    let tray = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tray.append(&tray_button(
        "preferences-desktop-appearance-symbolic",
        &tr!("Desktop themes and settings"),
        |_| launch::open_settings_hub(),
    ));
    tray.append(&tray_button(
        "camera-photo-symbolic",
        &tr!("Take a screenshot"),
        |_| capture::take_screenshot(),
    ));
    tray.append(&clipboard_button());
    tray.append(&clock());
    tray
}

/// Build the taskbar window.
pub fn build(
    app: &gtk::Application,
    menu: &Rc<StartMenu>,
    pins: &[&DesktopEntry],
) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(tr!("Taskbar"))
        .resizable(false)
        .build();
    let bar = gtk::CenterBox::new();
    bar.add_css_class("desk-shell-taskbar");
    bar.set_center_widget(Some(&center(menu, pins)));
    bar.set_end_widget(Some(&tray()));
    window.set_child(Some(&bar));
    if !layer_shell::place(&window, &PLACEMENT) {
        // a preview window on desktops without wlr-layer-shell
        window.set_default_size(1280, HEIGHT);
    }
    window
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
