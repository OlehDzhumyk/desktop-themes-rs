// SPDX-License-Identifier: GPL-3.0-or-later
//! «Create theme from wallpaper…» on the Appearance page (docs/spec/features/theme-from-wallpaper.md):
//! file chooser (portal) → seed colour and light/dark preview with contrast numbers → name →
//! `themes tokens from-image` → toast with «Apply». The image is decoded by a `themes` child
//! process, so a malformed file cannot crash the window either.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use desk_settings::tr;
use gtk::{gio, glib};
use serde_json::Value;

use super::a11y;

/// Runs the `themes` binary of this window with `args` (blocking: call from a worker thread).
fn run_themes(args: &[&std::ffi::OsStr]) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let out = std::process::Command::new(exe)
        .arg("--json")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            out.status.to_string()
        } else {
            err
        });
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

async fn themes_async(args: Vec<std::ffi::OsString>) -> Result<Value, String> {
    gio::spawn_blocking(move || {
        let refs: Vec<&std::ffi::OsStr> = args.iter().map(|a| a.as_os_str()).collect();
        run_themes(&refs)
    })
    .await
    .unwrap_or_else(|_| Err(tr!("the helper process failed")))
}

/// A theme id from a file name: lowercase ASCII, other characters become `-`.
pub fn id_from_name(stem: &str) -> String {
    let mut id = String::new();
    for c in stem.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            id.push(c);
        } else if !id.ends_with('-') && !id.is_empty() {
            id.push('-');
        }
    }
    let id: String = id.trim_end_matches('-').chars().take(48).collect();
    if id.is_empty() {
        "wallpaper".into()
    } else {
        format!("{id}-theme")
    }
}

fn rgb(hex: &str) -> (f64, f64, f64) {
    let v = u32::from_str_radix(hex.trim_start_matches('#').get(..6).unwrap_or("808080"), 16)
        .unwrap_or(0x80_80_80);
    let ch = |s: u32| f64::from((v >> s) & 0xff) / 255.0;
    (ch(16), ch(8), ch(0))
}

/// A focusable colour square with an accessible name.
fn swatch(hex: &str, name: &str) -> gtk::DrawingArea {
    let d = gtk::DrawingArea::builder()
        .content_width(40)
        .content_height(40)
        .focusable(true)
        .tooltip_text(name)
        .valign(gtk::Align::Center)
        .build();
    a11y::label(&d, name);
    let c = rgb(hex);
    d.set_draw_func(move |_, cr, w, h| {
        cr.set_source_rgb(c.0, c.1, c.2);
        cr.rectangle(0.0, 0.0, f64::from(w), f64::from(h));
        let _ = cr.fill();
    });
    d
}

/// A small window mock-up in one variant: background, a text line and an accent button.
fn variant_preview(v: &Value, title: &str) -> gtk::Box {
    let s = |k: &str| v[k].as_str().unwrap_or("#808080").to_string();
    let (bg, fg, ac, afg) = (s("window_bg"), s("window_fg"), s("accent"), s("accent_fg"));
    let n = |k: &str| v[k].as_f64().unwrap_or(0.0);
    let text = tr!(
        "{variant}: text {text}:1, accent {accent}:1",
        variant = title,
        text = format!("{:.1}", n("text_contrast")),
        accent = format!("{:.1}", n("accent_contrast"))
    );
    let d = gtk::DrawingArea::builder()
        .content_width(150)
        .content_height(84)
        .focusable(true)
        .tooltip_text(text.as_str())
        .build();
    a11y::label(&d, &text);
    let (bg, fg, ac, afg) = (rgb(&bg), rgb(&fg), rgb(&ac), rgb(&afg));
    d.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        cr.set_source_rgb(bg.0, bg.1, bg.2);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        cr.set_source_rgb(fg.0, fg.1, fg.2);
        cr.rectangle(12.0, 14.0, w * 0.6, 8.0);
        cr.rectangle(12.0, 28.0, w * 0.4, 8.0);
        let _ = cr.fill();
        cr.set_source_rgb(ac.0, ac.1, ac.2);
        cr.rectangle(12.0, h - 30.0, w * 0.45, 20.0);
        let _ = cr.fill();
        cr.set_source_rgb(afg.0, afg.1, afg.2);
        cr.rectangle(20.0, h - 22.0, w * 0.45 - 16.0, 4.0);
        let _ = cr.fill();
    });
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    b.append(&d);
    let l = gtk::Label::new(Some(&text));
    l.add_css_class("caption");
    l.set_wrap(true);
    l.set_max_width_chars(20);
    b.append(&l);
    b
}

/// Entry point of the action row.
pub fn create_theme(parent: &gtk::Widget, toasts: &adw::ToastOverlay) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&tr!("Images")));
    filter.add_mime_type("image/*");
    filter.add_suffix("xml");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dlg = gtk::FileDialog::builder()
        .title(tr!("Choose an image"))
        .modal(true)
        .filters(&filters)
        .default_filter(&filter)
        .build();
    let win = parent.root().and_downcast::<gtk::Window>();
    let (parent, toasts) = (parent.clone(), toasts.clone());
    dlg.open(win.as_ref(), gio::Cancellable::NONE, move |r| {
        let Some(path) = r.ok().and_then(|f| f.path()) else {
            return;
        };
        let (parent, toasts) = (parent.clone(), toasts.clone());
        glib::spawn_future_local(async move {
            let args = vec![
                "tokens".into(),
                "seed".into(),
                path.clone().into_os_string(),
            ];
            match themes_async(args).await {
                Ok(v) => preview_dialog(&parent, &toasts, path, &v),
                Err(e) => toasts.add_toast(adw::Toast::new(&tr!(
                    "Could not read the image: {error}",
                    error = e
                ))),
            }
        });
    });
}

fn preview_dialog(parent: &gtk::Widget, toasts: &adw::ToastOverlay, path: PathBuf, v: &Value) {
    let seed = v["seed"].as_str().unwrap_or("#808080").to_string();
    let stem = path
        .file_stem()
        .map_or_else(|| "wallpaper".into(), |s| s.to_string_lossy().into_owned());
    let dlg = adw::AlertDialog::new(
        Some(&tr!("Create theme from image")),
        Some(&tr!(
            "Seed colour {colour}; GNOME accent: {accent}. Every text pair passes WCAG AA.",
            colour = seed,
            accent = v["accent"].as_str().unwrap_or("")
        )),
    );
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_halign(gtk::Align::Center);
    row.append(&swatch(&seed, &tr!("Seed {colour}", colour = seed)));
    row.append(&variant_preview(&v["light"], &tr!("Light")));
    row.append(&variant_preview(&v["dark"], &tr!("Dark")));
    body.append(&row);
    if v["monochrome"].as_bool() == Some(true) {
        let l = gtk::Label::new(Some(&tr!("The image has almost no colour")));
        l.add_css_class("warning");
        body.append(&l);
    }
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let name = adw::EntryRow::builder()
        .title(tr!("Theme name"))
        .text(&stem)
        .build();
    let id = adw::EntryRow::builder()
        .title(tr!("Theme id"))
        .text(id_from_name(&stem))
        .build();
    list.append(&name);
    list.append(&id);
    body.append(&list);
    dlg.set_extra_child(Some(&body));
    dlg.add_responses(&[("cancel", &tr!("Cancel")), ("create", &tr!("Create"))]);
    dlg.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dlg.set_default_response(Some("create"));
    dlg.set_close_response("cancel");
    {
        let d = dlg.downgrade();
        id.connect_changed(move |e| {
            if let Some(d) = d.upgrade() {
                d.set_response_enabled("create", themes_core::util::valid_id(&e.text()));
            }
        });
    }
    let toasts = toasts.clone();
    dlg.connect_response(None, move |_, r| {
        if r != "create" {
            return;
        }
        let (title, tid) = (name.text().to_string(), id.text().to_string());
        let (path, toasts) = (path.clone(), toasts.clone());
        glib::spawn_future_local(async move { create(&toasts, &path, title, tid).await });
    });
    dlg.present(Some(parent));
}

async fn create(toasts: &adw::ToastOverlay, path: &Path, title: String, id: String) {
    let ctx = themes_core::Ctx::from_env();
    if ctx.themes.join(&id).exists() {
        toasts.add_toast(adw::Toast::new(&tr!(
            "A theme with the id “{id}” already exists; choose another id",
            id = id
        )));
        return;
    }
    let title = if title.trim().is_empty() {
        id.clone()
    } else {
        title
    };
    let args = vec![
        "tokens".into(),
        "from-image".into(),
        path.as_os_str().to_owned(),
        "--name".into(),
        id.clone().into(),
        "--title".into(),
        title.clone().into(),
    ];
    match themes_async(args).await {
        Ok(_) => {
            let t = adw::Toast::builder()
                .title(tr!("Theme “{name}” created", name = title))
                .button_label(tr!("Apply"))
                .action_name("win.apply-theme")
                .action_target(&id.to_variant())
                .timeout(10)
                .build();
            toasts.add_toast(t);
        }
        Err(e) => toasts.add_toast(adw::Toast::new(&e)),
    }
}

/// Subtitle of the «Accent from wallpaper» status row (D-Bus `WallpaperAccent`).
pub fn status_text(st: (bool, String, String, bool, String, String, String)) -> String {
    let (enabled, seed, accent, mono, _uri, error, palette) = st;
    if !enabled {
        return tr!("off");
    }
    if !error.is_empty() {
        return tr!("Could not read the image: {error}", error = error);
    }
    if accent.is_empty() {
        return tr!("waiting for the wallpaper");
    }
    let s = tr!(
        "{accent} (seed colour {colour})",
        accent = accent,
        colour = seed
    );
    let s = if palette.is_empty() {
        s
    } else {
        format!(
            "{s}. {}",
            tr!(
                "The app colours of the theme “{theme}” follow it too",
                theme = palette
            )
        )
    };
    if mono {
        format!("{s}. {}", tr!("The image has almost no colour"))
    } else {
        s
    }
}

pub fn refresh_status(row: &adw::ActionRow) {
    let row = row.clone();
    glib::spawn_future_local(async move {
        let res = gio::spawn_blocking(|| {
            zbus::block_on(async {
                let conn = zbus::Connection::session().await?;
                desk_proto::appearance::AppearanceProxy::new(&conn)
                    .await?
                    .wallpaper_accent()
                    .await
            })
        })
        .await;
        row.set_subtitle(&match res {
            Ok(Ok(st)) => status_text(st),
            Ok(Err(e)) => tr!("unavailable: {error}", error = e),
            Err(_) => tr!("unavailable"),
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_file_names_are_valid() {
        for (stem, want) in [
            ("Sunset Beach 01", "sunset-beach-01-theme"),
            ("___", "wallpaper"),
            ("Захід", "wallpaper"),
            ("a--b", "a-b-theme"),
        ] {
            let id = id_from_name(stem);
            assert_eq!(id, want);
            assert!(themes_core::util::valid_id(&id), "{id}");
        }
    }
}
