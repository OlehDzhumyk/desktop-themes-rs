// SPDX-License-Identifier: GPL-3.0-or-later
//! The Windows 11 emoji panel (Win+. , the switch button of the clipboard history): the
//! emoji of `deskd`'s `Emoji` engine in a grid, by category (recently used first) or by a search
//! in English and Ukrainian. A click puts the emoji on the clipboard of the session (deskd
//! applies the default skin tone and records it in the recents) and closes the panel; `Esc` and
//! a focus loss close it too. Without deskd the panel says that emoji are not available.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use desk_settings::tr;
use gtk::prelude::*;

use crate::deskd::{self, EmojiRow};
use crate::flyout::{self, Flyout};
use crate::launch;
use crate::layer_shell::Placement;
use crate::taskbar::name_widget;

const PLACEMENT: Placement = flyout::placement("desk-shell-emoji");
const WIDTH: i32 = 360;
const HEIGHT: i32 = 320;
/// Emoji per grid line.
const COLUMNS: u32 = 8;
/// The most search results shown.
const SEARCH_LIMIT: u32 = 120;

/// The category tabs: deskd's category id and the icon (GTK ships the `emoji-*` icons).
pub const CATEGORIES: [(&str, &str); 9] = [
    (RECENTS, "emoji-recent-symbolic"),
    ("smileys", "emoji-people-symbolic"),
    ("people", "emoji-body-symbolic"),
    ("animals", "emoji-nature-symbolic"),
    ("food", "emoji-food-symbolic"),
    ("travel", "emoji-travel-symbolic"),
    ("activities", "emoji-activities-symbolic"),
    ("objects", "emoji-objects-symbolic"),
    ("symbols", "emoji-symbols-symbolic"),
];

/// The pseudo-category of the recently used emoji (`Emoji.Recents`).
pub const RECENTS: &str = "recents";

/// The name of a category tab.
fn category_name(id: &str) -> String {
    match id {
        RECENTS => tr!("Recently used"),
        "smileys" => tr!("Smileys"),
        "people" => tr!("People"),
        "animals" => tr!("Animals and nature"),
        "food" => tr!("Food and drink"),
        "travel" => tr!("Travel and places"),
        "activities" => tr!("Activities"),
        "objects" => tr!("Objects"),
        _ => tr!("Symbols"),
    }
}

/// The `lang` of `Emoji.Search` for an interface language: names in Ukrainian or English, else
/// deskd's own choice (`""`).
pub fn emoji_lang(ui_language: &str) -> &'static str {
    let l = ui_language.to_ascii_lowercase();
    if l.starts_with("uk") {
        "uk"
    } else if l.starts_with("en") {
        "en"
    } else {
        ""
    }
}

/// What the grid shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    Category(&'static str),
    Search(String),
}

/// The query for the search text and the selected tab: text wins over the tab.
pub fn query_for(text: &str, category: &'static str) -> Query {
    let text = text.trim();
    if text.is_empty() {
        Query::Category(category)
    } else {
        Query::Search(text.to_owned())
    }
}

/// Why the grid is empty, or `None` when it has emoji.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Empty {
    Unavailable,
    NoRecents,
    NoMatch,
}

pub fn empty_reason(rows: Option<&[EmojiRow]>, query: &Query) -> Option<Empty> {
    match (rows, query) {
        (None, _) => Some(Empty::Unavailable),
        (Some([]), Query::Category(RECENTS)) => Some(Empty::NoRecents),
        (Some([]), _) => Some(Empty::NoMatch),
        (Some(_), _) => None,
    }
}

/// The panel.
pub struct EmojiFlyout {
    flyout: Rc<Flyout>,
    search: gtk::SearchEntry,
    tabs: Vec<(&'static str, gtk::ToggleButton)>,
    grid: gtk::FlowBox,
    status: gtk::Label,
    switch: gtk::Button,
    category: Cell<&'static str>,
    rows: RefCell<Vec<EmojiRow>>,
    /// Bumped by every load: answers of an older load are dropped.
    generation: Cell<u64>,
    /// What the switch button of the header opens (the clipboard history).
    on_switch: RefCell<Option<Box<dyn Fn()>>>,
}

impl EmojiFlyout {
    /// Build the (hidden) panel.
    pub fn new(app: &gtk::Application) -> Rc<EmojiFlyout> {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.add_css_class("emoji-body");
        let tabs = CATEGORIES
            .iter()
            .map(|(id, icon)| {
                let tab = gtk::ToggleButton::builder().icon_name(*icon).build();
                tab.add_css_class("emoji-tab");
                name_widget(&tab, &category_name(id));
                (*id, tab)
            })
            .collect::<Vec<_>>();
        if let Some((_, first)) = tabs.first() {
            for (_, tab) in &tabs[1..] {
                tab.set_group(Some(first));
            }
        }
        let emoji = Rc::new(EmojiFlyout {
            flyout: Flyout::new(app, &tr!("Emoji"), &PLACEMENT, WIDTH, &content),
            search: gtk::SearchEntry::builder()
                .placeholder_text(tr!("Search emoji"))
                .hexpand(true)
                .build(),
            tabs,
            grid: gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .homogeneous(true)
                .min_children_per_line(COLUMNS)
                .max_children_per_line(COLUMNS)
                .valign(gtk::Align::Start)
                .build(),
            status: gtk::Label::builder().wrap(true).visible(false).build(),
            switch: gtk::Button::from_icon_name("edit-paste-symbolic"),
            category: Cell::new(RECENTS),
            rows: RefCell::default(),
            generation: Cell::new(0),
            on_switch: RefCell::default(),
        });
        emoji.layout(&content);
        emoji.connect();
        emoji
    }

    fn layout(&self, content: &gtk::Box) {
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::builder()
            .label(tr!("Emoji"))
            .xalign(0.0)
            .hexpand(true)
            .build();
        title.add_css_class("section-title");
        header.append(&title);
        name_widget(&self.switch, &tr!("Clipboard history"));
        header.append(&self.switch);
        content.append(&header);
        self.search
            .update_property(&[gtk::accessible::Property::Label(&tr!("Search emoji"))]);
        content.append(&self.search);
        let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tabs.add_css_class("emoji-tabs");
        tabs.set_homogeneous(true);
        for (_, tab) in &self.tabs {
            tabs.append(tab);
        }
        content.append(&tabs);
        self.status.add_css_class("empty");
        content.append(&self.status);
        self.grid
            .update_property(&[gtk::accessible::Property::Label(&tr!("Emoji"))]);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&self.grid)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(HEIGHT)
            .max_content_height(HEIGHT)
            .vexpand(true)
            .build();
        content.append(&scroller);
    }

    fn connect(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        self.search.connect_search_changed(move |_| {
            if let Some(e) = me.upgrade() {
                e.load();
            }
        });
        // Return picks the first emoji of the grid
        let me = Rc::downgrade(self);
        self.search.connect_activate(move |_| {
            if let Some(e) = me.upgrade() {
                e.pick(0);
            }
        });
        for (id, tab) in &self.tabs {
            let (me, id) = (Rc::downgrade(self), *id);
            tab.connect_toggled(move |t| {
                if let Some(e) = me.upgrade().filter(|_| t.is_active()) {
                    e.category.set(id);
                    if !e.search.text().is_empty() {
                        // clearing the search loads the category (search-changed)
                        e.search.set_text("");
                    } else {
                        e.load();
                    }
                }
            });
        }
        let me = Rc::downgrade(self);
        self.switch.connect_clicked(move |_| {
            if let Some(e) = me.upgrade() {
                e.flyout.hide();
                if let Some(f) = e.on_switch.borrow().as_ref() {
                    f();
                }
            }
        });
    }

    /// What the switch button opens (set once, by the shell).
    pub fn set_switch(&self, f: impl Fn() + 'static) {
        *self.on_switch.borrow_mut() = Some(Box::new(f));
    }

    fn select_tab(&self, category: &'static str) {
        self.category.set(category);
        if let Some((_, tab)) = self.tabs.iter().find(|(id, _)| *id == category) {
            tab.set_active(true);
        }
    }

    /// Ask deskd for the emoji of the current query.
    fn load(self: &Rc<Self>) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let query = query_for(&self.search.text(), self.category.get());
        let me = Rc::downgrade(self);
        let q = query.clone();
        let done = move |rows: Option<Vec<EmojiRow>>| {
            if let Some(e) = me.upgrade().filter(|e| e.generation.get() == generation) {
                e.show_rows(rows, &q);
            }
        };
        match query {
            Query::Search(text) => {
                let lang = emoji_lang(&desk_settings::i18n::ui_language());
                deskd::emoji_search(&text, lang, SEARCH_LIMIT, done);
            }
            Query::Category(RECENTS) => deskd::emoji_recents(done),
            Query::Category(id) => deskd::emoji_category(id, 0, done),
        }
    }

    fn show_rows(self: &Rc<Self>, rows: Option<Vec<EmojiRow>>, query: &Query) {
        // nothing used yet: open on the smileys, as Windows 11 does
        if rows.as_deref() == Some(&[]) && *query == Query::Category(RECENTS) && self.is_fresh() {
            self.select_tab("smileys");
            return;
        }
        self.grid.remove_all();
        let status = empty_reason(rows.as_deref(), query).map(|why| match why {
            Empty::Unavailable => {
                tr!("Emoji are not available: the desktop service is not running")
            }
            Empty::NoRecents => tr!("No recently used emoji"),
            Empty::NoMatch => tr!("No emoji found"),
        });
        self.status.set_visible(status.is_some());
        self.status.set_label(status.as_deref().unwrap_or(""));
        let rows = rows.unwrap_or_default();
        for (i, row) in rows.iter().enumerate() {
            let button = gtk::Button::builder().label(&row.glyph).build();
            button.add_css_class("emoji-cell");
            name_widget(&button, &row.name);
            let me = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(e) = me.upgrade() {
                    e.pick(i);
                }
            });
            self.grid.append(&button);
        }
        *self.rows.borrow_mut() = rows;
    }

    /// The panel was just opened (the first load after `show`).
    fn is_fresh(&self) -> bool {
        self.rows.borrow().is_empty()
    }

    /// Put emoji `index` on the clipboard and close.
    fn pick(self: &Rc<Self>, index: usize) {
        let Some(row) = self.rows.borrow().get(index).cloned() else {
            return;
        };
        let me = Rc::downgrade(self);
        deskd::emoji_pick(&row.id, move |glyph| {
            let Some(e) = me.upgrade() else { return };
            // copy while the panel still has the keyboard (Wayland wants a recent input event),
            // then close; deskd not answering still copies the plain emoji
            launch::copy_text(glyph.as_deref().unwrap_or(&row.glyph));
            e.flyout.hide();
        });
    }

    /// Open (fresh: the recents, empty search) unless it is open already.
    pub fn open(self: &Rc<Self>) {
        if !self.flyout.is_open() {
            self.prepare();
            self.flyout.show();
            self.search.grab_focus();
        }
    }

    fn prepare(self: &Rc<Self>) {
        self.rows.borrow_mut().clear();
        self.grid.remove_all();
        self.search.set_text("");
        if self.category.get() == RECENTS {
            self.load();
        } else {
            self.select_tab(RECENTS);
        }
    }

    /// Open or close (`desk-shell --toggle-emoji`).
    pub fn toggle(self: &Rc<Self>) {
        self.flyout.toggle(|| self.prepare());
        if self.flyout.is_open() {
            self.search.grab_focus();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str) -> EmojiRow {
        EmojiRow {
            id: id.into(),
            glyph: "😀".into(),
            name: "Grinning face".into(),
        }
    }

    #[test]
    fn categories_are_deskd_ids() {
        // the ids deskd's dataset has (crates/deskd/src/emoji/data.rs), recents first
        let ids: Vec<&str> = CATEGORIES.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            [
                RECENTS,
                "smileys",
                "people",
                "animals",
                "food",
                "travel",
                "activities",
                "objects",
                "symbols"
            ]
        );
        assert!(
            CATEGORIES
                .iter()
                .all(|(_, icon)| icon.ends_with("-symbolic"))
        );
    }

    #[test]
    fn the_search_text_wins_over_the_tab() {
        assert_eq!(query_for("", "people"), Query::Category("people"));
        assert_eq!(query_for("  ", RECENTS), Query::Category(RECENTS));
        assert_eq!(
            query_for(" серце ", "people"),
            Query::Search("серце".into())
        );
    }

    #[test]
    fn names_follow_the_interface_language() {
        assert_eq!(emoji_lang("uk"), "uk");
        assert_eq!(emoji_lang("uk_UA"), "uk");
        assert_eq!(emoji_lang("en_GB"), "en");
        assert_eq!(emoji_lang("de"), "");
        assert_eq!(emoji_lang(""), "");
    }

    #[test]
    fn empty_grids_say_why() {
        let cat = Query::Category("smileys");
        assert_eq!(empty_reason(None, &cat), Some(Empty::Unavailable));
        assert_eq!(
            empty_reason(Some(&[]), &Query::Category(RECENTS)),
            Some(Empty::NoRecents)
        );
        assert_eq!(empty_reason(Some(&[]), &cat), Some(Empty::NoMatch));
        assert_eq!(
            empty_reason(Some(&[]), &Query::Search("zz".into())),
            Some(Empty::NoMatch)
        );
        assert_eq!(empty_reason(Some(&[row("grinning")]), &cat), None);
    }
}
