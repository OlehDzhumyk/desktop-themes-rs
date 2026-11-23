// SPDX-License-Identifier: GPL-3.0-or-later
//! Головне вікно: сітка тем, сторінка «Змішати», меню, діалоги.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use desk_settings::{Section, tr};
use gtk::{gio, glib};
use themes_core::Ctx;
use themes_core::modules::Module;
use themes_core::state::State;
use themes_core::theme::all_themes;

use super::GuiOptions;
use super::a11y;
use super::card::{ThemeCard, ThemeInfo};
use super::search::{self, SearchHit};

/// What a window action does: the window and the action's parameter.
type ActionHandler = Box<dyn Fn(&Rc<ThemesWindow>, Option<&glib::Variant>)>;

pub struct ThemesWindow {
    ctx: Ctx,
    pub win: adw::ApplicationWindow,
    content_title: adw::WindowTitle,
    content_page: adw::NavigationPage,
    content_stack: gtk::Stack,
    split: adw::NavigationSplitView,
    nav_list: gtk::ListBox,
    search_bar: gtk::SearchBar,
    search_entry: gtk::SearchEntry,
    save_btn: gtk::Button,
    flow: gtk::FlowBox,
    main_stack: gtk::Stack,
    status: adw::StatusPage,
    toasts: adw::ToastOverlay,
    banner: adw::Banner,
    mix_group: adw::PreferencesGroup,
    mix_rows: RefCell<Vec<(Module, adw::ComboRow)>>,
    mix_guard: Cell<bool>,
    cards: RefCell<Vec<ThemeCard>>,
    themes: RefCell<Vec<ThemeInfo>>,
    themes_subtitle: RefCell<String>,
    page_titles: HashMap<String, String>,
    targets: HashMap<String, HashMap<String, gtk::Widget>>,
    nav_rows: HashMap<String, gtk::ListBoxRow>,
    busy: Cell<bool>,
    reload_id: RefCell<Option<glib::SourceId>>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

fn load_infos(ctx: &Ctx) -> Vec<ThemeInfo> {
    let st = State::load(ctx);
    all_themes(ctx)
        .into_iter()
        .map(|t| ThemeInfo {
            active_modules: Module::ALL
                .into_iter()
                .filter(|m| st.source(*m) == Some(t.id.as_str()))
                .collect(),
            modules: t.modules(),
            extensions: t.extensions().len(),
            name: t.name(),
            description: t.description(),
            preview: t.preview(),
            preview_dark: t.preview_dark(),
            builtin: t.builtin(),
            experimental: t.experimental(),
            warning: if t.experimental() {
                t.warning()
            } else {
                String::new()
            },
            id: t.id,
        })
        .collect()
}

pub fn add_app_actions(app: &adw::Application) {
    let add = |name: &str, f: Box<dyn Fn(&adw::Application)>| {
        let a = gio::SimpleAction::new(name, None);
        let app2 = app.clone();
        a.connect_activate(move |_, _| f(&app2));
        app.add_action(&a);
    };
    add(
        "extension-manager",
        Box::new(|app| spawn_flatpak(app, "com.mattjakeman.ExtensionManager", "Extension Manager")),
    );
    add(
        "refine",
        Box::new(|app| spawn_flatpak(app, "page.tesk.Refine", "Refine")),
    );
    add(
        "open-folder",
        Box::new(|app| {
            let f = gio::File::for_path(Ctx::from_env().themes);
            let launcher = gtk::FileLauncher::new(Some(&f));
            let win = app.active_window();
            launcher.launch(win.as_ref(), gio::Cancellable::NONE, |r| {
                if let Err(e) = r {
                    eprintln!("{}", tr!("Could not open the folder: {error}", error = e));
                }
            });
        }),
    );
    add(
        "about",
        Box::new(|app| {
            let d = adw::AboutDialog::builder()
                .application_name(tr!("Desktop"))
                .application_icon("preferences-desktop-appearance")
                .developer_name("OlehDzhumyk")
                .version(env!("CARGO_PKG_VERSION"))
                .comments(tr!(
                    "Switches the look of the GNOME desktop: Fedora, macOS, Windows 11 and your own themes. \
                     Every theme consists of modules (look, behaviour, sounds, apps) that can be mixed.\n\n\
                     If the shell broke after applying a theme: menu → “Reset to the standard look” \
                     or “themes reset” in a terminal; “themes doctor --fix” disables broken extensions."
                ))
                .translator_credits(tr!("translator-credits"))
                .build();
            d.present(app.active_window().as_ref());
        }),
    );
    add(
        "restart",
        Box::new(|app| {
            super::RESTART.store(true, std::sync::atomic::Ordering::SeqCst);
            app.quit();
        }),
    );
    add("quit", Box::new(|app| app.quit()));
    app.set_accels_for_action("app.quit", &["<Control>q"]);
}

fn spawn_flatpak(app: &adw::Application, id: &str, label: &str) {
    if let Err(e) =
        desk_settings::i18n::restore_child_env(&mut std::process::Command::new("flatpak"))
            .args(["run", id])
            .spawn()
        && let Some(w) = app.active_window()
        && let Ok(w) = w.downcast::<adw::ApplicationWindow>()
        && let Some(t) = find_toasts(&w)
    {
        t.add_toast(adw::Toast::new(&tr!(
            "Could not start {app}: {error}",
            app = label,
            error = e
        )));
    }
}

fn find_toasts(w: &adw::ApplicationWindow) -> Option<adw::ToastOverlay> {
    w.content()?.downcast::<adw::ToastOverlay>().ok()
}

fn translate_str(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        desk_settings::i18n::gettext(s)
    }
}

fn build_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let s1 = gio::Menu::new();
    s1.append(
        Some(&tr!("Configure extensions")),
        Some("app.extension-manager"),
    );
    s1.append(Some(&tr!("Advanced settings")), Some("app.refine"));
    s1.append(
        Some(&tr!("Open the themes folder")),
        Some("app.open-folder"),
    );
    let s2 = gio::Menu::new();
    s2.append(Some(&tr!("Check the system")), Some("win.doctor"));
    s2.append(Some(&tr!("Reset to the standard look")), Some("win.reset"));
    s2.append(Some(&tr!("Reload the list")), Some("win.reload"));
    let s3 = gio::Menu::new();
    s3.append(Some(&tr!("About Desktop")), Some("app.about"));
    menu.append_section(None, &s1);
    menu.append_section(None, &s2);
    menu.append_section(None, &s3);
    menu
}

fn build_themes_page(ctx: &Ctx) -> (gtk::Stack, gtk::FlowBox, adw::StatusPage) {
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(4)
        .column_spacing(20)
        .row_spacing(20)
        .valign(gtk::Align::Start)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    let status = adw::StatusPage::builder()
        .icon_name("preferences-desktop-appearance-symbolic")
        .title(tr!("No themes found"))
        .description(tr!(
            "Add theme folders to {path}",
            path = ctx.themes.display()
        ))
        .build();
    let clamp = adw::Clamp::builder()
        .maximum_size(1600)
        .tightening_threshold(1600)
        .child(&flow)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&clamp)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let main_stack = gtk::Stack::new();
    main_stack.add_named(&scroller, Some("grid"));
    main_stack.add_named(&status, Some("empty"));
    main_stack.set_vexpand(true);
    (main_stack, flow, status)
}

fn build_mix_page() -> (adw::PreferencesPage, adw::PreferencesGroup) {
    let mix_page = adw::PreferencesPage::builder()
        .description(tr!(
            "Take single modules from different themes: for example the macOS look with Windows 11 behaviour. A change is applied at once."
        ))
        .build();
    let mix_group = adw::PreferencesGroup::builder()
        .title(tr!("Modules"))
        .build();
    mix_page.add(&mix_group);
    let mix_tools = adw::PreferencesGroup::new();
    let reset_row = adw::ButtonRow::builder()
        .title(tr!("Reset everything to the standard look"))
        .action_name("win.reset")
        .build();
    reset_row.add_css_class("destructive-action");
    mix_tools.add(&reset_row);
    mix_page.add(&mix_tools);
    (mix_page, mix_group)
}

fn add_sidebar_section(list: &gtk::ListBox, title: &str) {
    let lbl = gtk::Label::builder().label(title).xalign(0.0).build();
    lbl.add_css_class("sidebar-section-header");
    let row = gtk::ListBoxRow::builder()
        .child(&lbl)
        .selectable(false)
        .activatable(false)
        .focusable(false)
        .build();
    list.append(&row);
}

fn add_sidebar_item(list: &gtk::ListBox, id: &str, title: &str, icon: &str) -> gtk::ListBoxRow {
    let hbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    hbox.append(&gtk::Image::from_icon_name(icon));
    let lbl = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    hbox.append(&lbl);
    let row = gtk::ListBoxRow::builder().child(&hbox).build();
    row.set_widget_name(id);
    list.append(&row);
    row
}

struct SidebarPane {
    page: adw::NavigationPage,
    nav_list: gtk::ListBox,
    search_bar: gtk::SearchBar,
    search_entry: gtk::SearchEntry,
    sidebar_stack: gtk::Stack,
    search_list: gtk::ListBox,
}

fn build_sidebar_pane() -> SidebarPane {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();

    let search_btn = gtk::ToggleButton::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text(tr!("Search settings (Ctrl+F)"))
        .build();
    a11y::label(&search_btn, &tr!("Search settings"));
    header.pack_start(&search_btn);

    let menu = build_menu();
    header.pack_end(
        &gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .primary(true)
            .tooltip_text(tr!("Main menu"))
            .build(),
    );
    toolbar.add_top_bar(&header);

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text(tr!("Search settings…"))
        .hexpand(true)
        .build();
    a11y::label(&search_entry, &tr!("Search settings"));
    let search_bar = gtk::SearchBar::builder()
        .child(&search_entry)
        .show_close_button(false)
        .build();
    search_bar.connect_entry(&search_entry);
    search_btn
        .bind_property("active", &search_bar, "search-mode-enabled")
        .bidirectional()
        .sync_create()
        .build();
    toolbar.add_top_bar(&search_bar);

    let nav_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .build();
    nav_list.add_css_class("navigation-sidebar");
    let nav_scroll = gtk::ScrolledWindow::builder()
        .child(&nav_list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let search_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    search_list.add_css_class("navigation-sidebar");
    let search_scroll = gtk::ScrolledWindow::builder()
        .child(&search_list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let search_empty = adw::StatusPage::builder()
        .icon_name("system-search-symbolic")
        .title(tr!("No results"))
        .description(tr!("Try a different search term"))
        .build();

    let sidebar_stack = gtk::Stack::new();
    sidebar_stack.add_named(&nav_scroll, Some("nav"));
    sidebar_stack.add_named(&search_scroll, Some("search"));
    sidebar_stack.add_named(&search_empty, Some("search-empty"));
    sidebar_stack.set_visible_child_name("nav");
    toolbar.set_content(Some(&sidebar_stack));

    let page = adw::NavigationPage::builder()
        .title(tr!("Desktop"))
        .child(&toolbar)
        .build();

    SidebarPane {
        page,
        nav_list,
        search_bar,
        search_entry,
        sidebar_stack,
        search_list,
    }
}

struct RegisteredPages {
    page_titles: HashMap<String, String>,
    targets: HashMap<String, HashMap<String, gtk::Widget>>,
    nav_rows: HashMap<String, gtk::ListBoxRow>,
}

fn populate_pages(
    ctx: &Ctx,
    toasts: &adw::ToastOverlay,
    nav_list: &gtk::ListBox,
    content_stack: &gtk::Stack,
    themes_widget: &gtk::Stack,
    mix_page: &adw::PreferencesPage,
) -> RegisteredPages {
    let mut page_titles = HashMap::new();
    let mut targets = HashMap::new();
    let mut nav_rows = HashMap::new();

    // Appearance section: Themes, Mix
    add_sidebar_section(nav_list, &tr!("Appearance"));
    for (id, title, icon) in [
        ("themes", tr!("Themes"), "view-grid-symbolic"),
        ("mix", tr!("Mix"), "view-dual-symbolic"),
    ] {
        let row = add_sidebar_item(nav_list, id, &title, icon);
        nav_rows.insert(id.to_string(), row);
        page_titles.insert(id.to_string(), title);
    }
    content_stack.add_named(themes_widget, Some("themes"));
    content_stack.add_named(mix_page, Some("mix"));

    // Registry sections: Features, Application (+ System), About
    for section in [Section::Features, Section::App, Section::About] {
        let pages: Vec<&'static desk_settings::Page> = desk_settings::PAGES
            .iter()
            .copied()
            .filter(|p| p.section == section)
            .collect();
        if pages.is_empty() && section != Section::App {
            continue;
        }
        add_sidebar_section(nav_list, &translate_str(section.title()));
        for p in pages {
            let title = translate_str(p.title);
            let row = add_sidebar_item(nav_list, p.id, &title, p.icon);
            nav_rows.insert(p.id.to_string(), row);
            page_titles.insert(p.id.to_string(), title);
            let built = super::features::build_page(p, toasts);
            content_stack.add_named(&built.page, Some(p.id));
            targets.insert(p.id.to_string(), built.targets);
        }
        if section == Section::App {
            let sys_title = tr!("System");
            let row = add_sidebar_item(nav_list, "system", &sys_title, "emblem-system-symbolic");
            nav_rows.insert("system".to_string(), row);
            page_titles.insert("system".to_string(), sys_title);
            let sys_page = super::system::build_page(ctx, toasts);
            content_stack.add_named(&sys_page, Some("system"));
        }
    }

    RegisteredPages {
        page_titles,
        targets,
        nav_rows,
    }
}

fn resolve_initial_page(req: Option<&str>) -> &str {
    match req {
        None | Some("themes") => "themes",
        Some("mix") => "mix",
        Some("system") => "system",
        Some("features") => super::features::first_feature_page_id().unwrap_or("themes"),
        Some(id) if desk_settings::page(id).is_some() => id,
        _ => "themes",
    }
}

impl ThemesWindow {
    pub fn build(
        app: &adw::Application,
        opts: GuiOptions,
        exit: Rc<Cell<i32>>,
    ) -> Rc<ThemesWindow> {
        let ctx = Ctx::from_env();
        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title(tr!("Desktop"))
            .default_width(opts.size.0)
            .default_height(opts.size.1)
            .build();

        let toasts = adw::ToastOverlay::new();
        let sidebar = build_sidebar_pane();

        let content_toolbar = adw::ToolbarView::new();
        let content_header = adw::HeaderBar::new();
        let content_title = adw::WindowTitle::new(&tr!("Themes"), "");
        content_header.set_title_widget(Some(&content_title));
        let save_btn = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("document-save-symbolic")
                    .label(tr!("Save as theme"))
                    .build(),
            )
            .tooltip_text(tr!("Save the current look as a theme"))
            .action_name("win.snapshot")
            .build();
        content_header.pack_end(&save_btn);
        content_toolbar.add_top_bar(&content_header);

        let banner = adw::Banner::builder()
            .title(tr!("GNOME turned extensions off after a shell crash"))
            .button_label(tr!("Turn on…"))
            .action_name("win.enable-extensions")
            .revealed(themes_core::crash::user_extensions_disabled())
            .build();
        content_toolbar.add_top_bar(&banner);

        let content_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        let (main_stack, flow, status) = build_themes_page(&ctx);
        let (mix_page, mix_group) = build_mix_page();
        let reg = populate_pages(
            &ctx,
            &toasts,
            &sidebar.nav_list,
            &content_stack,
            &main_stack,
            &mix_page,
        );
        content_toolbar.set_content(Some(&content_stack));

        let content_page = adw::NavigationPage::builder()
            .title(tr!("Themes"))
            .child(&content_toolbar)
            .build();

        let split = adw::NavigationSplitView::builder()
            .min_sidebar_width(220.0)
            .max_sidebar_width(280.0)
            .sidebar_width_fraction(0.24)
            .sidebar(&sidebar.page)
            .content(&content_page)
            .build();
        let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            680.0,
            adw::LengthUnit::Sp,
        ));
        bp.add_setter(&split, "collapsed", Some(&true.to_value()));
        win.add_breakpoint(bp);

        toasts.set_child(Some(&split));
        win.set_content(Some(&toasts));

        let this = Rc::new(ThemesWindow {
            ctx,
            win: win.clone(),
            content_title,
            content_page,
            content_stack,
            split,
            nav_list: sidebar.nav_list,
            search_bar: sidebar.search_bar,
            search_entry: sidebar.search_entry,
            save_btn,
            flow,
            main_stack,
            status,
            toasts,
            banner,
            mix_group,
            mix_rows: RefCell::new(vec![]),
            mix_guard: Cell::new(false),
            cards: RefCell::new(vec![]),
            themes: RefCell::new(vec![]),
            themes_subtitle: RefCell::new(String::new()),
            page_titles: reg.page_titles,
            targets: reg.targets,
            nav_rows: reg.nav_rows,
            busy: Cell::new(false),
            reload_id: RefCell::new(None),
            monitors: RefCell::new(vec![]),
        });
        this.connect_sidebar_and_search(sidebar.sidebar_stack, sidebar.search_list);
        this.select_page(resolve_initial_page(opts.page.as_deref()), None);
        this.connect_window_signals(app, opts, exit);
        win.present();
        this
    }

    fn connect_window_signals(
        self: &Rc<Self>,
        app: &adw::Application,
        opts: GuiOptions,
        exit: Rc<Cell<i32>>,
    ) {
        {
            let w = Rc::downgrade(self);
            self.flow.connect_child_activated(move |_, child| {
                let Some(t) = w.upgrade() else { return };
                let idx = child.index();
                let card = usize::try_from(idx)
                    .ok()
                    .and_then(|i| t.cards.borrow().get(i).cloned());
                if let Some(c) = card {
                    t.apply(c.info.id.clone(), vec![]);
                }
            });
        }
        self.add_actions(app);
        self.watch();
        {
            // window keeps a strong reference to state until closed
            let keep = RefCell::new(Some(self.clone()));
            self.win.connect_close_request(move |_| {
                keep.borrow_mut().take();
                glib::Propagation::Proceed
            });
        }
        {
            let w = Rc::downgrade(self);
            adw::StyleManager::default().connect_dark_notify(move |sm| {
                if let Some(t) = w.upgrade() {
                    let dark = sm.is_dark();
                    for c in t.cards.borrow().iter() {
                        let c = c.clone();
                        glib::spawn_future_local(async move { c.load_preview(dark).await });
                    }
                }
            });
        }
        let t2 = self.clone();
        glib::spawn_future_local(async move {
            t2.reload().await;
            if let Some(path) = opts.screenshot.clone() {
                glib::timeout_future(Duration::from_millis(900)).await;
                match t2.screenshot(&path) {
                    Ok(()) => println!("screenshot: {path}"),
                    Err(e) => {
                        eprintln!("screenshot failed: {e}");
                        exit.set(1);
                    }
                }
                if let Some(a) = t2.win.application() {
                    a.quit()
                }
            }
        });
    }

    fn select_page(&self, id: &str, target_id: Option<&str>) {
        let resolved = if self.page_titles.contains_key(id) {
            id
        } else {
            "themes"
        };
        self.content_stack.set_visible_child_name(resolved);
        let title = self.page_titles.get(resolved).map_or("", String::as_str);
        self.content_title.set_title(title);
        self.content_page.set_title(title);
        if resolved == "themes" {
            self.content_title
                .set_subtitle(&self.themes_subtitle.borrow());
        } else {
            self.content_title.set_subtitle("");
        }
        self.save_btn
            .set_visible(resolved == "themes" || resolved == "mix");
        if let Some(row) = self.nav_rows.get(resolved)
            && self.nav_list.selected_row().as_ref() != Some(row)
        {
            self.nav_list.select_row(Some(row));
        }
        self.split.set_show_content(true);
        if let Some(tid) = target_id
            && let Some(widget) = self.targets.get(resolved).and_then(|m| m.get(tid))
        {
            super::features::highlight_widget(widget);
        }
    }

    fn connect_sidebar_and_search(
        self: &Rc<Self>,
        sidebar_stack: gtk::Stack,
        search_list: gtk::ListBox,
    ) {
        {
            let w = Rc::downgrade(self);
            self.nav_list.connect_row_selected(move |_, row| {
                let Some(t) = w.upgrade() else { return };
                let Some(row) = row else { return };
                let id = row.widget_name();
                if !id.is_empty() && t.page_titles.contains_key(id.as_str()) {
                    t.select_page(&id, None);
                }
            });
        }

        let current_hits: Rc<RefCell<Vec<SearchHit>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let hits_ref = current_hits.clone();
            let sidebar_stack = sidebar_stack.clone();
            let search_list = search_list.clone();
            self.search_entry.connect_search_changed(move |entry| {
                let query = entry.text();
                if query.trim().is_empty() {
                    hits_ref.borrow_mut().clear();
                    sidebar_stack.set_visible_child_name("nav");
                    return;
                }
                while let Some(child) = search_list.first_child() {
                    search_list.remove(&child);
                }
                let hits = search::search(&query);
                if hits.is_empty() {
                    hits_ref.borrow_mut().clear();
                    sidebar_stack.set_visible_child_name("search-empty");
                    return;
                }
                for hit in &hits {
                    let vbox = gtk::Box::builder()
                        .orientation(gtk::Orientation::Vertical)
                        .spacing(2)
                        .margin_top(6)
                        .margin_bottom(6)
                        .margin_start(6)
                        .margin_end(6)
                        .build();
                    let title = gtk::Label::builder()
                        .label(&hit.title)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .build();
                    vbox.append(&title);
                    if !hit.subtitle.is_empty() {
                        let sub = gtk::Label::builder()
                            .label(&hit.subtitle)
                            .xalign(0.0)
                            .ellipsize(gtk::pango::EllipsizeMode::End)
                            .build();
                        sub.add_css_class("caption");
                        sub.add_css_class("dim-label");
                        vbox.append(&sub);
                    }
                    let row = gtk::ListBoxRow::builder().child(&vbox).build();
                    search_list.append(&row);
                }
                *hits_ref.borrow_mut() = hits;
                sidebar_stack.set_visible_child_name("search");
            });
        }
        {
            let w = Rc::downgrade(self);
            let hits_ref = current_hits.clone();
            search_list.connect_row_activated(move |_, row| {
                let Some(t) = w.upgrade() else { return };
                let Ok(idx) = usize::try_from(row.index()) else {
                    return;
                };
                let hit = hits_ref.borrow().get(idx).cloned();
                if let Some(hit) = hit {
                    t.select_page(hit.page_id, hit.target_id.as_deref());
                }
            });
        }
        {
            let w = Rc::downgrade(self);
            let hits_ref = current_hits;
            self.search_entry.connect_activate(move |_| {
                let Some(t) = w.upgrade() else { return };
                let first = hits_ref.borrow().first().cloned();
                if let Some(hit) = first {
                    t.select_page(hit.page_id, hit.target_id.as_deref());
                }
            });
        }
        {
            let entry = self.search_entry.clone();
            self.search_bar
                .connect_search_mode_enabled_notify(move |bar| {
                    if !bar.is_search_mode() {
                        entry.set_text("");
                        sidebar_stack.set_visible_child_name("nav");
                    }
                });
        }
    }

    fn add_actions(self: &Rc<Self>, app: &adw::Application) {
        let add = |name: &str, ty: Option<&glib::VariantTy>, f: ActionHandler| {
            let a = gio::SimpleAction::new(name, ty);
            let w = Rc::downgrade(self);
            a.connect_activate(move |_, p| {
                if let Some(t) = w.upgrade() {
                    f(&t, p);
                }
            });
            self.win.add_action(&a);
        };
        add("snapshot", None, Box::new(|t, _| t.ask_snapshot()));
        add(
            "reload",
            None,
            Box::new(|t, _| {
                let t = t.clone();
                glib::spawn_future_local(async move { t.reload().await });
            }),
        );
        add(
            "reset",
            None,
            Box::new(|t, _| t.apply(themes_core::BUILTIN_THEME.into(), vec![])),
        );
        add("doctor", None, Box::new(|t, _| t.run_doctor(false)));
        add(
            "enable-extensions",
            None,
            Box::new(|t, _| t.ask_enable_extensions()),
        );
        add(
            "delete-theme",
            Some(glib::VariantTy::STRING),
            Box::new(|t, p| {
                if let Some(id) = p.and_then(|v| v.str()) {
                    t.ask_delete(id.to_string());
                }
            }),
        );
        add(
            "apply-module",
            Some(glib::VariantTy::new("(ss)").unwrap()),
            Box::new(|t, p| {
                if let Some((id, m)) = p.and_then(|v| v.get::<(String, String)>())
                    && let Ok(m) = Module::parse(&m)
                {
                    t.apply(id, vec![m]);
                }
            }),
        );
        add(
            "logout",
            None,
            Box::new(|t, _| {
                if let Err(e) = desk_settings::i18n::restore_child_env(
                    &mut std::process::Command::new("gnome-session-quit"),
                )
                .arg("--logout")
                .spawn()
                {
                    t.toast(&tr!("Could not log out: {error}", error = e), 8);
                }
            }),
        );
        add(
            "search",
            None,
            Box::new(|t, _| {
                let next = !t.search_bar.is_search_mode();
                t.search_bar.set_search_mode(next);
                if next {
                    t.split.set_show_content(false);
                    t.search_entry.grab_focus();
                }
            }),
        );
        add("close", None, Box::new(|t, _| t.win.close()));
        app.set_accels_for_action("win.reload", &["F5", "<Control>r"]);
        app.set_accels_for_action("win.snapshot", &["<Control>s"]);
        app.set_accels_for_action("win.search", &["<Control>f"]);
        app.set_accels_for_action("win.close", &["<Control>w"]);
    }

    fn watch(self: &Rc<Self>) {
        for (p, only_state) in [
            (self.ctx.themes.clone(), false),
            (self.ctx.root.clone(), true),
        ] {
            let Ok(m) = gio::File::for_path(&p)
                .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            else {
                continue;
            };
            let w = Rc::downgrade(self);
            m.connect_changed(move |_, f, _, _| {
                let Some(t) = w.upgrade() else { return };
                let n = f
                    .basename()
                    .map_or_default(|b| b.to_string_lossy().into_owned());
                if only_state && n != "current" && n != "state.json" {
                    return;
                }
                if let Some(id) = t.reload_id.borrow_mut().take() {
                    id.remove();
                }
                let w2 = Rc::downgrade(&t);
                let id = glib::timeout_add_local_once(Duration::from_millis(600), move || {
                    if let Some(t) = w2.upgrade() {
                        t.reload_id.borrow_mut().take();
                        if !t.busy.get() {
                            glib::spawn_future_local(async move { t.reload().await });
                        }
                    }
                });
                *t.reload_id.borrow_mut() = Some(id);
            });
            self.monitors.borrow_mut().push(m);
        }
    }

    pub fn toast(&self, title: &str, timeout: u32) -> adw::Toast {
        let t = adw::Toast::builder()
            .title(title)
            .timeout(timeout)
            .use_markup(false)
            .build();
        self.toasts.add_toast(t.clone());
        t
    }

    pub async fn reload(self: &Rc<Self>) {
        self.banner
            .set_revealed(themes_core::crash::user_extensions_disabled());
        let infos = load_infos(&self.ctx);
        while let Some(c) = self.flow.first_child() {
            self.flow.remove(&c);
        }
        let cards: Vec<ThemeCard> = infos.iter().cloned().map(ThemeCard::new).collect();
        for c in &cards {
            self.flow.append(&c.root);
            // FlowBoxChild отримує назву картки для читачів екрана
            if let Some(child) = c.root.parent() {
                super::a11y::label(&child, &c.accessible_label());
            }
        }
        *self.cards.borrow_mut() = cards.clone();
        *self.themes.borrow_mut() = infos.clone();
        let st = State::load(&self.ctx);
        let name_of = |id: &str| {
            infos
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.name.clone())
                .unwrap_or(id.to_string())
        };
        let subtitle = match (st.uniform(), st.source(Module::Look)) {
            (Some(u), _) => tr!("Active theme: {name}", name = name_of(u)),
            (None, Some(_)) => {
                let parts: Vec<String> = Module::ALL
                    .iter()
                    .map(|m| {
                        format!(
                            "{} — {}",
                            m.label().to_lowercase(),
                            st.source(*m).map_or("—".into(), name_of)
                        )
                    })
                    .collect();
                tr!("Mixed: {parts}", parts = parts.join(", "))
            }
            _ => tr!("No theme applied yet"),
        };
        *self.themes_subtitle.borrow_mut() = subtitle.clone();
        if self.content_stack.visible_child_name().as_deref() == Some("themes") {
            self.content_title.set_subtitle(&subtitle);
        }
        self.main_stack
            .set_visible_child_name(if infos.is_empty() { "empty" } else { "grid" });
        self.rebuild_mix(&infos, &st);
        let dark = adw::StyleManager::default().is_dark();
        for c in cards {
            c.load_preview(dark).await;
        }
    }

    fn rebuild_mix(self: &Rc<Self>, infos: &[ThemeInfo], st: &State) {
        self.mix_guard.set(true);
        for (_, r) in self.mix_rows.borrow_mut().drain(..) {
            self.mix_group.remove(&r);
        }
        for m in Module::ALL {
            let names: Vec<String> = infos
                .iter()
                .map(|t| {
                    if t.modules.contains(&m) {
                        t.name.clone()
                    } else {
                        tr!("{name} (standard)", name = t.name)
                    }
                })
                .collect();
            let model = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
            let row = adw::ComboRow::builder()
                .title(m.label())
                .subtitle(m.description())
                .model(&model)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(m.icon()));
            let cur = st
                .source(m)
                .and_then(|s| infos.iter().position(|t| t.id == s));
            row.set_selected(
                cur.and_then(|i| u32::try_from(i).ok())
                    .unwrap_or(gtk::INVALID_LIST_POSITION),
            );
            let w = Rc::downgrade(self);
            row.connect_selected_notify(move |r| {
                let Some(t) = w.upgrade() else { return };
                if t.mix_guard.get() {
                    return;
                }
                let id = t
                    .themes
                    .borrow()
                    .get(r.selected() as usize)
                    .map(|x| x.id.clone());
                if let Some(id) = id {
                    t.apply(id, vec![m]);
                }
            });
            self.mix_group.add(&row);
            self.mix_rows.borrow_mut().push((m, row));
        }
        self.mix_guard.set(false);
    }

    fn set_busy(&self, id: Option<&str>) {
        self.busy.set(id.is_some());
        for c in self.cards.borrow().iter() {
            c.set_busy(id == Some(c.info.id.as_str()));
        }
        self.flow.set_sensitive(id.is_none());
        self.mix_group.set_sensitive(id.is_none());
    }

    /// Застосувати; експериментальна тема — лише після підтвердження в діалозі з її попередженням.
    pub fn apply(self: &Rc<Self>, id: String, modules: Vec<Module>) {
        let info = self.themes.borrow().iter().find(|t| t.id == id).cloned();
        let Some(info) = info.filter(|i| i.experimental) else {
            self.confirm_commands(id, modules, false);
            return;
        };
        let dlg = adw::AlertDialog::builder()
            .heading(tr!(
                "Apply the experimental theme “{name}”?",
                name = info.name
            ))
            .body(&info.warning)
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dlg.add_response("cancel", &tr!("Cancel"));
        dlg.add_response("apply", &tr!("Apply anyway"));
        dlg.set_response_appearance("apply", adw::ResponseAppearance::Destructive);
        let t = self.clone();
        dlg.connect_response(None, move |_, r| {
            if r == "apply" {
                t.confirm_commands(id.clone(), modules.clone(), true);
            } else {
                // повернути вибір у «Змішати» до поточного стану
                let t = t.clone();
                glib::spawn_future_local(async move { t.reload().await });
            }
        });
        dlg.present(Some(&self.win));
    }

    /// Ключі теми, що запускають команди (скорочення, термінал, автозапуск): перший раз показати
    /// їх у діалозі й застосувати лише після згоди (S-5). Згода запам'ятовується для ключа й команди.
    /// `confirmed`: the user already said yes to the experimental-theme dialog.
    fn confirm_commands(self: &Rc<Self>, id: String, modules: Vec<Module>, confirmed: bool) {
        if self.busy.get() {
            return;
        }
        let t = self.clone();
        glib::spawn_future_local(async move {
            let ctx = t.ctx.clone();
            let (id2, mods) = (id.clone(), modules.clone());
            let plan = gio::spawn_blocking(move || {
                let dry = themes_core::apply::Options {
                    dry_run: true,
                    ..Default::default()
                };
                themes_core::apply::apply(&ctx, &id2, &mods, &dry)
                    .map(|r| (r.name, themes_core::commands::pending(&ctx, &r.writes)))
            })
            .await;
            // a failed dry run: apply_now meets the same error and reports it
            let Ok(Ok((name, pending))) = plan else {
                t.apply_now(id, modules, confirmed);
                return;
            };
            if pending.is_empty() {
                t.apply_now(id, modules, confirmed);
                return;
            }
            let list: Vec<String> = pending
                .iter()
                .map(|c| format!("{} = {}", c.key, c.value))
                .collect();
            let dlg = adw::AlertDialog::builder()
                .heading(tr!("Allow “{name}” to run commands?", name = name))
                .body(tr!(
                    "This theme sets keys that make the desktop run programs:\n\n{list}",
                    list = list.join("\n")
                ))
                .default_response("cancel")
                .close_response("cancel")
                .build();
            dlg.add_response("cancel", &tr!("Cancel"));
            dlg.add_response("apply", &tr!("Allow and apply"));
            dlg.set_response_appearance("apply", adw::ResponseAppearance::Destructive);
            let t2 = t.clone();
            dlg.connect_response(None, move |_, r| {
                if r == "apply" {
                    if let Err(e) = themes_core::commands::approve(&t2.ctx, &pending) {
                        t2.toast(&tr!("Error: {error}", error = e), 10);
                    }
                    t2.apply_now(id.clone(), modules.clone(), confirmed);
                } else {
                    let t = t2.clone();
                    glib::spawn_future_local(async move { t.reload().await });
                }
            });
            dlg.present(Some(&t.win));
        });
    }

    /// `confirmed`: the user said yes in the experimental-theme dialog (the library refuses an
    /// experimental theme without it).
    fn apply_now(self: &Rc<Self>, id: String, modules: Vec<Module>, confirmed: bool) {
        if self.busy.get() {
            return;
        }
        self.set_busy(Some(&id));
        let t = self.clone();
        glib::spawn_future_local(async move {
            let ctx = t.ctx.clone();
            let id2 = id.clone();
            let mods = modules.clone();
            let opts = themes_core::apply::Options {
                confirmed,
                ..Default::default()
            };
            let res =
                gio::spawn_blocking(move || themes_core::apply::apply(&ctx, &id2, &mods, &opts))
                    .await;
            t.set_busy(None);
            let res = match res {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    t.toast(&tr!("Error: {error}", error = e), 10);
                    t.reload().await;
                    return;
                }
                Err(_) => {
                    t.toast(&tr!("Error: the apply thread crashed"), 10);
                    return;
                }
            };
            let what = if modules.is_empty() {
                tr!("Theme “{name}” applied", name = res.name)
            } else {
                let m: Vec<String> = modules.iter().map(|m| m.label()).collect();
                tr!(
                    "“{modules}” from the theme “{name}” applied",
                    modules = m.join("”, “"),
                    name = res.name
                )
            };
            let mut title = what;
            if !res.warnings.is_empty() {
                title.push_str(&tr!(" (warnings: {count})", count = res.warnings.len()));
            }
            if res.relogin {
                let toast = t.toast(
                    &tr!(
                        "{what}. Some shell changes start working after you log in again",
                        what = title
                    ),
                    0,
                );
                toast.set_button_label(Some(&tr!("Log out")));
                toast.set_action_name(Some("win.logout"));
            } else if res.shell_running {
                t.toast(&tr!("{what} live", what = title), 5);
            } else {
                t.toast(&title, 5);
            }
            t.reload().await;
            // автовідкат: перевірити розширення через ~5 с
            if res.shell_running && res.extensions_changed {
                let ctx = t.ctx.clone();
                let bad = gio::spawn_blocking(move || {
                    themes_core::apply::verify_live(&ctx, Duration::from_secs(5))
                })
                .await;
                if let Ok(Ok(bad)) = bad
                    && !bad.is_empty()
                {
                    let list: Vec<&str> = bad.iter().map(|b| b.0.as_str()).collect();
                    t.toast(
                        &tr!(
                            "Extensions with errors turned off: {list}",
                            list = list.join(", ")
                        ),
                        0,
                    );
                    t.reload().await;
                }
            }
        });
    }

    fn ask_snapshot(self: &Rc<Self>) {
        let entry = gtk::Entry::builder()
            .placeholder_text(tr!("Theme name"))
            .activates_default(true)
            .build();
        let dlg = adw::AlertDialog::builder()
            .heading(tr!("Save the current look as a theme"))
            .body(tr!(
                "The current settings (look, behaviour, sounds, apps), the enabled extensions and the GTK files will be saved as a new theme."
            ))
            .extra_child(&entry)
            .default_response("save")
            .close_response("cancel")
            .build();
        dlg.add_response("cancel", &tr!("Cancel"));
        dlg.add_response("save", &tr!("Save"));
        super::a11y::label(&entry, &tr!("Theme name"));
        dlg.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dlg.set_response_enabled("save", false);
        {
            let dlg = dlg.clone();
            entry.connect_changed(move |e| {
                dlg.set_response_enabled("save", !e.text().trim().is_empty())
            });
        }
        let t = self.clone();
        let e2 = entry.clone();
        dlg.connect_response(None, move |_, r| {
            if r != "save" {
                return;
            }
            let name = e2.text().trim().to_string();
            let ids: Vec<String> = t.themes.borrow().iter().map(|x| x.id.clone()).collect();
            let base = themes_core::util::slugify(&name);
            let mut id = base.clone();
            let mut n = 2;
            while ids.contains(&id) || id == themes_core::BUILTIN_THEME {
                id = format!("{base}-{n}");
                n += 1;
            }
            let t = t.clone();
            glib::spawn_future_local(async move {
                let ctx = t.ctx.clone();
                let r = gio::spawn_blocking(move || {
                    themes_core::snapshot::snapshot(&ctx, &id, Some(&name), false)
                })
                .await;
                match r {
                    Ok(Ok(r)) => t.toast(&tr!("Theme “{name}” saved", name = r.name), 5),
                    Ok(Err(e)) => t.toast(&tr!("Error: {error}", error = e), 10),
                    Err(_) => t.toast(&tr!("Saving failed"), 10),
                };
                t.reload().await;
            });
        });
        dlg.present(Some(&self.win));
        entry.grab_focus();
    }

    fn ask_delete(self: &Rc<Self>, id: String) {
        let Some(theme) = self.themes.borrow().iter().find(|t| t.id == id).cloned() else {
            return;
        };
        if theme.builtin {
            return;
        }
        let dlg = adw::AlertDialog::builder()
            .heading(tr!("Delete the theme “{name}”?", name = theme.name))
            .body(tr!(
                "The theme folder will be moved to “trash” in the themes root; you can restore it by hand."
            ))
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dlg.add_response("cancel", &tr!("Cancel"));
        dlg.add_response("delete", &tr!("Delete"));
        dlg.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        let t = self.clone();
        dlg.connect_response(None, move |_, r| {
            if r != "delete" {
                return;
            }
            match themes_core::apply::delete(&t.ctx, &id) {
                Ok((name, _)) => t.toast(&tr!("Theme “{name}” deleted", name = name), 5),
                Err(e) => t.toast(&tr!("Error: {error}", error = e), 10),
            };
            let t = t.clone();
            glib::spawn_future_local(async move { t.reload().await });
        });
        dlg.present(Some(&self.win));
    }

    fn run_doctor(self: &Rc<Self>, fix: bool) {
        let t = self.clone();
        let wait = self.toast(
            &if fix {
                tr!("Fixing…")
            } else {
                tr!("Checking the system…")
            },
            2,
        );
        glib::spawn_future_local(async move {
            let ctx = t.ctx.clone();
            let r = gio::spawn_blocking(move || {
                themes_core::doctor::doctor(
                    &ctx,
                    &themes_core::doctor::DoctorOptions {
                        fix,
                        wait_shell: Duration::ZERO,
                        notify: false,
                    },
                )
            })
            .await;
            wait.dismiss();
            let rep = match r {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    t.toast(&tr!("Check failed: {error}", error = e), 10);
                    return;
                }
                Err(_) => return,
            };
            let mut body = tr!(
                "GNOME Shell {version} ({state}), extensions checked: {count}.",
                version = rep.gnome_version.clone().unwrap_or("?".into()),
                state = if rep.shell_running {
                    tr!("running")
                } else {
                    tr!("not running")
                },
                count = rep.extensions_checked
            );
            body.push('\n');
            if rep.issues.is_empty() {
                body.push('\n');
                body.push_str(&tr!("No problems found."));
            }
            for i in &rep.issues {
                let mark = match i.severity {
                    "error" => "✗",
                    "warning" => "⚠",
                    _ => "ℹ",
                };
                body.push_str(&format!("\n{mark} {}", i.message));
            }
            for f in &rep.fixed {
                body.push_str(&format!("\n✓ {f}"));
            }
            for u in &rep.restored {
                body.push_str(&format!("\n✓ {}", tr!("turned on again: {uuid}", uuid = u)));
            }
            let fixable = !fix && rep.issues.iter().any(|i| i.fixable);
            let dlg = adw::AlertDialog::builder()
                .heading(if rep.problems() == 0 {
                    tr!("The system is fine")
                } else {
                    tr!("Check results")
                })
                .body(body)
                .close_response("close")
                .build();
            dlg.add_response("close", &tr!("Close"));
            if fixable {
                dlg.add_response("fix", &tr!("Fix"));
                dlg.set_response_appearance("fix", adw::ResponseAppearance::Suggested);
                dlg.set_default_response(Some("fix"));
            }
            let t2 = t.clone();
            dlg.connect_response(None, move |_, r| {
                if r == "fix" {
                    t2.run_doctor(true);
                }
            });
            dlg.present(Some(&t.win));
            if fix {
                t.reload().await;
            }
        });
    }

    /// GNOME вимкнув розширення після збою: показати ймовірну причину й увімкнути лише за згодою,
    /// за бажання спершу перейшовши на безпечну тему.
    fn ask_enable_extensions(self: &Rc<Self>) {
        let t = self.clone();
        glib::spawn_future_local(async move {
            let ctx = t.ctx.clone();
            let crashes =
                gio::spawn_blocking(move || themes_core::crash::recent_shell_crashes(&ctx, 48))
                    .await
                    .unwrap_or_default();
            let mut body = tr!(
                "After a GNOME Shell crash the system turned all extensions off \
                 (disable-user-extensions), so the themes work without them now."
            );
            let culprit = crashes.first().and_then(|c| c.theme.clone());
            if let Some(c) = crashes.first() {
                body.push_str(&format!("\n\n{}.", themes_core::crash::describe(c)));
            }
            body.push_str("\n\n");
            body.push_str(&tr!(
                "If the crash happens again, GNOME turns the extensions off again. It is safer to \
                 switch to the standard theme first."
            ));
            let dlg = adw::AlertDialog::new(Some(&tr!("Turn extensions on?")), Some(&body));
            dlg.add_response("cancel", &tr!("Cancel"));
            if culprit
                .as_deref()
                .is_some_and(|c| c != themes_core::BUILTIN_THEME)
            {
                dlg.add_response("safe", &tr!("Switch to “Fedora” and turn on"));
                dlg.set_response_appearance("safe", adw::ResponseAppearance::Suggested);
            }
            dlg.add_response("enable", &tr!("Turn on"));
            dlg.set_default_response(Some("cancel"));
            let t2 = t.clone();
            dlg.connect_response(None, move |_, r| {
                let enable =
                    |t: &Rc<ThemesWindow>| match themes_core::crash::enable_user_extensions() {
                        Ok(()) => {
                            t.toast(&tr!("Extensions turned on again"), 5);
                            t.banner.set_revealed(false);
                        }
                        Err(e) => {
                            t.toast(&tr!("Failed: {error}", error = e), 10);
                        }
                    };
                match r {
                    "safe" => {
                        t2.apply(themes_core::BUILTIN_THEME.into(), vec![]);
                        enable(&t2);
                    }
                    "enable" => enable(&t2),
                    _ => {}
                }
            });
            dlg.present(Some(&t.win));
        });
    }

    fn screenshot(&self, path: &str) -> Result<(), String> {
        let w = self.win.width();
        let h = self.win.height();
        let paintable = gtk::WidgetPaintable::new(Some(&self.win));
        let snap = gtk::Snapshot::new();
        paintable.snapshot(&snap, w as f64, h as f64);
        let node = snap.to_node().ok_or("empty render node")?;
        let renderer = self.win.renderer().ok_or("no renderer")?;
        let tex = renderer.render_texture(
            &node,
            Some(&gtk::graphene::Rect::new(0.0, 0.0, w as f32, h as f32)),
        );
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        tex.save_to_png(path).map_err(|e| e.to_string())?;
        let _ = &self.status;
        Ok(())
    }
}
