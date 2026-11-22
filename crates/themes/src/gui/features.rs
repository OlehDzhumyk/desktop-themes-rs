// SPDX-License-Identifier: GPL-3.0-or-later
//! Сторінка «Функції», згенерована з реєстру налаштувань (`desk_settings::PAGES`): список сторінок
//! за розділами (функції, програма, про проєкт) і самі сторінки, рядки яких вибираються за типом
//! ключа (перемикач, лічильник, список вибору, поле, скорочення). «Подяки» беруться з CREDITS.md.
//!
//! Налаштування належать deskd (GSettings `io.github.OlehDzhumyk.Desk*`): сторінки лише прив'язують
//! до них віджети, тож немає окремого вікна налаштувань і дублювання.

use std::collections::HashMap;

use adw::prelude::*;
use desk_proto::ClipboardProxy;
use desk_settings::{Group, Item, Key, Kind, Origin, Page, PageKind, Section, Widget, tr};
use gtk::{gio, glib};

use super::a11y;
use super::credits::{self, Section as CreditsSection};

fn t(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        desk_settings::i18n::gettext(s)
    }
}

/// Підсторінка з власним заголовком (кнопка «Назад» і назва).
fn subpage(title: &str, tag: &str, child: &impl IsA<gtk::Widget>) -> adw::NavigationPage {
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .show_start_title_buttons(false)
        .build();
    hb.add_css_class("flat");
    tv.add_top_bar(&hb);
    tv.set_content(Some(child));
    adw::NavigationPage::builder()
        .title(title)
        .tag(tag)
        .child(&tv)
        .build()
}

fn schema(id: &str) -> Option<gio::Settings> {
    let src = gio::SettingsSchemaSource::default()?;
    src.lookup(id, true)?;
    Some(gio::Settings::new(id))
}

fn badge(text: &str, extra: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("badge");
    if !extra.is_empty() {
        l.add_css_class(extra);
    }
    l
}

pub fn origin_label(o: &Origin) -> String {
    match o {
        Origin::Has(s) => t(s),
        Origin::BasedOn { project, licence } => {
            tr!(
                "Based on {project} ({licence})",
                project = project,
                licence = licence
            )
        }
    }
}

fn origin_group(origins: &[Origin]) -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::new();
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    for (i, o) in origins.iter().enumerate() {
        row.append(&badge(&origin_label(o), if i == 0 { "" } else { "origin" }));
    }
    a11y::label(&row, &tr!("Origin of the feature"));
    g.add(&row);
    g
}

/// Людський вигляд скорочення: ['<Shift><Super>v'] → «Shift+Super+V».
pub fn shortcut_label(accels: &glib::StrV) -> String {
    let parts: Vec<String> = accels
        .iter()
        .filter_map(|a| {
            let (key, mods) = gtk::accelerator_parse(a.as_str())?;
            Some(gtk::accelerator_get_label(key, mods).to_string())
        })
        .collect();
    if parts.is_empty() {
        tr!("none")
    } else {
        parts.join(", ")
    }
}

fn choice_label(k: &Key, value: &str) -> String {
    match &k.kind {
        Kind::Choice { choices, .. } => choices
            .iter()
            .find(|c| c.value == value)
            .map(|c| t(c.label))
            .unwrap_or_else(|| value.to_string()),
        _ => value.to_string(),
    }
}

/// Поточне значення ключа текстом (для рядків лише для читання).
fn value_text(k: &'static Key, s: &gio::Settings) -> String {
    match &k.kind {
        Kind::Accel { .. } => shortcut_label(&s.strv(k.name)),
        Kind::Choice { .. } => choice_label(k, &s.string(k.name)),
        Kind::StrList { .. } => s
            .strv(k.name)
            .iter()
            .map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        Kind::Str { .. } => s.string(k.name).to_string(),
        Kind::Bool { .. } => {
            if s.boolean(k.name) {
                tr!("on")
            } else {
                tr!("off")
            }
        }
        Kind::UInt { .. } => s.uint(k.name).to_string(),
    }
}

/// Рядок лише для читання: заголовок, опис і поточне значення (оновлюється разом із GSettings).
fn display_row(k: &'static Key, s: &gio::Settings) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(t(k.label)).build();
    let desc = t(k.description);
    match &k.kind {
        // коротке значення праворуч, опис — підзаголовком
        Kind::Accel { .. } | Kind::Bool { .. } | Kind::UInt { .. } => {
            if !desc.is_empty() {
                row.set_subtitle(&desc);
            }
            let val = gtk::Label::new(Some(&value_text(k, s)));
            val.add_css_class("dim-label");
            val.set_selectable(true);
            row.add_suffix(&val);
            let v2 = val.clone();
            s.connect_changed(Some(k.name), move |s, _| v2.set_label(&value_text(k, s)));
        }
        // довше значення — підзаголовком
        _ => {
            row.set_subtitle(&value_text(k, s));
            let r2 = row.clone();
            s.connect_changed(Some(k.name), move |s, _| r2.set_subtitle(&value_text(k, s)));
        }
    }
    row
}

fn edit_row(k: &'static Key, s: &gio::Settings) -> gtk::Widget {
    let desc = t(k.description);
    match &k.kind {
        Kind::Bool { .. } => {
            let r = adw::SwitchRow::builder()
                .title(t(k.label))
                .subtitle(desc)
                .build();
            s.bind(k.name, &r, "active").build();
            r.upcast()
        }
        Kind::UInt { min, max, step, .. } => {
            let r = adw::SpinRow::with_range(*min as f64, *max as f64, *step as f64);
            r.set_title(&t(k.label));
            r.set_subtitle(&desc);
            s.bind(k.name, &r, "value").build();
            r.upcast()
        }
        Kind::Choice { choices, .. } => {
            let labels: Vec<String> = choices.iter().map(|c| t(c.label)).collect();
            let model =
                gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
            let r = adw::ComboRow::builder()
                .title(t(k.label))
                .subtitle(desc)
                .model(&model)
                .build();
            let values: Vec<&'static str> = choices.iter().map(|c| c.value).collect();
            let v2 = values.clone();
            s.bind(k.name, &r, "selected")
                .mapping(move |v, _| {
                    let cur: String = v.get()?;
                    Some((values.iter().position(|x| *x == cur).unwrap_or(0) as u32).to_value())
                })
                .set_mapping(move |v, _| {
                    let i: u32 = v.get().ok()?;
                    v2.get(i as usize).map(|x| x.to_variant())
                })
                .build();
            r.upcast()
        }
        Kind::Str { .. } => {
            let r = adw::EntryRow::builder().title(t(k.label)).build();
            s.bind(k.name, &r, "text").build();
            if !desc.is_empty() {
                a11y::description(&r, &desc);
            }
            r.upcast()
        }
        // списки рядків і скорочення поки не редагуються у вікні
        Kind::StrList { .. } | Kind::Accel { .. } => display_row(k, s).upcast(),
    }
}

fn key_row(k: &'static Key, s: &gio::Settings) -> Option<gtk::Widget> {
    let row = match k.widget {
        Widget::Hidden => return None,
        Widget::Display => display_row(k, s).upcast(),
        Widget::Edit => edit_row(k, s),
    };
    if let Some(dep) = k.sensitive_if {
        s.bind(dep, &row, "sensitive").get_only().build();
    }
    Some(row)
}

fn clear_history(toasts: &adw::ToastOverlay) {
    let toasts = toasts.clone();
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(|| {
            zbus::block_on(async {
                let conn = zbus::Connection::session().await?;
                let p = ClipboardProxy::new(&conn).await?;
                p.clear(false).await
            })
        })
        .await;
        let msg = match r {
            Ok(Ok(())) => tr!("Clipboard history cleared"),
            Ok(Err(e)) => tr!("Could not clear the history: {error}", error = e),
            Err(_) => tr!("Could not clear the history"),
        };
        toasts.add_toast(adw::Toast::new(&msg));
    });
}

fn action_row(
    id: &'static str,
    label: &str,
    destructive: bool,
    toasts: &adw::ToastOverlay,
) -> adw::ButtonRow {
    let row = adw::ButtonRow::builder().title(t(label)).build();
    if destructive {
        row.add_css_class("destructive-action");
    }
    let toasts = toasts.clone();
    row.connect_activated(move |row| match id {
        "clear-history" => {
            let dlg = adw::AlertDialog::new(
                Some(&tr!("Clear the clipboard history?")),
                Some(&tr!(
                    "All items will be deleted, pinned ones too. This cannot be undone."
                )),
            );
            dlg.add_responses(&[("cancel", &tr!("Cancel")), ("clear", &tr!("Clear"))]);
            dlg.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            dlg.set_default_response(Some("cancel"));
            dlg.set_close_response("cancel");
            let toasts = toasts.clone();
            dlg.connect_response(None, move |_, r| {
                if r == "clear" {
                    clear_history(&toasts);
                }
            });
            dlg.present(Some(row));
        }
        other => eprintln!("themes: unknown settings action {other}"),
    });
    row
}

fn status_row(id: &'static str, label: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(t(label))
        .subtitle(tr!("checking…"))
        .build();
    if id != "deskd" {
        return row;
    }
    let r2 = row.clone();
    glib::spawn_future_local(async move {
        let res = gio::spawn_blocking(|| {
            zbus::block_on(async {
                let conn = zbus::Connection::session().await?;
                let p = ClipboardProxy::new(&conn).await?;
                p.count().await
            })
        })
        .await;
        r2.set_subtitle(&match res {
            Ok(Ok(n)) => tr!("running; items in the history: {count}", count = n),
            Ok(Err(e)) => tr!("unavailable: {error}", error = e),
            Err(_) => tr!("unavailable"),
        });
    });
    row
}

fn settings_group(
    g: &Group,
    settings: &HashMap<&'static str, gio::Settings>,
    toasts: &adw::ToastOverlay,
) -> Option<adw::PreferencesGroup> {
    let pg = adw::PreferencesGroup::new();
    if !g.title.is_empty() {
        pg.set_title(&t(g.title));
    }
    if !g.description.is_empty() {
        pg.set_description(Some(&t(g.description)));
    }
    let mut any = false;
    for item in g.items {
        let w: Option<gtk::Widget> = match item {
            Item::Key(k) => settings.get(k.schema).and_then(|s| key_row(k, s)),
            Item::Action {
                id,
                label,
                destructive,
            } => Some(action_row(id, label, *destructive, toasts).upcast()),
            Item::Status { id, label } => Some(status_row(id, label).upcast()),
        };
        if let Some(w) = w {
            pg.add(&w);
            any = true;
        }
    }
    any.then_some(pg)
}

fn credits_groups(page: &adw::PreferencesPage) {
    for CreditsSection { title, entries } in credits::parse(credits::CREDITS_MD) {
        if entries.is_empty() {
            continue;
        }
        let g = adw::PreferencesGroup::builder()
            .title(title.as_str())
            .build();
        for e in entries {
            let mut sub = Vec::new();
            if !e.used_for.is_empty() {
                sub.push(e.used_for.clone());
            }
            if !e.licence.is_empty() {
                sub.push(e.licence.clone());
            }
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&e.name).as_str())
                .subtitle(glib::markup_escape_text(&sub.join(" · ")).as_str())
                .build();
            if let Some(url) = e.url.clone() {
                let b = gtk::Button::builder()
                    .icon_name("adw-external-link-symbolic")
                    .valign(gtk::Align::Center)
                    .tooltip_text(url.as_str())
                    .build();
                b.add_css_class("flat");
                a11y::label(&b, &tr!("Open the website of {name}", name = e.name));
                b.connect_clicked(move |b| {
                    let l = gtk::UriLauncher::new(&url);
                    let win = b.root().and_downcast::<gtk::Window>();
                    l.launch(win.as_ref(), gio::Cancellable::NONE, |_| {});
                });
                row.add_suffix(&b);
            }
            g.add(&row);
        }
        page.add(&g);
    }
}

/// Сторінка з реєстру.
fn build_page(p: &'static Page, toasts: &adw::ToastOverlay) -> adw::NavigationPage {
    let page = adw::PreferencesPage::builder()
        .description(t(p.description))
        .build();
    if let Some(f) = p.feature
        && !f.origins.is_empty()
    {
        page.add(&origin_group(f.origins));
    }
    match p.kind {
        PageKind::Credits => credits_groups(&page),
        PageKind::Settings => {
            let mut settings: HashMap<&'static str, gio::Settings> = HashMap::new();
            let mut missing = Vec::new();
            for k in p.keys() {
                if settings.contains_key(k.schema) || missing.contains(&k.schema) {
                    continue;
                }
                match schema(k.schema) {
                    Some(s) => {
                        settings.insert(k.schema, s);
                    }
                    None => missing.push(k.schema),
                }
            }
            if !missing.is_empty() {
                page.add(
                    &adw::PreferencesGroup::builder()
                        .title(tr!("Settings unavailable"))
                        .description(tr!(
                            "The GSettings schema {schema} is not installed. Run data/install.sh.",
                            schema = missing.join(", ")
                        ))
                        .build(),
                );
            } else {
                for g in p.groups {
                    if let Some(pg) = settings_group(g, &settings, toasts) {
                        page.add(&pg);
                    }
                }
                if p.keys().any(|k| std::ptr::eq(k, &desk_settings::LANGUAGE))
                    && let Some(s) = settings.get(desk_settings::DESK_SCHEMA)
                {
                    let toasts = toasts.clone();
                    s.connect_changed(Some(desk_settings::LANGUAGE.name), move |_, _| {
                        let toast = adw::Toast::builder()
                            .title(tr!("The language changes after a restart"))
                            .button_label(tr!("Restart"))
                            .action_name("app.restart")
                            .timeout(0)
                            .build();
                        toasts.add_toast(toast);
                    });
                }
                // тримати об'єкти налаштувань живими разом зі сторінкою
                page.connect_destroy(move |_| {
                    let _ = &settings;
                });
            }
        }
    }
    subpage(&t(p.title), p.id, &page)
}

/// Сторінка «Функції» (навігація: список сторінок → сторінка). `open` — id сторінки з реєстру.
pub fn build(toasts: &adw::ToastOverlay, open: Option<&str>) -> adw::NavigationView {
    let nav = adw::NavigationView::new();
    let root = adw::PreferencesPage::builder()
        .description(t(desk_settings::ROOT_DESCRIPTION))
        .build();
    for section in [Section::Features, Section::App, Section::About] {
        let pages: Vec<&'static Page> = desk_settings::PAGES
            .iter()
            .copied()
            .filter(|p| p.section == section)
            .collect();
        if pages.is_empty() {
            continue;
        }
        let g = adw::PreferencesGroup::builder()
            .title(t(section.title()))
            .build();
        for p in pages {
            let row = adw::ActionRow::builder()
                .title(t(p.title))
                .subtitle(t(p.subtitle))
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(p.icon));
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let nav2 = nav.clone();
            let toasts = toasts.clone();
            row.connect_activated(move |_| nav2.push(&build_page(p, &toasts)));
            g.add(&row);
        }
        root.add(&g);
    }
    nav.add(
        &adw::NavigationPage::builder()
            .title(tr!("Features"))
            .tag("features")
            .child(&root)
            .build(),
    );
    if let Some(p) = open.and_then(desk_settings::page) {
        nav.push(&build_page(p, toasts));
    }
    nav
}

/// Ідентифікатори сторінок для `themes gui --page`.
pub fn page_ids() -> impl Iterator<Item = &'static str> {
    desk_settings::PAGES.iter().map(|p| p.id)
}
