// SPDX-License-Identifier: GPL-3.0-or-later
//! Quick Look (`themes quick-look <paths>…`): a short-lived GTK 4 / libadwaita window that
//! previews files without opening an application (docs/spec/features/quick-look.md).
//!
//! It is a separate process: a decoder that crashes takes only this window with it, never the
//! shell or Files (AC-QL-09). What to show is decided by [`inspect`] (no GTK, unit-tested); this
//! module only draws it in the style of the active skin:
//!
//! * `macos`: a compact dark title strip with the name and «Open with…»;
//! * `windows11`: a Peek-style header with `<` `>`, the name, the counter and «Open with…»;
//! * `default`: a plain Adwaita window.
//!
//! Keys: `Space` or `Esc` closes, `Left`/`Right` (`Up`/`Down`) move through the files, `Return`
//! opens the file in its default application and closes, `I` toggles the details, and for
//! images `+`/`-` zoom and `0` fits. Nothing animates (AC-QL-08): the window simply appears.

pub mod inspect;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use adw::prelude::*;
use desk_settings::accessors::{DeskSettings, QuickLookSettings, Skin};
use desk_settings::tr;
use gtk::{gdk, gio, glib};

use inspect::{Content, ErrorReason, Inspected, Selection};

/// Zoom steps of images (1.0 = one image pixel per logical pixel).
const ZOOMS: [f64; 9] = [0.25, 0.33, 0.5, 0.67, 1.0, 1.5, 2.0, 3.0, 4.0];

const CSS: &str = "
.ql-mac-strip { background-color: #2b2b2b; color: #f2f2f2; min-height: 32px; }
.ql-mac-strip button { color: #f2f2f2; }
.ql-info { padding: 6px 12px; }
";

/// Options read from `io.github.OlehDzhumyk.Desk.QuickLook` (defaults without the schema).
#[derive(Clone, Copy, Debug)]
struct Options {
    max_text_bytes: usize,
    follow_selection: bool,
    skin: Skin,
}

struct Viewer {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    content: adw::Bin,
    toasts: adw::ToastOverlay,
    info: gtk::Revealer,
    info_label: gtk::Label,
    prev: gtk::Button,
    next: gtk::Button,
    opts: Options,
    selection: RefCell<Selection>,
    current: RefCell<Option<Inspected>>,
    /// The image of the current file, for zoom.
    picture: RefCell<Option<(gtk::Picture, gdk::Texture)>>,
    /// `None`: fit the window.
    zoom: Cell<Option<f64>>,
}

/// Open the previewer on `paths`. Returns the exit code.
pub fn run(paths: &[PathBuf]) -> Result<i32, String> {
    let ql = QuickLookSettings::new();
    if ql.as_ref().is_some_and(|s| !s.enabled()) {
        return Err(tr!(
            "Quick Look is turned off: turn it on in the Themes window, page «Quick Look»."
        ));
    }
    let opts = Options {
        max_text_bytes: ql
            .as_ref()
            .map_or(1024, QuickLookSettings::max_text_kb)
            .clamp(64, 8192) as usize
            * 1024,
        follow_selection: ql.as_ref().is_none_or(QuickLookSettings::follow_selection),
        skin: DeskSettings::new().map_or_else(Skin::default, |d| d.skin()),
    };
    let selection = Selection::new(paths.to_vec()).ok_or_else(|| tr!("No file to preview."))?;
    let app = adw::Application::builder()
        .application_id(themes_core::APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let selection = RefCell::new(Some(selection));
    app.connect_activate(move |app| {
        if let Some(s) = selection.borrow_mut().take() {
            build(app, s, opts);
        }
    });
    let argv0 = std::env::args().next().unwrap_or_else(|| "themes".into());
    Ok(i32::from(app.run_with_args(&[argv0])))
}

fn icon_button(icon: &str, name: &str) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(name)
        .build();
    crate::gui::a11y::label(&b, name);
    b
}

fn build(app: &adw::Application, selection: Selection, opts: Options) {
    if let Some(display) = gdk::Display::default() {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(CSS);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(if opts.skin == Skin::Macos { 820 } else { 960 })
        .default_height(if opts.skin == Skin::Macos { 600 } else { 680 })
        .build();
    let title = adw::WindowTitle::new("", "");
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    let prev = icon_button("go-previous-symbolic", &tr!("Previous file"));
    let next = icon_button("go-next-symbolic", &tr!("Next file"));
    let open_with = gtk::Button::with_label(&tr!("Open with…"));
    open_with.set_tooltip_text(Some(&tr!("Choose an application to open the file")));
    match opts.skin {
        Skin::Macos => {
            header.add_css_class("ql-mac-strip");
            header.add_css_class("flat");
            header.set_show_start_title_buttons(false);
            header.pack_end(&open_with);
        }
        Skin::Windows11 => {
            let nav = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            nav.add_css_class("linked");
            nav.append(&prev);
            nav.append(&next);
            header.pack_start(&nav);
            open_with.add_css_class("suggested-action");
            header.pack_end(&open_with);
        }
        Skin::Default => {
            header.pack_end(&open_with);
        }
    }
    let info_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    info_label.add_css_class("ql-info");
    info_label.add_css_class("caption");
    let info = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .child(&info_label)
        .build();
    let content = adw::Bin::builder().hexpand(true).vexpand(true).build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    toolbar.add_bottom_bar(&info);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&toolbar));
    window.set_content(Some(&toasts));

    let viewer = Rc::new(Viewer {
        window: window.clone(),
        title,
        content,
        toasts,
        info,
        info_label,
        prev: prev.clone(),
        next: next.clone(),
        opts,
        selection: RefCell::new(selection),
        current: RefCell::new(None),
        picture: RefCell::new(None),
        zoom: Cell::new(None),
    });
    let connect = |b: &gtk::Button, f: fn(&Rc<Viewer>)| {
        let weak = Rc::downgrade(&viewer);
        b.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                f(&v);
            }
        });
    };
    connect(&prev, |v| v.step(false));
    connect(&next, |v| v.step(true));
    connect(&open_with, Viewer::open_with);
    add_keys(&viewer);
    viewer.show_current();
    window.present();
}

fn add_keys(viewer: &Rc<Viewer>) {
    let keys = gtk::EventControllerKey::new();
    // before the text view: Space and the arrows belong to the previewer
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(viewer);
    keys.connect_key_pressed(move |_, key, _, mods| {
        let Some(v) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let modifiers = gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK;
        if mods.intersects(modifiers) {
            return glib::Propagation::Proceed;
        }
        match key {
            gdk::Key::space | gdk::Key::Escape => v.window.close(),
            gdk::Key::Right | gdk::Key::Down | gdk::Key::KP_Right | gdk::Key::KP_Down => {
                v.step(true);
            }
            gdk::Key::Left | gdk::Key::Up | gdk::Key::KP_Left | gdk::Key::KP_Up => v.step(false),
            gdk::Key::Return | gdk::Key::KP_Enter => v.open_default(),
            gdk::Key::i | gdk::Key::I => v.info.set_reveal_child(!v.info.reveals_child()),
            gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => v.step_zoom(true),
            gdk::Key::minus | gdk::Key::KP_Subtract => v.step_zoom(false),
            gdk::Key::_0 | gdk::Key::KP_0 => v.set_zoom(None),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    viewer.window.add_controller(keys);
}

/// «12 KB», «1.2 MB»: GLib's units, in the user's locale.
fn size_text(bytes: u64) -> String {
    glib::format_size(bytes).to_string()
}

fn date_text(t: SystemTime) -> Option<String> {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    let dt = glib::DateTime::from_unix_local(i64::try_from(secs).ok()?).ok()?;
    dt.format("%x %X").ok().map(|s| s.to_string())
}

fn reason_text(r: &ErrorReason) -> String {
    match r {
        ErrorReason::Missing => tr!("The file does not exist (it was moved or deleted)."),
        ErrorReason::PermissionDenied => tr!("You do not have permission to read the file."),
        ErrorReason::Other(e) => e.clone(),
    }
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hexpand(true)
        .vexpand(true)
        .build()
}

/// The card for files without a preview and for errors.
fn status_card(icon: &str, title: &str, description: &str) -> gtk::Widget {
    adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .description(glib::markup_escape_text(description).as_str())
        .build()
        .upcast()
}

impl Viewer {
    fn step(self: &Rc<Self>, forward: bool) {
        if !self.opts.follow_selection {
            return;
        }
        let moved = {
            let mut s = self.selection.borrow_mut();
            if forward { s.next() } else { s.previous() }
        };
        if moved {
            self.show_current();
        }
    }

    fn show_current(&self) {
        let path = self.selection.borrow().current().to_path_buf();
        let (index, total) = self.selection.borrow().position();
        let item = inspect::inspect(&path, self.opts.max_text_bytes);
        self.title.set_title(&item.display_name);
        let counter = if total > 1 {
            tr!("{index} of {total}", index = index, total = total)
        } else {
            String::new()
        };
        self.title.set_subtitle(&counter);
        self.window.set_title(Some(&if counter.is_empty() {
            item.display_name.clone()
        } else {
            format!("{} — {counter}", item.display_name)
        }));
        let navigable = self.opts.follow_selection;
        self.prev.set_sensitive(navigable && index > 1);
        self.next.set_sensitive(navigable && index < total);
        self.prev.set_visible(total > 1);
        self.next.set_visible(total > 1);
        self.picture.replace(None);
        self.zoom.set(None);
        let widget = self.content_widget(&item);
        self.content.set_child(Some(&widget));
        self.info_label.set_text(&info_text(&item));
        self.current.replace(Some(item));
    }

    fn content_widget(&self, item: &Inspected) -> gtk::Widget {
        match &item.content {
            Content::Image => self.image_widget(item),
            Content::Text {
                content, truncated, ..
            } => text_widget(content, *truncated),
            Content::Directory { entries, truncated } => dir_widget(entries, *truncated),
            Content::MetadataOnly { .. } => status_card(
                "text-x-generic-symbolic",
                &tr!("Preview not available"),
                &info_text(item),
            ),
            Content::Error { reason } => status_card(
                "dialog-error-symbolic",
                &tr!("Cannot read the file"),
                &reason_text(reason),
            ),
        }
    }

    fn image_widget(&self, item: &Inspected) -> gtk::Widget {
        // decoded by the image loaders of the system as passive data; failure: the details card
        match gdk::Texture::from_file(&gio::File::for_path(&item.path)) {
            Ok(texture) => {
                let picture = gtk::Picture::for_paintable(&texture);
                picture.set_content_fit(gtk::ContentFit::ScaleDown);
                picture.set_can_shrink(true);
                crate::gui::a11y::label(&picture, &item.display_name);
                self.picture.replace(Some((picture.clone(), texture)));
                scrolled(&picture).upcast()
            }
            Err(_) => status_card(
                "image-x-generic-symbolic",
                &tr!("Preview not available"),
                &info_text(item),
            ),
        }
    }

    fn set_zoom(&self, zoom: Option<f64>) {
        let Some((picture, texture)) = &*self.picture.borrow() else {
            return;
        };
        self.zoom.set(zoom);
        match zoom {
            None => {
                picture.set_size_request(-1, -1);
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::ScaleDown);
            }
            Some(z) => {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "image sizes times at most 4 fit in i32"
                )]
                let scale = |n: i32| (f64::from(n) * z).round() as i32;
                picture.set_can_shrink(false);
                picture.set_content_fit(gtk::ContentFit::Fill);
                picture.set_size_request(scale(texture.width()), scale(texture.height()));
            }
        }
    }

    fn step_zoom(&self, bigger: bool) {
        let Some((picture, texture)) = self.picture.borrow().clone() else {
            return;
        };
        // from "fit": start at the scale the picture has now
        let now = self.zoom.get().unwrap_or_else(|| {
            let w = f64::from(picture.width().max(1));
            (w / f64::from(texture.width().max(1))).min(1.0)
        });
        let z = if bigger {
            ZOOMS.iter().copied().find(|z| *z > now + 1e-3)
        } else {
            ZOOMS.iter().rev().copied().find(|z| *z < now - 1e-3)
        };
        if let Some(z) = z {
            self.set_zoom(Some(z));
        }
    }

    fn current_file(&self) -> Option<gio::File> {
        self.current
            .borrow()
            .as_ref()
            .filter(|i| !matches!(i.content, Content::Error { .. }))
            .map(|i| gio::File::for_path(&i.path))
    }

    /// `Return`: open in the default application and close.
    fn open_default(&self) {
        let Some(file) = self.current_file() else {
            return;
        };
        let ctx = gtk::prelude::WidgetExt::display(&self.window).app_launch_context();
        match gio::AppInfo::launch_default_for_uri(&file.uri(), Some(&ctx)) {
            Ok(()) => self.window.close(),
            Err(e) => self.toast(&tr!("Cannot open the file: {error}", error = e)),
        }
    }

    /// «Open with…»: the system chooser of applications.
    fn open_with(self: &Rc<Self>) {
        let Some(file) = self.current_file() else {
            return;
        };
        let launcher = gtk::FileLauncher::new(Some(&file));
        launcher.set_always_ask(true);
        let weak = Rc::downgrade(self);
        launcher.launch(Some(&self.window), None::<&gio::Cancellable>, move |r| {
            if let (Err(e), Some(v)) = (r, weak.upgrade())
                && !e.matches(gtk::DialogError::Dismissed)
            {
                v.toast(&tr!("Cannot open the file: {error}", error = e));
            }
        });
    }

    fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }
}

fn text_widget(content: &str, truncated: bool) -> gtk::Widget {
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(12)
        .right_margin(12)
        .build();
    view.buffer().set_text(content);
    crate::gui::a11y::label(&view, &tr!("File contents"));
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    if truncated {
        let banner = adw::Banner::new(&tr!("Showing the start of the file"));
        banner.set_revealed(true);
        page.append(&banner);
    }
    page.append(&scrolled(&view));
    page.upcast()
}

fn dir_widget(entries: &[inspect::DirEntry], truncated: bool) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    if truncated {
        let banner = adw::Banner::new(&tr!(
            "Only the first {count} items are shown",
            count = inspect::MAX_DIR_ENTRIES
        ));
        banner.set_revealed(true);
        page.append(&banner);
    }
    if entries.is_empty() {
        page.append(&status_card(
            "folder-symbolic",
            &tr!("The folder is empty"),
            "",
        ));
        return page.upcast();
    }
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");
    for e in entries {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&e.name).as_str())
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(if e.is_dir {
            "folder-symbolic"
        } else {
            "text-x-generic-symbolic"
        }));
        if let Some(size) = e.size {
            let l = gtk::Label::new(Some(&size_text(size)));
            l.add_css_class("dim-label");
            row.add_suffix(&l);
        }
        list.append(&row);
    }
    let clamp = adw::Clamp::builder()
        .maximum_size(720)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .child(&list)
        .build();
    page.append(&scrolled(&clamp));
    page.upcast()
}

/// The details: type, size, date, link target and path, one per line.
fn info_text(item: &Inspected) -> String {
    let mut lines = Vec::new();
    if !item.mime.is_empty() {
        lines.push(tr!("Type: {mime}", mime = item.mime));
    }
    if let Some(size) = item.size {
        lines.push(tr!("Size: {size}", size = size_text(size)));
    }
    if let Some(date) = item.modified.and_then(date_text) {
        lines.push(tr!("Modified: {date}", date = date));
    }
    if let Some(target) = &item.link_target {
        lines.push(tr!("Link to: {path}", path = target.display()));
    }
    lines.push(tr!(
        "Location: {path}",
        path = item.path.parent().unwrap_or(Path::new("")).display()
    ));
    lines.join("\n")
}
