// SPDX-License-Identifier: GPL-3.0-or-later
//! The screenshot annotation editor (`themes capture-edit <path.png>`): a short-lived GTK 4 /
//! libadwaita window over one PNG (docs/spec/features/screenshots.md, editor).
//!
//! The image is decoded with `desk-capture`; the view is the original with the committed
//! annotations rendered by `desk_capture::annotate::apply` (cached), the drag in progress on top and
//! the crop shown as a dimmed outside. Save renders everything, crop included, into one RGBA image,
//! encodes it without metadata and replaces the file atomically: redaction is destructive and no
//! annotation or undo step is kept (AC-SHOT-05).

pub mod state;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use desk_capture::Image;
use desk_settings::i18n::gettext;
use desk_settings::tr;
use gtk::{cairo, gdk, gio, glib};

use state::{EditorState, NamedColor, Tool};

/// Zoom steps (1.0 = one image pixel per logical pixel).
const ZOOMS: [f64; 9] = [0.25, 0.33, 0.5, 0.67, 1.0, 1.5, 2.0, 3.0, 4.0];

struct Editor {
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    area: gtk::DrawingArea,
    toasts: adw::ToastOverlay,
    zoom_label: gtk::Label,
    style_box: gtk::Box,
    tool_buttons: RefCell<Vec<(Tool, gtk::ToggleButton)>>,
    path: RefCell<PathBuf>,
    base: Image,
    state: RefCell<EditorState>,
    /// `base` with the committed annotations (no crop), for the view.
    committed: RefCell<Image>,
    /// What the area paints: the view as a cairo surface.
    surface: RefCell<Option<cairo::ImageSurface>>,
    /// `None`: fit the window (never larger than 100 %).
    zoom: Cell<Option<f64>>,
    /// A view update is queued for the next idle (drag motion is coalesced).
    pending: Cell<bool>,
    /// The close request was answered (saved or discarded): close without asking.
    closing: Cell<bool>,
    drag_origin: Cell<(f64, f64)>,
}

/// Open the editor on `path`. Returns the exit code; an unreadable file is an error message.
pub fn run(path: &Path) -> Result<i32, String> {
    let bytes = std::fs::read(path).map_err(|e| {
        tr!(
            "Cannot open {path}: {error}",
            path = path.display(),
            error = e
        )
    })?;
    let base = desk_capture::codec::decode(&bytes).map_err(|e| {
        tr!(
            "Cannot open {path}: {error}",
            path = path.display(),
            error = e
        )
    })?;
    drop(bytes);
    let app = adw::Application::builder()
        .application_id(themes_core::APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let path = path.to_path_buf();
    let base = RefCell::new(Some(base));
    app.connect_activate(move |app| {
        if let Some(img) = base.borrow_mut().take() {
            build(app, path.clone(), img);
        }
    });
    let argv0 = std::env::args().next().unwrap_or_else(|| "themes".into());
    Ok(i32::from(app.run_with_args(&[argv0])))
}

fn accent() -> [u8; 4] {
    let c = adw::StyleManager::default().accent_color_rgba();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 before the cast"
    )]
    let v = |f: f32| (f.clamp(0.0, 1.0) * 255.0).round() as u8;
    [v(c.red()), v(c.green()), v(c.blue()), 255]
}

fn build(app: &adw::Application, path: PathBuf, base: Image) {
    let palette = state::palette(accent());
    let st = EditorState::new(base.width, base.height, palette[0].rgba);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(1100)
        .default_height(760)
        .build();
    let title = adw::WindowTitle::new("", &format!("{} × {}", base.width, base.height));
    let area = gtk::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .focusable(true)
        .build();
    let toasts = adw::ToastOverlay::new();
    let ed = Rc::new(Editor {
        window: window.clone(),
        title: title.clone(),
        area: area.clone(),
        toasts: toasts.clone(),
        zoom_label: gtk::Label::new(None),
        style_box: gtk::Box::new(gtk::Orientation::Horizontal, 6),
        tool_buttons: RefCell::new(Vec::new()),
        path: RefCell::new(path),
        committed: RefCell::new(base.clone()),
        base,
        state: RefCell::new(st),
        surface: RefCell::new(None),
        zoom: Cell::new(None),
        pending: Cell::new(false),
        closing: Cell::new(false),
        drag_origin: Cell::new((0.0, 0.0)),
    });

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    header.pack_start(&icon_button(
        "edit-undo-symbolic",
        &tr!("Undo"),
        "Ctrl+Z",
        "win.undo",
    ));
    header.pack_start(&icon_button(
        "edit-redo-symbolic",
        &tr!("Redo"),
        "Ctrl+Shift+Z",
        "win.redo",
    ));
    let save = gtk::Button::builder()
        .label(tr!("Save"))
        .action_name("win.save")
        .tooltip_text(shortcut_tip(&tr!("Save"), "Ctrl+S"))
        .build();
    save.add_css_class("suggested-action");
    header.pack_end(&save);
    header.pack_end(&icon_button(
        "document-save-as-symbolic",
        &tr!("Save As…"),
        "Ctrl+Shift+S",
        "win.save-as",
    ));
    header.pack_end(&icon_button(
        "edit-copy-symbolic",
        &tr!("Copy to the clipboard"),
        "Ctrl+C",
        "win.copy",
    ));

    let scroller = gtk::ScrolledWindow::builder()
        .child(&area)
        .hexpand(true)
        .vexpand(true)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&tool_bar(&ed, &palette));
    toolbar.set_content(Some(&scroller));
    toasts.set_child(Some(&toolbar));
    window.set_content(Some(&toasts));

    area.set_draw_func({
        let ed = Rc::downgrade(&ed);
        move |_, cr, w, h| {
            if let Some(ed) = ed.upgrade() {
                ed.draw(cr, f64::from(w), f64::from(h));
            }
        }
    });
    add_gestures(&ed);
    add_actions(&ed, app);
    add_keys(&ed);
    window.connect_close_request({
        let ed = Rc::downgrade(&ed);
        move |_| match ed.upgrade() {
            Some(ed) if !ed.closing.get() && ed.state.borrow().dirty() => {
                ed.ask_save_changes();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    ed.refresh_committed();
    ed.update_chrome();
    window.present();
    area.grab_focus();
}

fn shortcut_tip(name: &str, keys: &str) -> String {
    tr!("{name} ({keys})", name = name, keys = keys)
}

/// A flat icon button for an action, with a tooltip that names its shortcut.
fn icon_button(icon: &str, name: &str, keys: &str, action: &str) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name(icon)
        .action_name(action)
        .tooltip_text(shortcut_tip(name, keys))
        .build();
    crate::gui::a11y::label(&b, name);
    b.update_property(&[gtk::accessible::Property::KeyShortcuts(keys)]);
    b
}

/// The second bar: tools, colours, width and zoom.
fn tool_bar(ed: &Rc<Editor>, palette: &[NamedColor]) -> gtk::ScrolledWindow {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bar.add_css_class("toolbar");
    let tools = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    crate::gui::a11y::label(&tools, &tr!("Tools"));
    let mut first: Option<gtk::ToggleButton> = None;
    for tool in Tool::ALL {
        let b = gtk::ToggleButton::builder()
            .child(&glyph(move |cr, s| draw_tool(cr, s, tool)))
            .tooltip_text(shortcut_tip(
                &gettext(tool.label()),
                &tool.key().to_ascii_uppercase().to_string(),
            ))
            .build();
        b.add_css_class("flat");
        ed.tool_buttons.borrow_mut().push((tool, b.clone()));
        crate::gui::a11y::label(&b, &gettext(tool.label()));
        b.update_property(&[gtk::accessible::Property::KeyShortcuts(
            &tool.key().to_ascii_uppercase().to_string(),
        )]);
        if let Some(f) = &first {
            b.set_group(Some(f));
        } else {
            first = Some(b.clone());
        }
        b.set_active(tool == ed.state.borrow().tool);
        let weak = Rc::downgrade(ed);
        b.connect_toggled(move |b| {
            if b.is_active()
                && let Some(ed) = weak.upgrade()
            {
                ed.state.borrow_mut().tool = tool;
                ed.update_chrome();
            }
        });
        tools.append(&b);
    }
    bar.append(&tools);
    bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    // colours and width: only for the tools that draw
    let colors = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    crate::gui::a11y::label(&colors, &tr!("Colour"));
    let mut first: Option<gtk::ToggleButton> = None;
    for c in palette {
        let rgba = c.rgba;
        let b = gtk::ToggleButton::builder()
            .child(&glyph(move |cr, s| draw_swatch(cr, s, rgba)))
            .tooltip_text(gettext(c.name))
            .build();
        b.add_css_class("flat");
        crate::gui::a11y::label(&b, &gettext(c.name));
        if let Some(f) = &first {
            b.set_group(Some(f));
        } else {
            first = Some(b.clone());
        }
        b.set_active(rgba == ed.state.borrow().color);
        let weak = Rc::downgrade(ed);
        b.connect_toggled(move |b| {
            if b.is_active()
                && let Some(ed) = weak.upgrade()
            {
                ed.state.borrow_mut().color = rgba;
            }
        });
        colors.append(&b);
    }
    ed.style_box.append(&colors);
    let names: Vec<String> = state::WIDTHS.iter().map(|(_, n)| gettext(*n)).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let width = gtk::DropDown::from_strings(&names);
    width.set_tooltip_text(Some(&tr!("Line width")));
    crate::gui::a11y::label(&width, &tr!("Line width"));
    let current = ed.state.borrow().stroke;
    if let Some(i) = state::WIDTHS.iter().position(|(w, _)| *w == current) {
        width.set_selected(u32::try_from(i).unwrap_or(0));
    }
    let weak = Rc::downgrade(ed);
    width.connect_selected_notify(move |d| {
        if let Some(ed) = weak.upgrade()
            && let Some((w, _)) = usize::try_from(d.selected())
                .ok()
                .and_then(|i| state::WIDTHS.get(i))
        {
            ed.state.borrow_mut().stroke = *w;
        }
    });
    ed.style_box.append(&width);
    bar.append(&ed.style_box);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bar.append(&spacer);
    bar.append(&icon_button(
        "zoom-out-symbolic",
        &tr!("Zoom out"),
        "Ctrl+−",
        "win.zoom-out",
    ));
    ed.zoom_label.set_width_chars(6);
    bar.append(&ed.zoom_label);
    bar.append(&icon_button(
        "zoom-in-symbolic",
        &tr!("Zoom in"),
        "Ctrl+=",
        "win.zoom-in",
    ));
    bar.append(&icon_button(
        "zoom-original-symbolic",
        &tr!("Original size"),
        "Ctrl+1",
        "win.zoom-original",
    ));
    bar.append(&icon_button(
        "zoom-fit-best-symbolic",
        &tr!("Fit to window"),
        "Ctrl+0",
        "win.zoom-fit",
    ));
    gtk::ScrolledWindow::builder()
        .child(&bar)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .build()
}

/// A 16 px glyph drawn in the widget's foreground colour.
fn glyph(f: impl Fn(&cairo::Context, f64) + 'static) -> gtk::DrawingArea {
    let a = gtk::DrawingArea::builder()
        .content_width(16)
        .content_height(16)
        .build();
    a.set_draw_func(move |a, cr, w, h| {
        let c = a.color();
        cr.set_source_rgba(
            f64::from(c.red()),
            f64::from(c.green()),
            f64::from(c.blue()),
            f64::from(c.alpha()),
        );
        f(cr, f64::from(w.min(h)));
    });
    a
}

fn rgb(cr: &cairo::Context, c: [u8; 4]) {
    let f = |v: u8| f64::from(v) / 255.0;
    cr.set_source_rgb(f(c[0]), f(c[1]), f(c[2]));
}

fn draw_swatch(cr: &cairo::Context, s: f64, c: [u8; 4]) {
    let fg = cr.source();
    cr.arc(s / 2.0, s / 2.0, s / 2.0 - 1.0, 0.0, std::f64::consts::TAU);
    rgb(cr, c);
    let _ = cr.fill_preserve();
    // an outline in the foreground colour, so white and black stay visible on any background
    let _ = cr.set_source(&fg);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

fn draw_tool(cr: &cairo::Context, s: f64, tool: Tool) {
    let u = s / 16.0;
    cr.set_line_width(1.6 * u);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    match tool {
        Tool::Pen => {
            cr.move_to(2.0 * u, 12.0 * u);
            cr.curve_to(5.0 * u, 2.0 * u, 8.0 * u, 14.0 * u, 14.0 * u, 4.0 * u);
            let _ = cr.stroke();
        }
        Tool::Highlighter => {
            cr.set_line_width(5.0 * u);
            cr.set_line_cap(cairo::LineCap::Butt);
            cr.move_to(2.0 * u, 9.0 * u);
            cr.line_to(14.0 * u, 9.0 * u);
            cr.push_group();
            let _ = cr.stroke();
            let _ = cr.pop_group_to_source();
            let _ = cr.paint_with_alpha(0.5);
        }
        Tool::Line => {
            cr.move_to(3.0 * u, 13.0 * u);
            cr.line_to(13.0 * u, 3.0 * u);
            let _ = cr.stroke();
        }
        Tool::Arrow => {
            cr.move_to(3.0 * u, 13.0 * u);
            cr.line_to(12.0 * u, 4.0 * u);
            let _ = cr.stroke();
            cr.move_to(14.0 * u, 2.0 * u);
            cr.line_to(7.0 * u, 4.0 * u);
            cr.line_to(12.0 * u, 9.0 * u);
            cr.close_path();
            let _ = cr.fill();
        }
        Tool::Rect => {
            cr.rectangle(2.5 * u, 3.5 * u, 11.0 * u, 9.0 * u);
            let _ = cr.stroke();
        }
        Tool::Ellipse => {
            cr.save().ok();
            cr.translate(8.0 * u, 8.0 * u);
            cr.scale(6.0 * u, 4.8 * u);
            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
            cr.restore().ok();
            let _ = cr.stroke();
        }
        Tool::RedactSolid => {
            cr.rectangle(2.0 * u, 5.0 * u, 12.0 * u, 6.0 * u);
            let _ = cr.fill();
        }
        Tool::RedactPixelate => {
            for (x, y) in [(2.0, 2.0), (8.0, 8.0), (8.0, 2.0), (2.0, 8.0)] {
                cr.rectangle(x * u, y * u, 6.0 * u, 6.0 * u);
                if x == y {
                    let _ = cr.fill();
                } else {
                    let _ = cr.stroke();
                }
            }
        }
        Tool::Crop => {
            cr.move_to(4.0 * u, 1.5 * u);
            cr.line_to(4.0 * u, 12.0 * u);
            cr.line_to(14.5 * u, 12.0 * u);
            cr.move_to(1.5 * u, 4.0 * u);
            cr.line_to(12.0 * u, 4.0 * u);
            cr.line_to(12.0 * u, 14.5 * u);
            let _ = cr.stroke();
        }
    }
}

/// The view as a cairo surface (premultiplied ARGB32, native endian).
fn surface_of(img: &Image) -> Option<cairo::ImageSurface> {
    let (w, h) = (
        i32::try_from(img.width).ok()?,
        i32::try_from(img.height).ok()?,
    );
    let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
    let stride = usize::try_from(surf.stride()).ok()?;
    let row = img.width as usize * 4;
    {
        let mut data = surf.data().ok()?;
        for (y, src) in img.data.chunks_exact(row).enumerate() {
            let dst = &mut data[y * stride..y * stride + row];
            for (o, p) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                let a = u32::from(p[3]);
                let pm = |c: u8| (u32::from(c) * a + 127) / 255;
                let v = (a << 24) | (pm(p[0]) << 16) | (pm(p[1]) << 8) | pm(p[2]);
                *o = v.to_ne_bytes();
            }
        }
    }
    surf.mark_dirty();
    Some(surf)
}

fn add_gestures(ed: &Rc<Editor>) {
    let drag = gtk::GestureDrag::builder()
        .button(gdk::BUTTON_PRIMARY)
        .build();
    let weak = Rc::downgrade(ed);
    drag.connect_drag_begin(move |_, x, y| {
        if let Some(ed) = weak.upgrade() {
            ed.area.grab_focus();
            ed.drag_origin.set((x, y));
            let p = ed.to_image(x, y);
            ed.state.borrow_mut().drag_begin(p);
            ed.queue_view();
        }
    });
    let weak = Rc::downgrade(ed);
    drag.connect_drag_update(move |_, dx, dy| {
        if let Some(ed) = weak.upgrade() {
            let (x, y) = ed.drag_origin.get();
            let p = ed.to_image(x + dx, y + dy);
            ed.state.borrow_mut().drag_update(p);
            ed.queue_view();
        }
    });
    let weak = Rc::downgrade(ed);
    drag.connect_drag_end(move |_, dx, dy| {
        if let Some(ed) = weak.upgrade() {
            if !ed.state.borrow().dragging() {
                return; // cancelled with Esc
            }
            let (x, y) = ed.drag_origin.get();
            let p = ed.to_image(x + dx, y + dy);
            let made = {
                let mut st = ed.state.borrow_mut();
                st.drag_update(p);
                st.drag_commit()
            };
            if made {
                ed.refresh_committed();
            } else {
                ed.queue_view();
            }
            ed.update_chrome();
        }
    });
    ed.area.add_controller(drag);
    ed.area.set_cursor_from_name(Some("crosshair"));
}

fn add_keys(ed: &Rc<Editor>) {
    let keys = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(ed);
    keys.connect_key_pressed(move |_, key, _, mods| {
        let Some(ed) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let modifiers = gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK;
        if mods.intersects(modifiers) {
            return glib::Propagation::Proceed;
        }
        if let Some(tool) = key.to_unicode().and_then(Tool::from_key) {
            ed.select_tool(tool);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    ed.window.add_controller(keys);
}

fn add_actions(ed: &Rc<Editor>, app: &adw::Application) {
    let add = |name: &str, accels: &[&str], f: fn(&Rc<Editor>)| {
        let a = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(ed);
        a.connect_activate(move |_, _| {
            if let Some(ed) = weak.upgrade() {
                f(&ed);
            }
        });
        ed.window.add_action(&a);
        app.set_accels_for_action(&format!("win.{name}"), accels);
    };
    add("undo", &["<Control>z"], |ed| {
        if ed.state.borrow_mut().undo() {
            ed.refresh_committed();
            ed.update_chrome();
        }
    });
    add("redo", &["<Control><Shift>z", "<Control>y"], |ed| {
        if ed.state.borrow_mut().redo() {
            ed.refresh_committed();
            ed.update_chrome();
        }
    });
    add("copy", &["<Control>c"], Editor::copy);
    add("save", &["<Control>s"], |ed| {
        let path = ed.path.borrow().clone();
        ed.save_to(&path, false);
    });
    add("save-as", &["<Control><Shift>s"], Editor::save_as);
    add("close", &["Escape", "<Control>w"], |ed| {
        if ed.state.borrow().dragging() {
            ed.state.borrow_mut().drag_cancel();
            ed.queue_view();
        } else {
            ed.window.close();
        }
    });
    add(
        "zoom-in",
        &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
        |ed| ed.step_zoom(true),
    );
    add(
        "zoom-out",
        &["<Control>minus", "<Control>KP_Subtract"],
        |ed| ed.step_zoom(false),
    );
    add("zoom-original", &["<Control>1"], |ed| {
        ed.set_zoom(Some(1.0))
    });
    add("zoom-fit", &["<Control>0"], |ed| ed.set_zoom(None));
}

impl Editor {
    fn image_size(&self) -> (f64, f64) {
        (f64::from(self.base.width), f64::from(self.base.height))
    }

    /// Scale and top-left offset of the image in the area.
    fn layout(&self, w: f64, h: f64) -> (f64, f64, f64) {
        let (iw, ih) = self.image_size();
        let s = self
            .zoom
            .get()
            .unwrap_or_else(|| (w / iw).min(h / ih).min(1.0));
        let s = if s > 0.0 { s } else { 1.0 };
        (
            s,
            ((w - iw * s) / 2.0).max(0.0),
            ((h - ih * s) / 2.0).max(0.0),
        )
    }

    /// Area coordinates to image pixels (may lie outside the image; annotations are clipped).
    fn to_image(&self, x: f64, y: f64) -> [i32; 2] {
        let (w, h) = (f64::from(self.area.width()), f64::from(self.area.height()));
        let (s, ox, oy) = self.layout(w, h);
        let px = |v: f64| {
            let v = v.floor().clamp(-1.0e6, 1.0e6);
            // in range by the clamp above
            #[expect(clippy::cast_possible_truncation, reason = "clamped to ±1e6")]
            let i = v as i32;
            i
        };
        [px((x - ox) / s), px((y - oy) / s)]
    }

    fn draw(&self, cr: &cairo::Context, w: f64, h: f64) {
        let Some(surf) = self.surface.borrow().clone() else {
            return;
        };
        let (s, ox, oy) = self.layout(w, h);
        cr.translate(ox, oy);
        cr.scale(s, s);
        if cr.set_source_surface(&surf, 0.0, 0.0).is_err() {
            return;
        }
        let filter = if s < 1.0 {
            cairo::Filter::Good
        } else {
            cairo::Filter::Nearest
        };
        cr.source().set_filter(filter);
        let _ = cr.paint();
    }

    /// Render the committed annotations again (after a commit, undo or redo), then the view.
    fn refresh_committed(&self) {
        match self.state.borrow().render_committed(&self.base) {
            Ok(img) => *self.committed.borrow_mut() = img,
            Err(e) => self.toast(&e.to_string()),
        }
        self.refresh_view();
    }

    fn refresh_view(&self) {
        self.pending.set(false);
        let view = self.state.borrow().render_view(&self.committed.borrow());
        match view {
            Ok(img) => *self.surface.borrow_mut() = surface_of(&img),
            Err(e) => self.toast(&e.to_string()),
        }
        self.area.queue_draw();
    }

    /// Update the view at the next idle (many motion events, one render).
    fn queue_view(self: &Rc<Self>) {
        if self.pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(ed) = weak.upgrade() {
                ed.refresh_view();
            }
        });
    }

    /// Title, action states and the colour/width controls.
    fn update_chrome(&self) {
        let st = self.state.borrow();
        let name = self
            .path
            .borrow()
            .file_name()
            .map_or_default(|n| n.to_string_lossy().into_owned());
        let shown = if st.dirty() {
            format!("• {name}")
        } else {
            name
        };
        self.title.set_title(&shown);
        self.window.set_title(Some(&shown));
        for (name, on) in [("undo", st.can_undo()), ("redo", st.can_redo())] {
            if let Some(a) = self
                .window
                .lookup_action(name)
                .and_then(|a| a.downcast::<gio::SimpleAction>().ok())
            {
                a.set_enabled(on);
            }
        }
        self.style_box.set_sensitive(st.tool.uses_color());
        self.zoom_label.set_label(&match self.zoom.get() {
            Some(z) => tr!("{percent}%", percent = (z * 100.0).round()),
            None => tr!("Fit"),
        });
    }

    fn select_tool(&self, tool: Tool) {
        // activating the button updates the state (its `toggled` handler)
        if let Some((_, b)) = self.tool_buttons.borrow().iter().find(|(t, _)| *t == tool) {
            b.set_active(true);
        }
    }

    fn set_zoom(&self, z: Option<f64>) {
        self.zoom.set(z);
        let (iw, ih) = self.image_size();
        let size = |v: f64| {
            let v = v.round().clamp(0.0, 1.0e6);
            #[expect(clippy::cast_possible_truncation, reason = "clamped to 0..=1e6")]
            let i = v as i32;
            i
        };
        match z {
            Some(z) => {
                self.area.set_content_width(size(iw * z));
                self.area.set_content_height(size(ih * z));
            }
            None => {
                self.area.set_content_width(0);
                self.area.set_content_height(0);
            }
        }
        self.area.queue_draw();
        self.update_chrome();
    }

    fn step_zoom(&self, bigger: bool) {
        let (w, h) = (f64::from(self.area.width()), f64::from(self.area.height()));
        let current = self.layout(w, h).0;
        let next = if bigger {
            ZOOMS.iter().copied().find(|z| *z > current + 1e-3)
        } else {
            ZOOMS.iter().rev().copied().find(|z| *z < current - 1e-3)
        };
        if let Some(z) = next {
            self.set_zoom(Some(z));
        }
    }

    fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }

    /// The final PNG: every annotation rendered, crop applied, no metadata.
    fn render_png(&self) -> desk_capture::Result<Vec<u8>> {
        let img = self.state.borrow().apply_to_image(&self.base)?;
        desk_capture::codec::encode(&img)
    }

    fn copy(self: &Rc<Self>) {
        match self.render_png() {
            Ok(png) => {
                let provider =
                    gdk::ContentProvider::for_bytes("image/png", &glib::Bytes::from_owned(png));
                if self.window.clipboard().set_content(Some(&provider)).is_ok() {
                    self.toast(&tr!("Copied to the clipboard"));
                }
            }
            Err(e) => self.show_error(&tr!("Could not copy the screenshot"), &e.to_string()),
        }
    }

    /// Render and write to `path` atomically. `close`: close the window after a good save.
    fn save_to(&self, path: &Path, close: bool) -> bool {
        let res = self
            .render_png()
            .and_then(|png| state::write_png(path, &png));
        match res {
            Ok(()) => {
                *self.path.borrow_mut() = path.to_path_buf();
                self.state.borrow_mut().mark_saved();
                self.update_chrome();
                if close {
                    self.closing.set(true);
                    self.window.close();
                } else {
                    self.toast(&tr!("Saved"));
                }
                true
            }
            Err(e) => {
                self.show_error(&tr!("Could not save the screenshot"), &e.to_string());
                false
            }
        }
    }

    fn save_as(self: &Rc<Self>) {
        let current = self.path.borrow().clone();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&tr!("PNG images")));
        filter.add_mime_type("image/png");
        filter.add_suffix("png");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(tr!("Save As…"))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        if let Some(name) = current.file_name() {
            dialog.set_initial_name(Some(&name.to_string_lossy()));
        }
        if let Some(dir) = current.parent().filter(|d| !d.as_os_str().is_empty()) {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        let weak = Rc::downgrade(self);
        dialog.save(Some(&self.window), None::<&gio::Cancellable>, move |res| {
            if let (Some(ed), Ok(file)) = (weak.upgrade(), res)
                && let Some(path) = file.path()
            {
                ed.save_to(&path, false);
            }
        });
    }

    fn show_error(&self, heading: &str, body: &str) {
        let dlg = adw::AlertDialog::new(Some(heading), Some(body));
        dlg.add_response("close", &tr!("Close"));
        dlg.present(Some(&self.window));
    }

    /// The «Save changes?» question of a close request with unsaved annotations.
    fn ask_save_changes(self: &Rc<Self>) {
        let dlg = adw::AlertDialog::builder()
            .heading(tr!("Save changes?"))
            .body(tr!(
                "The annotations are lost if you close without saving. Redactions are applied to \
                 the file only when it is saved."
            ))
            .default_response("save")
            .close_response("cancel")
            .build();
        dlg.add_response("cancel", &tr!("Cancel"));
        dlg.add_response("discard", &tr!("Discard"));
        dlg.add_response("save", &tr!("Save"));
        dlg.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dlg.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        let weak = Rc::downgrade(self);
        dlg.connect_response(None, move |_, resp| {
            let Some(ed) = weak.upgrade() else {
                return;
            };
            match resp {
                "save" => {
                    let path = ed.path.borrow().clone();
                    ed.save_to(&path, true);
                }
                "discard" => {
                    ed.closing.set(true);
                    ed.window.close();
                }
                _ => {}
            }
        });
        dlg.present(Some(&self.window));
    }
}
