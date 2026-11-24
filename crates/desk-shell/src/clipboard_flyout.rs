// SPDX-License-Identifier: GPL-3.0-or-later
//! The Windows 11 clipboard history flyout (Win+V, the clipboard button of the taskbar): the
//! recent items of `deskd`'s `Clipboard` engine, most recent first. A click or `Return` puts an
//! item on the clipboard of the session and closes the flyout; each item can be pinned or
//! deleted, and "Clear all" keeps the pinned ones (as on Windows). A banner says when the
//! private mode is on (nothing new is recorded).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use desk_settings::tr;
use gtk::prelude::*;

use crate::deskd::{self, ClipRow};
use crate::flyout::{self, Flyout};
use crate::layer_shell::Placement;
use crate::taskbar::name_widget;

const PLACEMENT: Placement = flyout::placement("desk-shell-clipboard");
const WIDTH: i32 = 360;
const HEIGHT: i32 = 440;
/// How many items the flyout lists.
const ITEMS: u32 = 25;
/// The longest item preview, in characters.
const PREVIEW: usize = 80;

/// The symbolic icon of an item kind.
pub fn kind_icon(kind: &str) -> &'static str {
    if kind == "image" {
        "image-x-generic-symbolic"
    } else {
        "edit-paste-symbolic"
    }
}

/// The text a row shows for an item: one short line (empty text gets a placeholder).
pub fn row_text(row: &ClipRow) -> String {
    let text = deskd::preview(&row.text, PREVIEW);
    if text.is_empty() {
        "…".to_owned()
    } else {
        text
    }
}

/// The flyout.
pub struct ClipboardFlyout {
    flyout: Rc<Flyout>,
    list: gtk::ListBox,
    status: gtk::Label,
    banner: gtk::Label,
    clear: gtk::Button,
    rows: RefCell<Vec<ClipRow>>,
    /// Bumped by every refresh: answers of an older refresh are dropped, so two quick refreshes
    /// (open, then pin) never show stale or doubled rows.
    generation: Cell<u64>,
}

impl ClipboardFlyout {
    /// Build the (hidden) flyout.
    pub fn new(app: &gtk::Application) -> Rc<ClipboardFlyout> {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.add_css_class("clip-body");
        let clip = Rc::new(ClipboardFlyout {
            flyout: Flyout::new(app, &tr!("Clipboard history"), &PLACEMENT, WIDTH, &content),
            list: gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::Single)
                .build(),
            status: gtk::Label::builder().wrap(true).visible(false).build(),
            banner: gtk::Label::builder()
                .label(tr!("Private mode is on: new items are not recorded"))
                .wrap(true)
                .xalign(0.0)
                .visible(false)
                .build(),
            clear: gtk::Button::with_label(&tr!("Clear all")),
            rows: RefCell::default(),
            generation: Cell::new(0),
        });
        clip.layout(&content);
        clip.connect();
        clip
    }

    fn layout(self: &Rc<Self>, content: &gtk::Box) {
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::builder()
            .label(tr!("Clipboard history"))
            .xalign(0.0)
            .hexpand(true)
            .build();
        title.add_css_class("section-title");
        header.append(&title);
        header.append(&self.clear);
        content.append(&header);
        self.banner.add_css_class("clip-banner");
        content.append(&self.banner);
        self.status.add_css_class("empty");
        content.append(&self.status);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&self.list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(HEIGHT)
            .max_content_height(HEIGHT)
            .vexpand(true)
            .build();
        self.list
            .update_property(&[gtk::accessible::Property::Label(&tr!("Clipboard history"))]);
        content.append(&scroller);
    }

    fn connect(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let (Some(c), Ok(i)) = (me.upgrade(), usize::try_from(row.index())) {
                c.activate(i);
            }
        });
        let me = Rc::downgrade(self);
        self.clear.connect_clicked(move |_| {
            let me = me.clone();
            deskd::clipboard_clear(move || {
                if let Some(c) = me.upgrade() {
                    c.refresh();
                }
            });
        });
    }

    /// Put item `index` on the clipboard, then close.
    fn activate(self: &Rc<Self>, index: usize) {
        let Some(row) = self.rows.borrow().get(index).cloned() else {
            return;
        };
        let me = Rc::downgrade(self);
        deskd::clipboard_activate(row.id, move || {
            if let Some(c) = me.upgrade() {
                c.flyout.hide();
            }
        });
    }

    /// Read the items and the private mode from deskd again.
    fn refresh(self: &Rc<Self>) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let me = Rc::downgrade(self);
        deskd::clipboard_private(move |on| {
            if let Some(c) = me.upgrade().filter(|c| c.generation.get() == generation) {
                c.banner.set_visible(on == Some(true));
            }
        });
        let me = Rc::downgrade(self);
        deskd::clipboard_list(ITEMS, move |rows| {
            if let Some(c) = me.upgrade().filter(|c| c.generation.get() == generation) {
                c.show_rows(rows);
            }
        });
    }

    fn show_rows(self: &Rc<Self>, rows: Option<Vec<ClipRow>>) {
        self.list.remove_all();
        let status = match rows.as_deref() {
            None => Some(tr!(
                "The clipboard history is not available: the desktop service is not running"
            )),
            Some([]) => Some(tr!("The clipboard history is empty")),
            Some(_) => None,
        };
        self.status.set_visible(status.is_some());
        self.status.set_label(status.as_deref().unwrap_or(""));
        let rows = rows.unwrap_or_default();
        self.clear.set_sensitive(rows.iter().any(|r| !r.pinned));
        for row in &rows {
            self.list.append(&self.item_row(row));
        }
        *self.rows.borrow_mut() = rows;
        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
            first.grab_focus();
        }
    }

    fn item_row(self: &Rc<Self>, row: &ClipRow) -> gtk::ListBoxRow {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.append(&gtk::Image::from_icon_name(kind_icon(&row.kind)));
        let text = row_text(row);
        let label = gtk::Label::builder()
            .label(&text)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        line.append(&label);
        if row.pinned {
            let badge = gtk::Image::from_icon_name("view-pin-symbolic");
            badge.add_css_class("clip-pin-badge");
            name_widget(&badge, &tr!("Pinned item"));
            line.append(&badge);
        }
        let pin = gtk::Button::from_icon_name("view-pin-symbolic");
        pin.add_css_class("flat");
        name_widget(&pin, &if row.pinned { tr!("Unpin") } else { tr!("Pin") });
        let (me, id, pinned) = (Rc::downgrade(self), row.id, row.pinned);
        pin.connect_clicked(move |_| {
            let me = me.clone();
            deskd::clipboard_pin(id, !pinned, move || {
                if let Some(c) = me.upgrade() {
                    c.refresh();
                }
            });
        });
        line.append(&pin);
        let delete = gtk::Button::from_icon_name("user-trash-symbolic");
        delete.add_css_class("flat");
        name_widget(&delete, &tr!("Delete"));
        let me = Rc::downgrade(self);
        delete.connect_clicked(move |_| {
            let me = me.clone();
            deskd::clipboard_delete(id, move || {
                if let Some(c) = me.upgrade() {
                    c.refresh();
                }
            });
        });
        line.append(&delete);
        let item = gtk::ListBoxRow::builder().child(&line).build();
        let name = if row.pinned {
            format!("{text}, {}", tr!("Pinned item"))
        } else {
            text
        };
        item.update_property(&[gtk::accessible::Property::Label(&name)]);
        item
    }

    /// The taskbar button opens the flyout.
    pub fn set_button(&self, button: &gtk::ToggleButton) {
        self.flyout.set_button(button);
    }

    /// Open or close (the taskbar button, `desk-shell --toggle-clipboard`).
    pub fn toggle(self: &Rc<Self>) {
        self.flyout.toggle(|| self.refresh());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, text: &str) -> ClipRow {
        ClipRow {
            id: 1,
            kind: kind.into(),
            text: text.into(),
            pinned: false,
        }
    }

    #[test]
    fn items_show_one_line_and_a_kind_icon() {
        assert_eq!(row_text(&row("text", "a\n  b")), "a b");
        assert_eq!(row_text(&row("text", " \n ")), "…");
        let long = "x".repeat(200);
        assert_eq!(row_text(&row("text", &long)).chars().count(), PREVIEW);
        assert_eq!(kind_icon("image"), "image-x-generic-symbolic");
        assert_eq!(kind_icon("text"), "edit-paste-symbolic");
    }
}
