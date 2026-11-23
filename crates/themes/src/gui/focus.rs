// SPDX-License-Identifier: GPL-3.0-or-later
//! «Focus» page parts that are not plain settings: the state row, «End focus», and the dialog
//! that starts, adds, edits and deletes modes. deskd owns the modes (`themes focus` speaks the
//! same D-Bus API); every call runs off the main thread.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use desk_settings::tr;
use gtk::{gio, glib};
use serde_json::{Value, json};
use themes_core::Error;

use crate::focus_cli::{call, modes, status_line};

/// Run a blocking deskd call off the main thread, then `done` on it.
fn run<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, Error> + Send + 'static,
    done: impl FnOnce(Result<T, Error>) + 'static,
) {
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(job)
            .await
            .unwrap_or_else(|_| Err(Error(tr!("unavailable"))));
        done(r);
    });
}

fn refresh_status(row: &adw::ActionRow) {
    let row = row.clone();
    run(
        || call(async |p| p.status().await),
        move |r| {
            row.set_subtitle(&match r {
                Ok(st) => status_line(&st),
                Err(e) => tr!("unavailable: {error}", error = e.0),
            });
        },
    );
}

/// The state row: refreshed when shown and after every action of this page.
pub fn status_row(label: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(label)
        .subtitle(tr!("checking…"))
        .build();
    row.add_css_class("property");
    row.add_css_class("focus-status");
    refresh_status(&row);
    row.connect_map(refresh_status);
    row
}

/// Refresh every focus state row of the window that holds `w`.
fn refresh_all(w: &impl IsA<gtk::Widget>) {
    let Some(root) = w.root() else { return };
    let mut stack = vec![root.upcast::<gtk::Widget>()];
    while let Some(x) = stack.pop() {
        if let Some(r) = x.downcast_ref::<adw::ActionRow>()
            && r.has_css_class("focus-status")
        {
            refresh_status(r);
        }
        let mut c = x.first_child();
        while let Some(ch) = c {
            c = ch.next_sibling();
            stack.push(ch);
        }
    }
}

fn toast_result(toasts: &adw::ToastOverlay, w: &impl IsA<gtk::Widget>, r: Result<(), Error>) {
    match r {
        Ok(()) => {}
        Err(e) => toasts.add_toast(adw::Toast::new(&e.0)),
    }
    refresh_all(w);
}

/// «End focus».
pub fn stop(row: &adw::ButtonRow, toasts: &adw::ToastOverlay) {
    let (row, toasts) = (row.clone(), toasts.clone());
    run(
        || call(async |p| p.stop().await),
        move |r| {
            if r.is_ok() {
                toasts.add_toast(adw::Toast::new(&tr!("Focus is off")));
            }
            toast_result(&toasts, &row, r);
        },
    );
}

fn summary(m: &Value) -> String {
    let mut parts = vec![];
    if m["block_banners"].as_bool() == Some(true) {
        parts.push(tr!("banners hidden"));
    }
    if m["mute_sounds"].as_bool() == Some(true) {
        parts.push(tr!("sounds muted"));
    }
    if let Some(a) = m["allowed_apps"].as_array().filter(|a| !a.is_empty()) {
        parts.push(tr!("{count} apps allowed", count = a.len()));
    }
    if let Some(s) = m["schedule"].as_object() {
        let days: Vec<&str> = s["days"]
            .as_array()
            .map_or_default(|a| a.iter().filter_map(Value::as_str).collect());
        parts.push(format!(
            "{} {}–{}",
            days.join(","),
            s["start"].as_str().unwrap_or(""),
            s["end"].as_str().unwrap_or("")
        ));
    }
    parts.join(" · ")
}

/// The dialog with the list of modes.
pub fn modes_dialog(parent: &adw::ButtonRow, toasts: &adw::ToastOverlay) {
    let dlg = adw::Dialog::builder()
        .title(tr!("Focus modes"))
        .content_width(520)
        .content_height(560)
        .build();
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .description(tr!(
            "Start a mode until you end it, or edit what it hides and when it starts by itself."
        ))
        .build();
    page.add(&group);
    let header = adw::HeaderBar::new();
    let add = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text(tr!("Add a mode"))
        .build();
    add.update_property(&[gtk::accessible::Property::Label(&tr!("Add a mode"))]);
    header.pack_start(&add);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&page));
    dlg.set_child(Some(&view));

    let reload: Reload = Rc::default();
    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::default();
    let fill = {
        let (group, toasts, dlg, reload) =
            (group.clone(), toasts.clone(), dlg.clone(), reload.clone());
        move || {
            let (group, toasts, dlg, reload, rows) = (
                group.clone(),
                toasts.clone(),
                dlg.clone(),
                reload.clone(),
                rows.clone(),
            );
            run(modes, move |r| {
                for old in rows.borrow_mut().drain(..) {
                    group.remove(&old);
                }
                match r {
                    Ok(list) => {
                        for m in list {
                            let row = mode_row(&m, &dlg, &toasts, &reload);
                            group.add(&row);
                            rows.borrow_mut().push(row);
                        }
                    }
                    Err(e) => toasts.add_toast(adw::Toast::new(&e.0)),
                }
            });
        }
    };
    *reload.borrow_mut() = Some(Box::new(fill.clone()));
    fill();
    {
        let (toasts, reload, dlg2) = (toasts.clone(), reload.clone(), dlg.clone());
        add.connect_clicked(move |_| edit_dialog(&dlg2, None, &toasts, &reload));
    }
    dlg.present(Some(parent));
}

type Reload = Rc<RefCell<Option<Box<dyn Fn()>>>>;

fn reload_now(reload: &Reload) {
    if let Some(f) = reload.borrow().as_ref() {
        f();
    }
}

fn mode_row(
    m: &Value,
    dlg: &adw::Dialog,
    toasts: &adw::ToastOverlay,
    reload: &Reload,
) -> adw::ActionRow {
    let id = m["id"].as_str().unwrap_or("").to_string();
    let label = m["label"].as_str().unwrap_or("").to_string();
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&label).as_str())
        .subtitle(glib::markup_escape_text(&summary(m)).as_str())
        .build();
    if let Some(icon) = m["icon"].as_str() {
        row.add_prefix(&gtk::Image::from_icon_name(icon));
    }
    let start = gtk::Button::builder()
        .label(tr!("Start"))
        .valign(gtk::Align::Center)
        .build();
    start.add_css_class("suggested-action");
    start.update_property(&[gtk::accessible::Property::Label(&tr!(
        "Start {mode}",
        mode = label
    ))]);
    let edit = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(tr!("Edit"))
        .build();
    edit.add_css_class("flat");
    edit.update_property(&[gtk::accessible::Property::Label(&tr!(
        "Edit {mode}",
        mode = label
    ))]);
    row.add_suffix(&start);
    row.add_suffix(&edit);
    {
        let (id, toasts, dlg) = (id.clone(), toasts.clone(), dlg.clone());
        start.connect_clicked(move |b| {
            let (id, toasts, dlg, b) = (id.clone(), toasts.clone(), dlg.clone(), b.clone());
            run(
                move || call(async |p| p.start(&id, 0).await),
                move |r| {
                    let ok = r.is_ok();
                    toast_result(&toasts, &b, r);
                    if ok {
                        dlg.close();
                    }
                },
            );
        });
    }
    {
        let (m, toasts, reload, dlg) = (m.clone(), toasts.clone(), reload.clone(), dlg.clone());
        edit.connect_clicked(move |_| edit_dialog(&dlg, Some(&m), &toasts, &reload));
    }
    if m["builtin"].as_bool() != Some(true) {
        let del = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(tr!("Delete"))
            .build();
        del.add_css_class("flat");
        del.update_property(&[gtk::accessible::Property::Label(&tr!(
            "Delete {mode}",
            mode = label
        ))]);
        row.add_suffix(&del);
        let (toasts, reload) = (toasts.clone(), reload.clone());
        del.connect_clicked(move |b| {
            let (id, toasts, reload, b) = (id.clone(), toasts.clone(), reload.clone(), b.clone());
            run(
                move || call(async |p| p.delete_mode(&id).await),
                move |r| {
                    toast_result(&toasts, &b, r);
                    reload_now(&reload);
                },
            );
        });
    }
    row
}

fn csv(v: &Value) -> String {
    v.as_array().map_or_default(|a| {
        a.iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    })
}

fn split(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// Add (`m` = None) or edit a mode.
fn edit_dialog(
    parent: &adw::Dialog,
    m: Option<&Value>,
    toasts: &adw::ToastOverlay,
    reload: &Reload,
) {
    let m = m.cloned().unwrap_or_else(|| json!({"block_banners": true}));
    let new = m.get("id").is_none();
    let dlg = adw::Dialog::builder()
        .title(if new {
            tr!("New focus mode")
        } else {
            tr!("Edit focus mode")
        })
        .content_width(480)
        .build();
    let page = adw::PreferencesPage::new();
    let g1 = adw::PreferencesGroup::new();
    let name = adw::EntryRow::builder().title(tr!("Name")).build();
    name.set_text(m["label"].as_str().or(m["name"].as_str()).unwrap_or(""));
    // built-in names are translated: they stay as they are
    name.set_sensitive(m["builtin"].as_bool() != Some(true));
    let banners = adw::SwitchRow::builder()
        .title(tr!("Hide notification banners"))
        .subtitle(tr!("Critical notifications always show"))
        .active(m["block_banners"].as_bool().unwrap_or(true))
        .build();
    let sounds = adw::SwitchRow::builder()
        .title(tr!("Mute event sounds"))
        .active(m["mute_sounds"].as_bool().unwrap_or(false))
        .build();
    let apps = adw::EntryRow::builder()
        .title(tr!("Allowed apps (app ids, comma-separated)"))
        .build();
    apps.set_text(&csv(&m["allowed_apps"]));
    for r in [
        name.upcast_ref::<gtk::Widget>(),
        banners.upcast_ref(),
        sounds.upcast_ref(),
        apps.upcast_ref(),
    ] {
        g1.add(r);
    }
    let sched = m["schedule"].as_object().cloned();
    let g2 = adw::PreferencesGroup::builder()
        .title(tr!("Schedule"))
        .build();
    let on = adw::SwitchRow::builder()
        .title(tr!("Start by itself"))
        .active(sched.is_some())
        .build();
    let days = adw::EntryRow::builder()
        .title(tr!("Days (mon, tue, wed, thu, fri, sat, sun)"))
        .build();
    let from = adw::EntryRow::builder().title(tr!("From (HH:MM)")).build();
    let to = adw::EntryRow::builder().title(tr!("To (HH:MM)")).build();
    let s = sched.unwrap_or_default();
    days.set_text(
        &s.get("days")
            .map_or_else(|| "mon, tue, wed, thu, fri".into(), csv),
    );
    from.set_text(s.get("start").and_then(Value::as_str).unwrap_or("22:00"));
    to.set_text(s.get("end").and_then(Value::as_str).unwrap_or("07:00"));
    g2.add(&on);
    for r in [&days, &from, &to] {
        on.bind_property("active", r, "sensitive")
            .sync_create()
            .build();
        g2.add(r);
    }
    page.add(&g1);
    page.add(&g2);
    let header = adw::HeaderBar::new();
    let save = gtk::Button::builder().label(tr!("Save")).build();
    save.add_css_class("suggested-action");
    header.pack_end(&save);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&page));
    dlg.set_child(Some(&view));
    {
        let (dlg, toasts, reload) = (dlg.clone(), toasts.clone(), reload.clone());
        save.connect_clicked(move |b| {
            let mut out = m.clone();
            let label = name.text().to_string();
            if let Some(o) = out.as_object_mut() {
                if new {
                    let id: String = label
                        .to_lowercase()
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                        .collect();
                    let id = id.trim_matches('-').to_string();
                    let id = if id.is_empty() {
                        format!("mode-{}", glib::random_int_range(1000, 9999))
                    } else {
                        id
                    };
                    o.insert("id".into(), id.into());
                }
                if o.get("builtin").and_then(Value::as_bool) != Some(true) {
                    o.insert("name".into(), label.into());
                }
                o.remove("label");
                o.remove("builtin");
                o.insert("block_banners".into(), banners.is_active().into());
                o.insert("mute_sounds".into(), sounds.is_active().into());
                o.insert("allowed_apps".into(), json!(split(&apps.text())));
                let schedule = if on.is_active() {
                    json!({
                        "days": split(&days.text().to_lowercase()),
                        "start": from.text().trim(),
                        "end": to.text().trim(),
                    })
                } else {
                    Value::Null
                };
                o.insert("schedule".into(), schedule);
            }
            let body = out.to_string();
            let (dlg, toasts, reload, b) = (dlg.clone(), toasts.clone(), reload.clone(), b.clone());
            run(
                move || call(async |p| p.save_mode(&body).await),
                move |r| {
                    let ok = r.is_ok();
                    toast_result(&toasts, &b, r);
                    if ok {
                        dlg.close();
                        reload_now(&reload);
                    }
                },
            );
        });
    }
    dlg.present(Some(parent));
}
