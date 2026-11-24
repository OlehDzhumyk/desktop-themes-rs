// SPDX-License-Identifier: GPL-3.0-or-later
//! The Windows 11 quick settings flyout (Win+A, the tray pill of the taskbar): a 3 × 2 grid of
//! quick toggles (dark mode, do not disturb, private clipboard) and actions (screenshot, the
//! settings hub) over the volume of the default output and of each application that plays
//! sound, all from `deskd` (`Appearance`, `Focus`, `Clipboard`, `Mixer`).
//!
//! The content is read again each time the flyout opens. When deskd does not answer, its
//! toggles are insensitive and the audio section says that sound is not available.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use desk_settings::tr;
use gtk::glib;
use gtk::prelude::*;

use crate::deskd::{self, MixerApp, MixerDevice};
use crate::flyout::{self, Flyout};
use crate::layer_shell::Placement;
use crate::taskbar::{icon_of, name_widget};
use crate::{capture, launch};

const PLACEMENT: Placement = flyout::placement("desk-shell-quick-settings");
const WIDTH: i32 = 360;
/// Columns of the tile grid.
const TILE_COLUMNS: i32 = 3;

/// The value of a volume slider (0 to 100 %; deskd allows up to 150 %).
pub fn slider_value(volume: f64) -> f64 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The grid cell `(column, row)` of the tile at `index`.
pub fn tile_cell(index: usize) -> (i32, i32) {
    let i = i32::try_from(index).unwrap_or(0);
    (i % TILE_COLUMNS, i / TILE_COLUMNS)
}

/// A Fluent quick tile: an icon over a label.
fn tile_content(icon: &str, label: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&gtk::Image::from_icon_name(icon));
    let label = gtk::Label::builder()
        .label(label)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(12)
        .build();
    content.append(&label);
    content
}

fn toggle_tile(icon: &str, label: &str) -> gtk::ToggleButton {
    let tile = gtk::ToggleButton::builder()
        .child(&tile_content(icon, label))
        .hexpand(true)
        .sensitive(false)
        .build();
    tile.add_css_class("qs-tile");
    name_widget(&tile, label);
    tile
}

fn action_tile(icon: &str, label: &str) -> gtk::Button {
    let tile = gtk::Button::builder()
        .child(&tile_content(icon, label))
        .hexpand(true)
        .build();
    tile.add_css_class("qs-tile");
    name_widget(&tile, label);
    tile
}

fn volume_scale() -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale
}

fn percent_label() -> gtk::Label {
    let label = gtk::Label::builder().width_chars(4).xalign(1.0).build();
    label.add_css_class("qs-percent");
    label
}

/// The mute button of a volume row: its icon and its name follow the state.
fn update_mute(button: &gtk::Button, volume: f64, muted: bool) {
    button.set_icon_name(deskd::volume_icon(volume, muted));
    name_widget(button, &if muted { tr!("Unmute") } else { tr!("Mute") });
}

/// The flyout.
pub struct QuickSettings {
    flyout: Rc<Flyout>,
    dark: gtk::ToggleButton,
    focus: gtk::ToggleButton,
    private: gtk::ToggleButton,
    audio_status: gtk::Label,
    master_row: gtk::Box,
    master_name: gtk::Label,
    master: gtk::Scale,
    master_percent: gtk::Label,
    master_mute: gtk::Button,
    apps: gtk::Box,
    apps_expander: gtk::Expander,
    /// The default output: `(device id, muted)`.
    output: Cell<Option<(u32, bool)>>,
    /// Set while the code (not the user) changes a widget, so no deskd call follows.
    updating: Cell<bool>,
    pill_icon: RefCell<Option<glib::WeakRef<gtk::Image>>>,
}

impl QuickSettings {
    /// Build the (hidden) flyout.
    pub fn new(app: &gtk::Application) -> Rc<QuickSettings> {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.add_css_class("qs-body");
        let qs = Rc::new(QuickSettings {
            flyout: Flyout::new(app, &tr!("Quick settings"), &PLACEMENT, WIDTH, &content),
            dark: toggle_tile("weather-clear-night-symbolic", &tr!("Dark mode")),
            focus: toggle_tile("notifications-disabled-symbolic", &tr!("Do not disturb")),
            private: toggle_tile("view-conceal-symbolic", &tr!("Private clipboard")),
            audio_status: gtk::Label::builder().xalign(0.0).wrap(true).build(),
            master_row: gtk::Box::new(gtk::Orientation::Horizontal, 8),
            master_name: gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build(),
            master: volume_scale(),
            master_percent: percent_label(),
            master_mute: gtk::Button::from_icon_name("audio-volume-high-symbolic"),
            apps: gtk::Box::new(gtk::Orientation::Vertical, 4),
            apps_expander: gtk::Expander::builder()
                .label(tr!("Volume mixer"))
                .expanded(true)
                .visible(false)
                .build(),
            output: Cell::new(None),
            updating: Cell::new(false),
            pill_icon: RefCell::default(),
        });
        qs.layout(&content);
        qs.connect();
        qs
    }

    fn layout(self: &Rc<Self>, content: &gtk::Box) {
        let tiles = gtk::Grid::builder()
            .column_homogeneous(true)
            .row_spacing(8)
            .column_spacing(8)
            .build();
        let screenshot = action_tile("camera-photo-symbolic", &tr!("Screenshot"));
        let hub = action_tile(
            "preferences-desktop-appearance-symbolic",
            &tr!("Settings hub"),
        );
        let widgets: [&gtk::Widget; 5] = [
            self.dark.upcast_ref(),
            self.focus.upcast_ref(),
            self.private.upcast_ref(),
            screenshot.upcast_ref(),
            hub.upcast_ref(),
        ];
        for (i, w) in widgets.into_iter().enumerate() {
            let (column, row) = tile_cell(i);
            tiles.attach(w, column, row, 1, 1);
        }
        let me = Rc::downgrade(self);
        screenshot.connect_clicked(move |_| {
            if let Some(q) = me.upgrade() {
                q.flyout.hide();
            }
            capture::take_screenshot();
        });
        let me = Rc::downgrade(self);
        hub.connect_clicked(move |_| {
            if let Some(q) = me.upgrade() {
                q.flyout.hide();
            }
            launch::open_settings_hub();
        });
        content.append(&tiles);

        let audio = gtk::Box::new(gtk::Orientation::Vertical, 6);
        audio.add_css_class("qs-audio");
        let title = gtk::Label::builder()
            .label(tr!("Sound"))
            .xalign(0.0)
            .build();
        title.add_css_class("section-title");
        audio.append(&title);
        self.audio_status.add_css_class("dim");
        audio.append(&self.audio_status);
        self.master_name.add_css_class("dim");
        audio.append(&self.master_name);
        self.master
            .update_property(&[gtk::accessible::Property::Label(&tr!("Volume"))]);
        self.master_row.append(&self.master_mute);
        self.master_row.append(&self.master);
        self.master_row.append(&self.master_percent);
        self.master_row.set_visible(false);
        audio.append(&self.master_row);
        self.apps_expander.set_child(Some(&self.apps));
        audio.append(&self.apps_expander);
        content.append(&audio);
    }

    fn connect(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        self.dark.connect_toggled(move |t| match me.upgrade() {
            Some(q) if !q.updating.get() => deskd::set_dark_mode(t.is_active()),
            _ => {}
        });
        let me = Rc::downgrade(self);
        self.focus.connect_toggled(move |t| match me.upgrade() {
            Some(q) if !q.updating.get() => deskd::set_focus(t.is_active()),
            _ => {}
        });
        let me = Rc::downgrade(self);
        self.private.connect_toggled(move |t| match me.upgrade() {
            Some(q) if !q.updating.get() => deskd::set_clipboard_private(t.is_active()),
            _ => {}
        });
        let me = Rc::downgrade(self);
        self.master.connect_value_changed(move |s| {
            let Some(q) = me.upgrade() else { return };
            q.master_percent
                .set_label(&deskd::volume_percent(s.value()).to_string());
            if let (false, Some((id, muted))) = (q.updating.get(), q.output.get()) {
                deskd::set_device_volume(id, s.value());
                update_mute(&q.master_mute, s.value(), muted);
                q.set_pill(s.value(), muted);
            }
        });
        let me = Rc::downgrade(self);
        self.master_mute.connect_clicked(move |b| {
            let Some(q) = me.upgrade() else { return };
            if let Some((id, muted)) = q.output.get() {
                deskd::set_device_mute(id, !muted);
                q.output.set(Some((id, !muted)));
                update_mute(b, q.master.value(), !muted);
                q.set_pill(q.master.value(), !muted);
            }
        });
    }

    /// Run `f` with the deskd calls of the widgets turned off.
    fn quietly(&self, f: impl FnOnce()) {
        self.updating.set(true);
        f();
        self.updating.set(false);
    }

    fn set_toggle(&self, tile: &gtk::ToggleButton, state: Option<bool>) {
        self.quietly(|| {
            tile.set_sensitive(state.is_some());
            tile.set_active(state.unwrap_or(false));
        });
    }

    fn set_pill(&self, volume: f64, muted: bool) {
        if let Some(icon) = self
            .pill_icon
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
        {
            icon.set_icon_name(Some(deskd::volume_icon(volume, muted)));
        }
    }

    /// Read every state from deskd again.
    fn refresh(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        deskd::dark_mode(move |on| {
            if let Some(q) = me.upgrade() {
                q.set_toggle(&q.dark, on);
            }
        });
        let me = Rc::downgrade(self);
        deskd::focus_status(move |on| {
            if let Some(q) = me.upgrade() {
                q.set_toggle(&q.focus, on);
            }
        });
        let me = Rc::downgrade(self);
        deskd::clipboard_private(move |on| {
            if let Some(q) = me.upgrade() {
                q.set_toggle(&q.private, on);
            }
        });
        let me = Rc::downgrade(self);
        deskd::mixer_outputs(move |devices| {
            if let Some(q) = me.upgrade() {
                q.show_output(devices.as_deref());
            }
        });
        let me = Rc::downgrade(self);
        deskd::mixer_playing_apps(move |apps| {
            if let Some(q) = me.upgrade() {
                q.show_apps(apps.as_deref().unwrap_or_default());
            }
        });
    }

    fn show_output(&self, devices: Option<&[MixerDevice]>) {
        let device = devices.and_then(deskd::default_device);
        let status = match (devices, device) {
            (None, _) => Some(tr!(
                "Sound is not available: the desktop service is not running"
            )),
            (Some(_), None) => Some(tr!("No output device")),
            _ => None,
        };
        self.audio_status.set_visible(status.is_some());
        self.audio_status.set_label(status.as_deref().unwrap_or(""));
        self.master_row.set_visible(device.is_some());
        self.master_name.set_visible(device.is_some());
        self.output.set(device.map(|d| (d.id, d.muted)));
        if let Some(d) = device {
            self.master_name.set_label(&d.description);
            self.quietly(|| self.master.set_value(slider_value(d.volume)));
            self.master_percent
                .set_label(&deskd::volume_percent(d.volume).to_string());
            update_mute(&self.master_mute, d.volume, d.muted);
            self.set_pill(d.volume, d.muted);
        }
    }

    fn show_apps(&self, apps: &[MixerApp]) {
        while let Some(child) = self.apps.first_child() {
            self.apps.remove(&child);
        }
        self.apps_expander.set_visible(!apps.is_empty());
        for app in apps {
            self.apps.append(&app_row(app));
        }
    }

    /// The taskbar pill opens the flyout and shows the output volume.
    pub fn attach_pill(&self, button: &gtk::ToggleButton, icon: &gtk::Image) {
        self.flyout.set_button(button);
        *self.pill_icon.borrow_mut() = Some(icon.downgrade());
    }

    /// Open or close (the tray pill, `desk-shell --toggle-quick-settings`).
    pub fn toggle(self: &Rc<Self>) {
        self.flyout.toggle(|| self.refresh());
    }
}

/// One application of the volume mixer: icon, name, mute button, slider.
fn app_row(app: &MixerApp) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.add_css_class("qs-app");
    let icon = gtk::Image::from_gicon(&icon_of(&app.icon));
    icon.set_pixel_size(20);
    name_widget(&icon, &app.name);
    row.append(&icon);
    let mute = gtk::Button::new();
    update_mute(&mute, app.volume, app.muted);
    let scale = volume_scale();
    scale.set_value(slider_value(app.volume));
    scale.update_property(&[gtk::accessible::Property::Label(&tr!(
        "Volume of {app}",
        app = app.name
    ))]);
    scale.set_tooltip_text(Some(&app.name));
    let percent = percent_label();
    percent.set_label(&deskd::volume_percent(app.volume).to_string());
    let muted = Rc::new(Cell::new(app.muted));
    let (key, m, b, p) = (
        app.key.clone(),
        Rc::clone(&muted),
        mute.clone(),
        percent.clone(),
    );
    // connected after set_value, so building the row sends nothing to deskd
    scale.connect_value_changed(move |s| {
        deskd::set_app_volume(&key, s.value());
        p.set_label(&deskd::volume_percent(s.value()).to_string());
        update_mute(&b, s.value(), m.get());
    });
    let (key, s) = (app.key.clone(), scale.clone());
    mute.connect_clicked(move |b| {
        let now = !muted.get();
        muted.set(now);
        deskd::set_app_mute(&key, now);
        update_mute(b, s.value(), now);
    });
    row.append(&mute);
    row.append(&scale);
    row.append(&percent);
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliders_stay_in_range() {
        assert!((slider_value(0.42) - 0.42).abs() < f64::EPSILON);
        // deskd allows 150 %; the slider stops at 100 % like Windows 11
        assert!((slider_value(1.4) - 1.0).abs() < f64::EPSILON);
        assert!(slider_value(-0.2).abs() < f64::EPSILON);
        assert!(slider_value(f64::NAN).abs() < f64::EPSILON);
    }

    #[test]
    fn tiles_fill_rows_of_three() {
        assert_eq!(tile_cell(0), (0, 0));
        assert_eq!(tile_cell(2), (2, 0));
        assert_eq!(tile_cell(3), (0, 1));
        assert_eq!(tile_cell(4), (1, 1));
    }
}
