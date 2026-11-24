// SPDX-License-Identifier: GPL-3.0-or-later
//! The Windows 11 Start and Search flyout: a layer surface centred above the taskbar
//! (`Layer::Overlay`, keyboard on demand, 12 px above the bar).
//!
//! An empty query shows the pinned grid (4 × 3) and the footer (user, Settings, Lock, Power); a
//! query shows the ranked results of deskd's `Search.Query` (calculator, layout-tolerant
//! applications, system actions, files, clipboard history, emoji, usage ranking). Without deskd
//! the in-process `desk-search` catalog answers (no files, clipboard or emoji). Keys: `Up`/`Down` move, `Return` activates, `Esc` clears the query, then
//! closes. Losing the focus closes the flyout too.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use desk_search::{Catalog, DesktopEntry, ResultKind, SearchOptions, SearchResult, UsageStore, n_};
use desk_settings::i18n::gettext;
use desk_settings::tr;
use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::launch::{self, Activation};
use crate::layer_shell::{self, Edge, KeyboardMode, Layer, Placement};
use crate::pinned::GRID_COLUMNS;
use crate::taskbar::{icon_of, name_widget};

const PLACEMENT: Placement = Placement {
    namespace: "desk-shell-start",
    layer: Layer::Overlay,
    anchors: &[Edge::Bottom],
    margins: &[(Edge::Bottom, 12)],
    exclusive_zone: false,
    keyboard: KeyboardMode::OnDemand,
};

const WIDTH: i32 = 640;
const HEIGHT: i32 = 720;
/// A click on the Start button right after a focus loss closed the flyout must not reopen it.
const REOPEN_GUARD_USEC: i64 = 300_000;
/// The system actions in the Power menu.
const POWER_ACTIONS: [&str; 4] = ["suspend", "restart", "power-off", "logout"];

/// Which page the flyout shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Pinned,
    Results,
}

/// The page for a query: blank text shows the pinned grid.
pub fn page_for(query: &str) -> Page {
    if query.trim().is_empty() {
        Page::Pinned
    } else {
        Page::Results
    }
}

/// What `Esc` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape {
    ClearQuery,
    Close,
}

pub fn escape_for(query: &str) -> Escape {
    if query.is_empty() {
        Escape::Close
    } else {
        Escape::ClearQuery
    }
}

/// The row selected after `Up` (`down == false`) or `Down`; stops at both ends.
pub fn moved(current: Option<usize>, len: usize, down: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (current, down) {
        (None, _) => 0,
        (Some(i), true) => (i + 1).min(len - 1),
        (Some(i), false) => i.saturating_sub(1),
    })
}

/// The second line of a result: its subtitle, else what it is (English gettext id).
pub fn subtitle_id(r: &SearchResult) -> Result<&str, &'static str> {
    if !r.subtitle.is_empty() && r.kind != ResultKind::Calculator {
        return Ok(&r.subtitle);
    }
    Err(match r.kind {
        ResultKind::Calculator => n_("Calculator: press Enter to copy the result"),
        ResultKind::App => n_("Application"),
        ResultKind::Action => n_("System action"),
        ResultKind::Web => n_("Search the web"),
        ResultKind::File => n_("File"),
        ResultKind::Clipboard => n_("Clipboard"),
        ResultKind::Emoji => n_("Emoji"),
    })
}

/// The flyout.
pub struct StartMenu {
    window: gtk::Window,
    entry: gtk::SearchEntry,
    stack: gtk::Stack,
    list: gtk::ListBox,
    results: RefCell<Vec<SearchResult>>,
    catalog: Catalog,
    usage: UsageStore,
    opts: SearchOptions,
    start_button: RefCell<Option<glib::WeakRef<gtk::ToggleButton>>>,
    /// When a focus loss last closed the flyout (monotonic µs).
    closed_at: Cell<i64>,
    /// Bumped by every query: answers of an older query are dropped.
    query_gen: Cell<u64>,
}

impl StartMenu {
    /// Build the (hidden) flyout. `grid` is the pinned grid.
    pub fn new(
        app: &gtk::Application,
        catalog: Catalog,
        usage: UsageStore,
        opts: SearchOptions,
        grid: &[&DesktopEntry],
    ) -> Rc<StartMenu> {
        let window = gtk::Window::builder()
            .application(app)
            .title(tr!("Start menu"))
            .resizable(false)
            .default_width(WIDTH)
            .default_height(HEIGHT)
            .hide_on_close(true)
            .build();
        let entry = gtk::SearchEntry::builder()
            .placeholder_text(tr!("Type here to search"))
            .hexpand(true)
            .build();
        entry.update_property(&[gtk::accessible::Property::Label(&tr!("Search"))]);
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        let stack = gtk::Stack::new();
        let menu = Rc::new(StartMenu {
            window,
            entry,
            stack,
            list,
            results: RefCell::default(),
            catalog,
            usage,
            opts,
            start_button: RefCell::default(),
            closed_at: Cell::new(0),
            query_gen: Cell::new(0),
        });
        menu.layout(grid);
        menu.connect();
        layer_shell::place(&menu.window, &PLACEMENT);
        menu
    }

    fn layout(self: &Rc<Self>, grid: &[&DesktopEntry]) {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("start-body");
        body.set_vexpand(true);
        body.append(&self.entry);
        self.stack
            .add_named(&self.pinned_page(grid), Some("pinned"));
        let scroller = gtk::ScrolledWindow::builder()
            .child(&self.list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        self.stack.add_named(&scroller, Some("results"));
        self.stack.set_vexpand(true);
        body.append(&self.stack);
        let surface = gtk::Box::new(gtk::Orientation::Vertical, 0);
        surface.add_css_class("desk-shell-start-menu");
        surface.append(&body);
        surface.append(&self.footer());
        self.window.set_child(Some(&surface));
    }

    fn pinned_page(self: &Rc<Self>, grid: &[&DesktopEntry]) -> gtk::Box {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let title = gtk::Label::builder()
            .label(tr!("Pinned"))
            .xalign(0.0)
            .build();
        title.add_css_class("section-title");
        page.append(&title);
        let tiles = gtk::Grid::builder()
            .column_homogeneous(true)
            .row_spacing(4)
            .column_spacing(4)
            .build();
        for (i, entry) in grid.iter().enumerate() {
            let column = i32::try_from(i % GRID_COLUMNS).unwrap_or(0);
            let row = i32::try_from(i / GRID_COLUMNS).unwrap_or(0);
            tiles.attach(&self.tile(entry), column, row, 1, 1);
        }
        if grid.is_empty() {
            let empty = gtk::Label::new(Some(&tr!("No applications found")));
            empty.add_css_class("empty");
            page.append(&empty);
        }
        page.append(&tiles);
        page
    }

    fn tile(self: &Rc<Self>, entry: &DesktopEntry) -> gtk::Button {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&gtk::Image::from_gicon(&icon_of(&entry.icon)));
        let label = gtk::Label::builder()
            .label(entry.display_name())
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(12)
            .build();
        content.append(&label);
        let button = gtk::Button::builder().child(&content).build();
        button.add_css_class("tile");
        let (menu, id) = (Rc::downgrade(self), entry.id.clone());
        button.connect_clicked(move |_| {
            let result_id = format!("app:{id}");
            launch::activate(&Activation::Launch(id.clone()), &result_id);
            if let Some(m) = menu.upgrade() {
                m.hide();
            }
        });
        button
    }

    fn footer_button(
        self: &Rc<Self>,
        icon: &str,
        name: &str,
        run: impl Fn() + 'static,
    ) -> gtk::Button {
        let button = gtk::Button::from_icon_name(icon);
        name_widget(&button, name);
        let menu = Rc::downgrade(self);
        button.connect_clicked(move |_| {
            if let Some(m) = menu.upgrade() {
                m.hide();
            }
            run();
        });
        button
    }

    fn power_button(self: &Rc<Self>) -> gtk::MenuButton {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let popover = gtk::Popover::builder().child(&column).build();
        for action in desk_search::SYSTEM_ACTIONS
            .iter()
            .filter(|a| POWER_ACTIONS.contains(&a.id))
        {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&gtk::Image::from_icon_name(action.icon));
            row.append(&gtk::Label::new(Some(&gettext(action.title))));
            let button = gtk::Button::builder().child(&row).build();
            button.add_css_class("flat");
            let (menu, id, p) = (Rc::downgrade(self), action.id, popover.clone());
            button.connect_clicked(move |_| {
                p.popdown();
                if let Some(m) = menu.upgrade() {
                    m.hide();
                }
                launch::run_action(id);
            });
            column.append(&button);
        }
        let button = gtk::MenuButton::builder()
            .icon_name("system-shutdown-symbolic")
            .popover(&popover)
            .direction(gtk::ArrowType::Up)
            .build();
        name_widget(&button, &tr!("Power"));
        button
    }

    fn footer(self: &Rc<Self>) -> gtk::Box {
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.add_css_class("start-footer");
        let user = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        user.append(&gtk::Image::from_icon_name("avatar-default-symbolic"));
        let real = glib::real_name().to_string_lossy().into_owned();
        let name = if real.is_empty() || real == "Unknown" {
            glib::user_name().to_string_lossy().into_owned()
        } else {
            real
        };
        user.append(&gtk::Label::new(Some(&name)));
        user.set_hexpand(true);
        footer.append(&user);
        footer.append(&self.footer_button(
            "emblem-system-symbolic",
            &tr!("Settings"),
            launch::open_settings_hub,
        ));
        footer.append(&self.footer_button(
            "system-lock-screen-symbolic",
            &tr!("Lock the screen"),
            || launch::run_action("lock"),
        ));
        footer.append(&self.power_button());
        footer
    }

    fn connect(self: &Rc<Self>) {
        let menu = Rc::downgrade(self);
        self.entry.connect_search_changed(move |_| {
            if let Some(m) = menu.upgrade() {
                m.refresh();
            }
        });
        let menu = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let (Some(m), Ok(i)) = (menu.upgrade(), usize::try_from(row.index())) {
                m.activate(i);
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let menu = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| match menu.upgrade() {
            Some(m) if m.on_key(key) => glib::Propagation::Stop,
            _ => glib::Propagation::Proceed,
        });
        self.window.add_controller(keys);
        let menu = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |w| {
            if let Some(m) = menu.upgrade()
                && !w.is_active()
                && w.is_visible()
            {
                m.closed_at.set(glib::monotonic_time());
                m.hide();
            }
        });
        let menu = Rc::downgrade(self);
        self.window.connect_hide(move |_| {
            if let Some(m) = menu.upgrade() {
                m.set_button_state(false);
            }
        });
    }

    /// Handle a key; `true` when it was used.
    fn on_key(&self, key: gdk::Key) -> bool {
        let results_page = page_for(&self.entry.text()) == Page::Results;
        match key {
            gdk::Key::Escape => {
                match escape_for(&self.entry.text()) {
                    Escape::ClearQuery => self.entry.set_text(""),
                    Escape::Close => self.hide(),
                }
                true
            }
            gdk::Key::Down | gdk::Key::Up if results_page => {
                self.select(moved(
                    self.selected(),
                    self.results.borrow().len(),
                    key == gdk::Key::Down,
                ));
                true
            }
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter if results_page => {
                self.activate(self.selected().unwrap_or(0));
                true
            }
            _ => false,
        }
    }

    fn selected(&self) -> Option<usize> {
        self.list
            .selected_row()
            .and_then(|r| usize::try_from(r.index()).ok())
    }

    fn select(&self, index: Option<usize>) {
        let row = index
            .and_then(|i| i32::try_from(i).ok())
            .and_then(|i| self.list.row_at_index(i));
        self.list.select_row(row.as_ref());
        if let Some(row) = row {
            // keep the selected row in view; the focus stays in the search field
            if let Some(adj) = self
                .list
                .parent()
                .and_then(|p| p.downcast::<gtk::Viewport>().ok())
                .and_then(|v| v.vadjustment())
                && let Some(bounds) = row.compute_bounds(&self.list)
            {
                let (top, bottom) = (
                    f64::from(bounds.y()),
                    f64::from(bounds.y() + bounds.height()),
                );
                if top < adj.value() {
                    adj.set_value(top);
                } else if bottom > adj.value() + adj.page_size() {
                    adj.set_value(bottom - adj.page_size());
                }
            }
        }
    }

    fn refresh(self: &Rc<Self>) {
        let generation = self.query_gen.get().wrapping_add(1);
        self.query_gen.set(generation);
        let text = self.entry.text().to_string();
        if page_for(&text) == Page::Pinned {
            self.stack.set_visible_child_name("pinned");
            self.results.borrow_mut().clear();
            return;
        }
        let (menu, query) = (Rc::downgrade(self), text.clone());
        crate::deskd::search(&query, move |remote| {
            let Some(m) = menu.upgrade() else { return };
            if m.query_gen.get() != generation {
                return;
            }
            let results = remote.unwrap_or_else(|| m.local_search(&text));
            m.show_results(results);
        });
    }

    /// The in-process catalog (deskd is not running or refused).
    fn local_search(&self, text: &str) -> Vec<SearchResult> {
        let now = glib::real_time().max(0).unsigned_abs() / 1_000_000;
        self.catalog.search(text, &self.usage, &self.opts, now)
    }

    fn show_results(&self, results: Vec<SearchResult>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        if results.is_empty() {
            let empty = gtk::Label::new(Some(&tr!("No results")));
            empty.add_css_class("empty");
            let row = gtk::ListBoxRow::builder()
                .child(&empty)
                .activatable(false)
                .selectable(false)
                .build();
            self.list.append(&row);
        }
        for r in &results {
            self.list.append(&result_row(r));
        }
        *self.results.borrow_mut() = results;
        self.stack.set_visible_child_name("results");
        self.select((!self.results.borrow().is_empty()).then_some(0));
    }

    fn activate(&self, index: usize) {
        let Some(r) = self.results.borrow().get(index).cloned() else {
            return;
        };
        self.hide();
        launch::activate(&Activation::of(&r), &r.id);
    }

    fn set_button_state(&self, active: bool) {
        if let Some(b) = self
            .start_button
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
        {
            b.set_active(active);
        }
    }

    /// The taskbar's Start button mirrors whether the flyout is open.
    pub fn set_start_button(&self, button: &gtk::ToggleButton) {
        *self.start_button.borrow_mut() = Some(button.downgrade());
    }

    pub fn is_open(&self) -> bool {
        self.window.is_visible()
    }

    pub fn show(&self) {
        self.window.present();
        self.entry.grab_focus();
        self.set_button_state(true);
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
        self.entry.set_text("");
        self.set_button_state(false);
    }

    /// Open or close (the Start button, `desk-shell --toggle-start`).
    pub fn toggle(&self) {
        if self.is_open() {
            self.hide();
        } else if glib::monotonic_time() - self.closed_at.get() < REOPEN_GUARD_USEC {
            // this click is what took the focus away: it closed the flyout already
            self.set_button_state(false);
        } else {
            self.show();
        }
    }
}

fn result_row(r: &SearchResult) -> gtk::ListBoxRow {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = gtk::Image::from_gicon(&icon_of(&r.icon));
    icon.set_pixel_size(24);
    line.append(&icon);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let title = gtk::Label::builder()
        .label(&r.title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let sub = match subtitle_id(r) {
        Ok(s) => s.to_owned(),
        Err(id) => gettext(id),
    };
    let subtitle = gtk::Label::builder()
        .label(&sub)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    subtitle.add_css_class("result-subtitle");
    text.append(&title);
    text.append(&subtitle);
    line.append(&text);
    let row = gtk::ListBoxRow::builder().child(&line).build();
    row.update_property(&[gtk::accessible::Property::Label(&format!(
        "{}, {sub}",
        r.title
    ))]);
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(kind: ResultKind, subtitle: &str) -> SearchResult {
        SearchResult {
            id: "x".into(),
            kind,
            title: "t".into(),
            subtitle: subtitle.into(),
            icon: String::new(),
            activate: String::new(),
            score: 0,
        }
    }

    #[test]
    fn blank_queries_show_the_pinned_grid() {
        assert_eq!(page_for(""), Page::Pinned);
        assert_eq!(page_for("  "), Page::Pinned);
        assert_eq!(page_for("= 1+1"), Page::Results);
    }

    #[test]
    fn escape_clears_then_closes() {
        assert_eq!(escape_for("text"), Escape::ClearQuery);
        // a query of spaces is still text to clear first
        assert_eq!(escape_for(" "), Escape::ClearQuery);
        assert_eq!(escape_for(""), Escape::Close);
    }

    #[test]
    fn selection_moves_and_stops_at_the_ends() {
        assert_eq!(moved(None, 0, true), None);
        assert_eq!(moved(None, 3, true), Some(0));
        assert_eq!(moved(None, 3, false), Some(0));
        assert_eq!(moved(Some(0), 3, true), Some(1));
        assert_eq!(moved(Some(2), 3, true), Some(2));
        assert_eq!(moved(Some(0), 3, false), Some(0));
        assert_eq!(moved(Some(2), 3, false), Some(1));
        // a stale index after a shorter result list is pulled back in range
        assert_eq!(moved(Some(9), 3, true), Some(2));
    }

    #[test]
    fn subtitles_fall_back_to_the_kind() {
        assert_eq!(
            subtitle_id(&result(ResultKind::App, "Web browser")),
            Ok("Web browser")
        );
        assert_eq!(
            subtitle_id(&result(ResultKind::App, "")),
            Err("Application")
        );
        assert_eq!(
            subtitle_id(&result(ResultKind::Action, "")),
            Err("System action")
        );
        // the calculator always explains Enter, not its "= expr" echo
        assert_eq!(
            subtitle_id(&result(ResultKind::Calculator, "= 2+2")),
            Err("Calculator: press Enter to copy the result")
        );
    }

    #[test]
    fn the_flyout_finds_calculations_and_actions_through_desk_search() {
        let catalog = Catalog::new(Vec::new(), str::to_owned);
        let usage = UsageStore::disabled(std::env::temp_dir().join("desk-shell-none.json"));
        let r = catalog.search("= 12*(3+4)", &usage, &SearchOptions::default(), 0);
        assert_eq!(Activation::of(&r[0]), Activation::Copy("84".into()));
        let r = catalog.search("> lock", &usage, &SearchOptions::default(), 0);
        assert_eq!(Activation::of(&r[0]), Activation::Action("lock".into()));
        assert!(
            POWER_ACTIONS
                .iter()
                .all(|id| desk_search::actions::action(id).is_some())
        );
    }
}
