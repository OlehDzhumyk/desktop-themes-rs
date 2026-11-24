// SPDX-License-Identifier: GPL-3.0-or-later
//! The Windows 11 quick settings flyout (Win+A, the tray pill of the taskbar): a grid of quick
//! toggles (Wi-Fi and Bluetooth when hardware is present, dark mode, do not disturb, private
//! clipboard) and actions (screenshot, colour picker, screen text, the settings hub), three per
//! row, over an optional display brightness slider, the volume of the default output and of each
//! application that plays sound, and an optional battery badge in the footer.
//!
//! The content is read again each time the flyout opens, and system indicators (Wi-Fi, Bluetooth,
//! brightness, battery) also update live over the system bus and sysfs. When deskd does not
//! answer, its toggles are insensitive and the audio section says that sound is not available.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use desk_settings::tr;
use gtk::glib;
use gtk::prelude::*;

use crate::deskd::{self, MixerApp, MixerDevice};
use crate::flyout::{self, Flyout};
use crate::layer_shell::Placement;
use crate::system::{self, BatteryState, SystemState, SystemStatus};
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

/// The value of the brightness slider (1 to 100 %).
pub fn brightness_slider_value(percent: u32) -> f64 {
    f64::from(percent.clamp(1, 100))
}

/// Whole brightness percentage (`1..=100`) from the brightness slider value.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to finite 1.0..=100.0 before casting to u32"
)]
pub fn brightness_percent_from_slider(value: f64) -> u32 {
    if value.is_finite() {
        value.clamp(1.0, 100.0).round() as u32
    } else {
        1
    }
}

/// Symbolic icon name for the taskbar Wi-Fi indicator.
pub fn wifi_icon_name(enabled: bool) -> &'static str {
    if enabled {
        "network-wireless-symbolic"
    } else {
        "network-wireless-offline-symbolic"
    }
}

/// Accessible label and tooltip for the Wi-Fi indicator.
pub fn wifi_tooltip(enabled: bool) -> String {
    if enabled {
        tr!("Wi-Fi connected")
    } else {
        tr!("Wi-Fi off")
    }
}

/// Accessible label and tooltip for the battery indicator.
pub fn battery_tooltip(battery: &BatteryState) -> String {
    let pct = system::battery_percent_rounded(battery.percent);
    if battery.charging {
        tr!("Battery {percent}% (charging)", percent = pct)
    } else {
        tr!("Battery {percent}%", percent = pct)
    }
}

/// Compact badge text shown beside the battery icon in the quick settings footer.
pub fn battery_badge_text(battery: &BatteryState) -> String {
    let pct = system::battery_percent_rounded(battery.percent);
    if battery.charging {
        tr!("{percent}% (charging)", percent = pct)
    } else {
        format!("{pct}%")
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

fn brightness_scale() -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 100.0, 1.0);
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

fn set_css_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

fn sync_pill_system(wifi: &gtk::Image, battery: &gtk::Image, state: &SystemState) {
    wifi.set_visible(state.wifi.available);
    if state.wifi.available {
        wifi.set_icon_name(Some(wifi_icon_name(state.wifi.enabled)));
        name_widget(wifi, &wifi_tooltip(state.wifi.enabled));
    }
    battery.set_visible(state.battery.present);
    if state.battery.present {
        battery.set_icon_name(Some(&state.battery.icon_name));
        set_css_class(battery, "battery-low", state.battery.low);
        name_widget(battery, &battery_tooltip(&state.battery));
    }
}

struct PillWidgets {
    volume: glib::WeakRef<gtk::Image>,
    wifi: glib::WeakRef<gtk::Image>,
    battery: glib::WeakRef<gtk::Image>,
}

/// The flyout.
pub struct QuickSettings {
    flyout: Rc<Flyout>,
    system: Rc<SystemStatus>,
    tiles_grid: gtk::Grid,
    wifi: gtk::ToggleButton,
    bluetooth: gtk::ToggleButton,
    dark: gtk::ToggleButton,
    focus: gtk::ToggleButton,
    private: gtk::ToggleButton,
    screenshot: gtk::Button,
    color: gtk::Button,
    ocr: gtk::Button,
    hub: gtk::Button,
    /// Last `(wifi_visible, bluetooth_visible)` applied to `tiles_grid`.
    tile_visibility: Cell<(bool, bool)>,
    brightness_row: gtk::Box,
    brightness_scale: gtk::Scale,
    brightness_percent: gtk::Label,
    audio_status: gtk::Label,
    master_row: gtk::Box,
    master_name: gtk::Label,
    master: gtk::Scale,
    master_percent: gtk::Label,
    master_mute: gtk::Button,
    apps: gtk::Box,
    apps_expander: gtk::Expander,
    footer: gtk::Box,
    battery_pill: gtk::Box,
    battery_icon: gtk::Image,
    battery_label: gtk::Label,
    /// The default output: `(device id, muted)`.
    output: Cell<Option<(u32, bool)>>,
    /// Set while the code (not the user) changes a widget, so no backend call follows.
    updating: Cell<bool>,
    /// The tray pill icons of each taskbar.
    pills: RefCell<Vec<PillWidgets>>,
}

impl QuickSettings {
    /// Build the (hidden) flyout.
    pub fn new(app: &gtk::Application, system: &Rc<SystemStatus>) -> Rc<QuickSettings> {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.add_css_class("qs-body");
        let tiles_grid = gtk::Grid::builder()
            .column_homogeneous(true)
            .row_spacing(8)
            .column_spacing(8)
            .build();
        let wifi = toggle_tile("network-wireless-symbolic", &tr!("Wi-Fi"));
        wifi.set_visible(false);
        let bluetooth = toggle_tile("bluetooth-active-symbolic", &tr!("Bluetooth"));
        bluetooth.set_visible(false);

        let qs = Rc::new(QuickSettings {
            flyout: Flyout::new(app, &tr!("Quick settings"), &PLACEMENT, WIDTH, &content),
            system: Rc::clone(system),
            tiles_grid,
            wifi,
            bluetooth,
            dark: toggle_tile("weather-clear-night-symbolic", &tr!("Dark mode")),
            focus: toggle_tile("notifications-disabled-symbolic", &tr!("Do not disturb")),
            private: toggle_tile("view-conceal-symbolic", &tr!("Private clipboard")),
            screenshot: action_tile("camera-photo-symbolic", &tr!("Screenshot")),
            color: action_tile("color-select-symbolic", &tr!("Color picker")),
            ocr: action_tile("insert-text-symbolic", &tr!("Screen text")),
            hub: action_tile(
                "preferences-desktop-appearance-symbolic",
                &tr!("Settings hub"),
            ),
            tile_visibility: Cell::new((false, false)),
            brightness_row: gtk::Box::new(gtk::Orientation::Horizontal, 8),
            brightness_scale: brightness_scale(),
            brightness_percent: percent_label(),
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
            footer: gtk::Box::new(gtk::Orientation::Horizontal, 8),
            battery_pill: gtk::Box::new(gtk::Orientation::Horizontal, 6),
            battery_icon: gtk::Image::from_icon_name("battery-level-100-symbolic"),
            battery_label: gtk::Label::builder().xalign(0.0).build(),
            output: Cell::new(None),
            updating: Cell::new(false),
            pills: RefCell::default(),
        });
        qs.layout(&content);
        qs.connect();
        qs.apply_system(&system.state());
        let me = Rc::downgrade(&qs);
        system.subscribe(move |state| {
            if let Some(q) = me.upgrade() {
                q.apply_system(state);
            }
        });
        qs
    }

    fn relayout_tiles(&self, show_wifi: bool, show_bt: bool) {
        while let Some(child) = self.tiles_grid.first_child() {
            self.tiles_grid.remove(&child);
        }
        self.wifi.set_visible(show_wifi);
        self.bluetooth.set_visible(show_bt);
        let mut widgets: Vec<&gtk::Widget> = Vec::with_capacity(9);
        if show_wifi {
            widgets.push(self.wifi.upcast_ref());
        }
        if show_bt {
            widgets.push(self.bluetooth.upcast_ref());
        }
        let fixed: [&gtk::Widget; 7] = [
            self.dark.upcast_ref(),
            self.focus.upcast_ref(),
            self.private.upcast_ref(),
            self.screenshot.upcast_ref(),
            self.color.upcast_ref(),
            self.ocr.upcast_ref(),
            self.hub.upcast_ref(),
        ];
        widgets.extend(fixed);
        for (i, w) in widgets.into_iter().enumerate() {
            let (column, row) = tile_cell(i);
            self.tiles_grid.attach(w, column, row, 1, 1);
        }
        self.tile_visibility.set((show_wifi, show_bt));
    }

    fn layout(self: &Rc<Self>, content: &gtk::Box) {
        self.relayout_tiles(false, false);
        content.append(&self.tiles_grid);

        self.brightness_row.add_css_class("qs-brightness");
        self.brightness_row.set_visible(false);
        let brightness_icon = gtk::Image::from_icon_name("display-brightness-symbolic");
        name_widget(&brightness_icon, &tr!("Brightness"));
        self.brightness_scale
            .update_property(&[gtk::accessible::Property::Label(&tr!("Brightness"))]);
        self.brightness_row.append(&brightness_icon);
        self.brightness_row.append(&self.brightness_scale);
        self.brightness_row.append(&self.brightness_percent);
        content.append(&self.brightness_row);

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

        self.footer.add_css_class("qs-footer");
        self.footer.set_visible(false);
        self.battery_pill.add_css_class("qs-battery");
        self.battery_pill.append(&self.battery_icon);
        self.battery_pill.append(&self.battery_label);
        self.footer.append(&self.battery_pill);
        content.append(&self.footer);
    }

    fn connect(self: &Rc<Self>) {
        self.connect_actions();
        self.connect_toggles();
        self.connect_sliders();
    }

    fn connect_actions(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        self.screenshot.connect_clicked(move |_| {
            if let Some(q) = me.upgrade() {
                q.flyout.hide();
            }
            capture::take_screenshot();
        });
        // one-shot commands of this program (slurp needs the flyout out of the way first)
        for (tile, flag) in [(&self.color, "--pick-color"), (&self.ocr, "--ocr-region")] {
            let me = Rc::downgrade(self);
            tile.connect_clicked(move |_| {
                if let Some(q) = me.upgrade() {
                    q.flyout.hide();
                }
                launch::spawn(&[&launch::self_binary(), flag]);
            });
        }
        let me = Rc::downgrade(self);
        self.hub.connect_clicked(move |_| {
            if let Some(q) = me.upgrade() {
                q.flyout.hide();
            }
            launch::open_settings_hub();
        });
    }

    fn connect_toggles(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        self.wifi.connect_toggled(move |t| match me.upgrade() {
            Some(q) if !q.updating.get() => q.system.set_wifi_enabled(t.is_active()),
            _ => {}
        });
        let me = Rc::downgrade(self);
        self.bluetooth.connect_toggled(move |t| match me.upgrade() {
            Some(q) if !q.updating.get() => q.system.set_bluetooth_powered(t.is_active()),
            _ => {}
        });
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
    }

    fn connect_sliders(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        self.brightness_scale.connect_value_changed(move |s| {
            let Some(q) = me.upgrade() else { return };
            let pct = brightness_percent_from_slider(s.value());
            q.brightness_percent.set_label(&format!("{pct}%"));
            if !q.updating.get() {
                q.system.set_brightness_percent(pct);
            }
        });
        let me = Rc::downgrade(self);
        self.master.connect_value_changed(move |s| {
            let Some(q) = me.upgrade() else { return };
            q.master_percent
                .set_label(&deskd::volume_percent(s.value()).to_string());
            if let (false, Some((id, muted))) = (q.updating.get(), q.output.get()) {
                deskd::set_device_volume(id, s.value());
                update_mute(&q.master_mute, s.value(), muted);
                q.set_pill_volume(s.value(), muted);
            }
        });
        let me = Rc::downgrade(self);
        self.master_mute.connect_clicked(move |b| {
            let Some(q) = me.upgrade() else { return };
            if let Some((id, muted)) = q.output.get() {
                deskd::set_device_mute(id, !muted);
                q.output.set(Some((id, !muted)));
                update_mute(b, q.master.value(), !muted);
                q.set_pill_volume(q.master.value(), !muted);
            }
        });
    }

    /// Run `f` with the backend calls of the widgets turned off.
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

    fn set_pill_volume(&self, volume: f64, muted: bool) {
        for pill in self.pills.borrow().iter() {
            if let Some(icon) = pill.volume.upgrade() {
                icon.set_icon_name(Some(deskd::volume_icon(volume, muted)));
            }
        }
    }

    fn apply_system(&self, state: &SystemState) {
        let want_vis = (state.wifi.available, state.bluetooth.available);
        if self.tile_visibility.get() != want_vis {
            self.relayout_tiles(want_vis.0, want_vis.1);
        }
        self.set_toggle(
            &self.wifi,
            state.wifi.available.then_some(state.wifi.enabled),
        );
        self.set_toggle(
            &self.bluetooth,
            state.bluetooth.available.then_some(state.bluetooth.powered),
        );

        self.brightness_row.set_visible(state.brightness.available);
        if state.brightness.available {
            let pct = state.brightness.percent.clamp(1, 100);
            self.quietly(|| {
                self.brightness_scale
                    .set_value(brightness_slider_value(pct))
            });
            self.brightness_percent.set_label(&format!("{pct}%"));
        }

        self.footer.set_visible(state.battery.present);
        if state.battery.present {
            self.battery_icon
                .set_icon_name(Some(&state.battery.icon_name));
            set_css_class(&self.battery_icon, "battery-low", state.battery.low);
            self.battery_label
                .set_label(&battery_badge_text(&state.battery));
            name_widget(&self.battery_pill, &battery_tooltip(&state.battery));
        }

        for pill in self.pills.borrow().iter() {
            if let (Some(wifi), Some(battery)) = (pill.wifi.upgrade(), pill.battery.upgrade()) {
                sync_pill_system(&wifi, &battery, state);
            }
        }
    }

    /// Read every state from deskd and system backends again.
    fn refresh(self: &Rc<Self>) {
        self.system.refresh();
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
            self.set_pill_volume(d.volume, d.muted);
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

    /// The taskbar pill opens the flyout and shows the output volume, Wi-Fi and battery icons.
    pub fn attach_pill(
        &self,
        button: &gtk::ToggleButton,
        volume: &gtk::Image,
        wifi: &gtk::Image,
        battery: &gtk::Image,
    ) {
        self.flyout.set_button(button);
        sync_pill_system(wifi, battery, &self.system.state());
        let mut pills = self.pills.borrow_mut();
        pills.retain(|p| p.volume.upgrade().is_some());
        pills.push(PillWidgets {
            volume: volume.downgrade(),
            wifi: wifi.downgrade(),
            battery: battery.downgrade(),
        });
    }

    /// The flyout is open (its slider shows volume changes: the on-screen display stays away).
    pub fn is_open(&self) -> bool {
        self.flyout.is_open()
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

        assert!((brightness_slider_value(0) - 1.0).abs() < f64::EPSILON);
        assert!((brightness_slider_value(65) - 65.0).abs() < f64::EPSILON);
        assert!((brightness_slider_value(150) - 100.0).abs() < f64::EPSILON);
        assert_eq!(brightness_percent_from_slider(42.4), 42);
        assert_eq!(brightness_percent_from_slider(0.0), 1);
        assert_eq!(brightness_percent_from_slider(f64::NAN), 1);
    }

    #[test]
    fn tiles_fill_rows_of_three() {
        assert_eq!(tile_cell(0), (0, 0));
        assert_eq!(tile_cell(2), (2, 0));
        assert_eq!(tile_cell(3), (0, 1));
        assert_eq!(tile_cell(4), (1, 1));
        assert_eq!(tile_cell(8), (2, 2));
    }

    #[test]
    fn tray_and_badge_formatters_reflect_wifi_and_battery_state() {
        assert_eq!(wifi_icon_name(true), "network-wireless-symbolic");
        assert_eq!(wifi_icon_name(false), "network-wireless-offline-symbolic");

        let bat = BatteryState {
            present: true,
            percent: 82.6,
            charging: false,
            icon_name: "battery-level-80-symbolic".into(),
            low: false,
        };
        assert_eq!(battery_badge_text(&bat), "83%");
        assert!(battery_tooltip(&bat).contains("83%"));

        let charging = BatteryState {
            charging: true,
            ..bat
        };
        assert!(battery_badge_text(&charging).contains("83%"));
        assert!(battery_tooltip(&charging).contains("83%"));
    }
}
