// SPDX-License-Identifier: GPL-3.0-or-later
//! Live system status for the taskbar tray pill and the quick settings flyout:
//!
//! * **Battery** over the system bus from UPower (`org.freedesktop.UPower`,
//!   `/org/freedesktop/UPower/devices/DisplayDevice`);
//! * **Brightness** from Linux `sysfs` (`/sys/class/backlight/*`, preferring `firmware` >
//!   `platform` > `raw`), written through `systemd-logind` (`SetBrightness`) and watched with
//!   `gio::FileMonitor` so hardware keys stay in sync;
//! * **Wi-Fi** over the system bus from NetworkManager (`org.freedesktop.NetworkManager`);
//! * **Bluetooth** over the system bus from BlueZ (`org.bluez`, first `org.bluez.Adapter1`).
//!
//! Every D-Bus call is asynchronous and non-blocking. When the system bus, a service or the
//! hardware is absent (desktops, VMs, headless containers), each indicator stays hidden with no
//! error logs.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gio;
use gtk::glib::{self, Variant};
use gtk::prelude::*;

const DBUS_BUS: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const OBJ_MANAGER_IFACE: &str = "org.freedesktop.DBus.ObjectManager";

const UPOWER_BUS: &str = "org.freedesktop.UPower";
const UPOWER_DISPLAY_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const UPOWER_DEVICE_IFACE: &str = "org.freedesktop.UPower.Device";

const NM_BUS: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";
const NM_DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
/// `NM_DEVICE_TYPE_WIFI` in NetworkManager's D-Bus API.
const NM_DEVICE_TYPE_WIFI: u32 = 2;

const BLUEZ_BUS: &str = "org.bluez";
const BLUEZ_ROOT_PATH: &str = "/";
const BLUEZ_ADAPTER_IFACE: &str = "org.bluez.Adapter1";

const LOGIND_BUS: &str = "org.freedesktop.login1";
const LOGIND_SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const LOGIND_SESSION_IFACE: &str = "org.freedesktop.login1.Session";

const TIMEOUT_MS: i32 = 5000;

/// Battery percentage at or below which a discharging battery shows a low-battery warning.
pub const BATTERY_LOW_THRESHOLD: f64 = 15.0;

/// Battery status reported by UPower's `DisplayDevice`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatteryState {
    pub present: bool,
    pub percent: f64,
    pub charging: bool,
    pub icon_name: String,
    pub low: bool,
}

/// Wi-Fi status reported by NetworkManager.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WifiState {
    pub available: bool,
    pub enabled: bool,
}

/// Bluetooth adapter status reported by BlueZ.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BluetoothState {
    pub available: bool,
    pub powered: bool,
    pub adapter_path: Option<String>,
}

/// Display backlight status from sysfs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrightnessState {
    pub available: bool,
    pub device: Option<String>,
    pub percent: u32,
    pub max_brightness: u32,
}

/// Snapshot of all system indicators.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemState {
    pub battery: BatteryState,
    pub wifi: WifiState,
    pub bluetooth: BluetoothState,
    pub brightness: BrightnessState,
}

/// Raw UPower `DisplayDevice` properties cached for incremental `PropertiesChanged` updates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawBattery {
    pub present: bool,
    pub percent: f64,
    pub state: u32,
    pub icon_name: String,
}

/// Raw NetworkManager wireless state cached across property and device updates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RawWifi {
    hardware_enabled: bool,
    enabled: bool,
    has_wifi_device: bool,
}

/// Sysfs backlight driver type, ordered by selection preference (`Firmware` first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BacklightType {
    Firmware,
    Platform,
    Raw,
    Other,
}

impl BacklightType {
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "firmware" => Self::Firmware,
            "platform" => Self::Platform,
            "raw" => Self::Raw,
            _ => Self::Other,
        }
    }
}

/// A discovered sysfs backlight device under `<sysfs>/class/backlight/<name>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BacklightDevice {
    pub name: String,
    pub path: PathBuf,
    pub max_brightness: u32,
    pub kind: BacklightType,
}

/// Whether a UPower `DeviceState` value represents active or pending charging.
///
/// UPower states: 1 = Charging, 5 = PendingCharge.
pub fn battery_is_charging(state: u32) -> bool {
    matches!(state, 1 | 5)
}

/// Rounded whole battery percentage in `0..=100`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to finite 0.0..=100.0 before casting to u32"
)]
pub fn battery_percent_rounded(percent: f64) -> u32 {
    if percent.is_finite() {
        percent.clamp(0.0, 100.0).round() as u32
    } else {
        0
    }
}

/// Fallback symbolic icon name when UPower returns an empty `IconName`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=10 before multiplying by 10"
)]
pub fn battery_icon_fallback(percent: f64, charging: bool) -> &'static str {
    let pct = if percent.is_finite() {
        percent.clamp(0.0, 100.0)
    } else {
        0.0
    };
    let bucket = ((pct / 10.0).round() as u32).min(10) * 10;
    match (bucket, charging) {
        (0, false) => "battery-level-0-symbolic",
        (0, true) => "battery-level-0-charging-symbolic",
        (10, false) => "battery-level-10-symbolic",
        (10, true) => "battery-level-10-charging-symbolic",
        (20, false) => "battery-level-20-symbolic",
        (20, true) => "battery-level-20-charging-symbolic",
        (30, false) => "battery-level-30-symbolic",
        (30, true) => "battery-level-30-charging-symbolic",
        (40, false) => "battery-level-40-symbolic",
        (40, true) => "battery-level-40-charging-symbolic",
        (50, false) => "battery-level-50-symbolic",
        (50, true) => "battery-level-50-charging-symbolic",
        (60, false) => "battery-level-60-symbolic",
        (60, true) => "battery-level-60-charging-symbolic",
        (70, false) => "battery-level-70-symbolic",
        (70, true) => "battery-level-70-charging-symbolic",
        (80, false) => "battery-level-80-symbolic",
        (80, true) => "battery-level-80-charging-symbolic",
        (90, false) => "battery-level-90-symbolic",
        (90, true) => "battery-level-90-charging-symbolic",
        (_, false) => "battery-level-100-symbolic",
        (_, true) => "battery-level-100-charged-symbolic",
    }
}

/// Derive [`BatteryState`] from cached UPower fields.
pub fn battery_state_from_raw(raw: &RawBattery) -> BatteryState {
    let percent = if raw.percent.is_finite() {
        raw.percent.clamp(0.0, 100.0)
    } else {
        0.0
    };
    let charging = battery_is_charging(raw.state);
    let low = raw.present && !charging && percent <= BATTERY_LOW_THRESHOLD;
    let icon_name = if raw.icon_name.is_empty() {
        battery_icon_fallback(percent, charging).to_owned()
    } else {
        raw.icon_name.clone()
    };
    BatteryState {
        present: raw.present,
        percent,
        charging,
        icon_name,
        low,
    }
}

/// Apply UPower property entries from a `GetAll` or `PropertiesChanged` dict onto `raw`.
pub fn apply_upower_props(raw: &mut RawBattery, dict: &glib::VariantDict) {
    if let Ok(Some(present)) = dict.lookup::<bool>("IsPresent") {
        raw.present = present;
    }
    if let Ok(Some(percent)) = dict.lookup::<f64>("Percentage") {
        raw.percent = percent;
    }
    if let Ok(Some(state)) = dict.lookup::<u32>("State") {
        raw.state = state;
    }
    if let Ok(Some(icon)) = dict.lookup::<String>("IconName") {
        raw.icon_name = icon;
    }
}

/// Parse a `Properties.GetAll("org.freedesktop.UPower.Device")` reply (`(a{sv})`).
pub fn parse_upower_get_all(reply: &Variant) -> Option<RawBattery> {
    if !reply.is_container() || reply.n_children() == 0 {
        return None;
    }
    let dict = glib::VariantDict::new(Some(&reply.child_value(0)));
    let mut raw = RawBattery::default();
    apply_upower_props(&mut raw, &dict);
    Some(raw)
}

/// Extract `(interface_name, changed_properties_dict)` from a `PropertiesChanged` signal (`(sa{sv}as)`).
pub fn parse_properties_changed(params: &Variant) -> Option<(String, glib::VariantDict)> {
    if !params.is_container() || params.n_children() < 2 {
        return None;
    }
    let iface = params.child_value(0).str()?.to_owned();
    let dict = glib::VariantDict::new(Some(&params.child_value(1)));
    Some((iface, dict))
}

/// Extract object paths from an `ao` `Variant` array.
fn object_path_array(v: &Variant) -> Vec<String> {
    if !v.is_container() {
        return Vec::new();
    }
    (0..v.n_children())
        .filter_map(|i| v.child_value(i).str().map(str::to_owned))
        .collect()
}

/// Parse a `Properties.GetAll("org.freedesktop.NetworkManager")` reply (`(a{sv})`).
///
/// Returns `(wireless_hardware_enabled, wireless_enabled, device_paths)`.
pub fn parse_nm_get_all(reply: &Variant) -> Option<(bool, bool, Vec<String>)> {
    if !reply.is_container() || reply.n_children() == 0 {
        return None;
    }
    let dict = glib::VariantDict::new(Some(&reply.child_value(0)));
    let hw = dict
        .lookup::<bool>("WirelessHardwareEnabled")
        .ok()
        .flatten()
        .unwrap_or(false);
    let enabled = dict
        .lookup::<bool>("WirelessEnabled")
        .ok()
        .flatten()
        .unwrap_or(false);
    let devices = dict
        .lookup_value("AllDevices", None)
        .or_else(|| dict.lookup_value("Devices", None))
        .map_or_default(|v| object_path_array(&v));
    Some((hw, enabled, devices))
}

/// Parse a `NetworkManager.GetDevices` reply (`(ao)`).
pub fn parse_object_paths_reply(reply: &Variant) -> Option<Vec<String>> {
    if !reply.is_container() || reply.n_children() == 0 {
        return None;
    }
    Some(object_path_array(&reply.child_value(0)))
}

/// Parse a `Properties.Get` reply containing a `u32` (`(v)`).
pub fn parse_property_u32(reply: &Variant) -> Option<u32> {
    if !reply.is_container() || reply.n_children() == 0 {
        return None;
    }
    let value = reply.child_value(0);
    if !value.is_type(glib::VariantTy::VARIANT) {
        return None;
    }
    value.as_variant()?.get::<u32>()
}

/// Parse a BlueZ `ObjectManager.GetManagedObjects` reply (`(a{oa{sa{sv}}})`).
///
/// Picks the lexicographically first object implementing `org.bluez.Adapter1`.
pub fn parse_bluez_objects(reply: &Variant) -> BluetoothState {
    if !reply.is_container() || reply.n_children() == 0 {
        return BluetoothState::default();
    }
    let objects = reply.child_value(0);
    if !objects.is_container() {
        return BluetoothState::default();
    }
    let mut adapters: Vec<(String, bool)> = Vec::new();
    for i in 0..objects.n_children() {
        let entry = objects.child_value(i);
        if !entry.is_container() || entry.n_children() < 2 {
            continue;
        }
        let Some(path) = entry.child_value(0).str().map(str::to_owned) else {
            continue;
        };
        let ifaces = entry.child_value(1);
        if !ifaces.is_container() {
            continue;
        }
        for j in 0..ifaces.n_children() {
            let iface_entry = ifaces.child_value(j);
            if !iface_entry.is_container() || iface_entry.n_children() < 2 {
                continue;
            }
            if iface_entry.child_value(0).str() == Some(BLUEZ_ADAPTER_IFACE) {
                let props = glib::VariantDict::new(Some(&iface_entry.child_value(1)));
                let powered = props
                    .lookup::<bool>("Powered")
                    .ok()
                    .flatten()
                    .unwrap_or(false);
                adapters.push((path.clone(), powered));
            }
        }
    }
    adapters.sort_by(|a, b| a.0.cmp(&b.0));
    match adapters.into_iter().next() {
        Some((path, powered)) => BluetoothState {
            available: true,
            powered,
            adapter_path: Some(path),
        },
        None => BluetoothState::default(),
    }
}

/// Arguments for `org.freedesktop.DBus.Properties.Set` (`(ssv)`).
pub fn set_property_args(iface: &str, prop: &str, value: Variant) -> Variant {
    (iface, prop, value).to_variant()
}

/// Arguments for `org.freedesktop.login1.Session.SetBrightness` (`(ssu)`).
pub fn set_brightness_args(device: &str, raw: u32) -> Variant {
    ("backlight", device, raw).to_variant()
}

/// Scan `<sysfs_root>/class/backlight/*` and pick the best backlight device.
///
/// Ignores devices with missing, unreadable, or zero `max_brightness`. Prefers `type` in the
/// order `firmware` > `platform` > `raw` > other, breaking ties deterministically by device name.
pub fn find_backlight(sysfs_root: &Path) -> Option<BacklightDevice> {
    let dir = sysfs_root.join("class").join("backlight");
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut found: Vec<BacklightDevice> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !std::fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(max_brightness) = read_u32_file(&path.join("max_brightness")).filter(|&m| m > 0)
        else {
            continue;
        };
        let kind = std::fs::read_to_string(path.join("type"))
            .map_or(BacklightType::Other, |s| BacklightType::parse(&s));
        found.push(BacklightDevice {
            name,
            path,
            max_brightness,
            kind,
        });
    }
    found.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
    found.into_iter().next()
}

fn read_u32_file(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Read the current raw brightness of `device`.
pub fn read_backlight_raw(device: &BacklightDevice) -> Option<u32> {
    read_u32_file(&device.path.join("brightness"))
}

/// Convert a raw sysfs brightness value to a whole percentage in `0..=100`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "fraction is in 0.0..=1.0, scaled to 0.0..=100.0"
)]
pub fn brightness_percent(raw: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let clamped = raw.min(max);
    ((f64::from(clamped) / f64::from(max)) * 100.0).round() as u32
}

/// Convert a whole percentage (`0..=100`) to a raw sysfs brightness value in `0..=max`.
///
/// Any positive percentage maps to at least `1` (when `max >= 1`) so 1% never blanks the panel.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "percentage is clamped to 0..=100 and scaled by u32 max"
)]
pub fn brightness_raw(percent: u32, max: u32) -> u32 {
    if max == 0 || percent == 0 {
        return 0;
    }
    let pct = percent.min(100);
    let raw = ((f64::from(pct) / 100.0) * f64::from(max)).round() as u32;
    raw.clamp(1, max)
}

type Listener = Box<dyn Fn(&SystemState)>;

/// Live controller for battery, brightness, Wi-Fi, and Bluetooth status.
pub struct SystemStatus {
    state: RefCell<SystemState>,
    bus: RefCell<Option<gio::DBusConnection>>,
    raw_battery: RefCell<RawBattery>,
    raw_wifi: RefCell<RawWifi>,
    wifi_probe_gen: Cell<u64>,
    backlight: RefCell<Option<BacklightDevice>>,
    backlight_monitor: RefCell<Option<gio::FileMonitor>>,
    subs: RefCell<Vec<gio::SignalSubscription>>,
    listeners: RefCell<Vec<Listener>>,
}

impl SystemStatus {
    /// Discover local backlight hardware and connect asynchronously to the system bus.
    pub fn new() -> Rc<SystemStatus> {
        let status = Rc::new(SystemStatus {
            state: RefCell::default(),
            bus: RefCell::default(),
            raw_battery: RefCell::default(),
            raw_wifi: RefCell::default(),
            wifi_probe_gen: Cell::new(0),
            backlight: RefCell::default(),
            backlight_monitor: RefCell::default(),
            subs: RefCell::default(),
            listeners: RefCell::default(),
        });
        status.init_backlight(Path::new("/sys"));
        status.connect_system_bus();
        status
    }

    /// Current system status snapshot.
    pub fn state(&self) -> SystemState {
        self.state.borrow().clone()
    }

    /// Run `cb` whenever any indicator changes.
    pub fn subscribe(&self, cb: impl Fn(&SystemState) + 'static) {
        self.listeners.borrow_mut().push(Box::new(cb));
    }

    fn notify(&self) {
        let snapshot = self.state.borrow().clone();
        let count = self.listeners.borrow().len();
        for i in 0..count {
            let listeners = self.listeners.borrow();
            if let Some(cb) = listeners.get(i) {
                cb(&snapshot);
            }
        }
    }

    fn init_backlight(self: &Rc<Self>, sysfs_root: &Path) {
        let Some(dev) = find_backlight(sysfs_root) else {
            return;
        };
        let percent =
            read_backlight_raw(&dev).map_or(100, |raw| brightness_percent(raw, dev.max_brightness));
        self.state.borrow_mut().brightness = BrightnessState {
            available: true,
            device: Some(dev.name.clone()),
            percent,
            max_brightness: dev.max_brightness,
        };
        let file = gio::File::for_path(dev.path.join("brightness"));
        if let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        {
            let me = Rc::downgrade(self);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(s) = me.upgrade() {
                    s.reload_backlight();
                }
            });
            *self.backlight_monitor.borrow_mut() = Some(monitor);
        }
        *self.backlight.borrow_mut() = Some(dev);
    }

    fn reload_backlight(&self) {
        let changed = {
            let dev_ref = self.backlight.borrow();
            let Some(dev) = dev_ref.as_ref() else {
                return;
            };
            let Some(raw) = read_backlight_raw(dev) else {
                return;
            };
            let percent = brightness_percent(raw, dev.max_brightness);
            let mut state = self.state.borrow_mut();
            if state.brightness.percent != percent {
                state.brightness.percent = percent;
                true
            } else {
                false
            }
        };
        if changed {
            self.notify();
        }
    }

    fn connect_system_bus(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        gio::bus_get(gio::BusType::System, gio::Cancellable::NONE, move |res| {
            let Ok(bus) = res else {
                // Silent when there is no system bus (headless container, minimal VM).
                return;
            };
            let Some(s) = me.upgrade() else { return };
            *s.bus.borrow_mut() = Some(bus.clone());
            s.subscribe_signals(&bus);
            s.refresh();
        });
    }

    fn subscribe_signals(self: &Rc<Self>, bus: &gio::DBusConnection) {
        let mut subs = self.subs.borrow_mut();
        self.subscribe_upower(bus, &mut subs);
        self.subscribe_network_manager(bus, &mut subs);
        self.subscribe_bluez(bus, &mut subs);
        self.subscribe_name_owners(bus, &mut subs);
    }

    fn subscribe_upower(
        self: &Rc<Self>,
        bus: &gio::DBusConnection,
        subs: &mut Vec<gio::SignalSubscription>,
    ) {
        let me = Rc::downgrade(self);
        subs.push(bus.subscribe_to_signal(
            Some(UPOWER_BUS),
            Some(PROPS_IFACE),
            Some("PropertiesChanged"),
            Some(UPOWER_DISPLAY_PATH),
            Some(UPOWER_DEVICE_IFACE),
            gio::DBusSignalFlags::NONE,
            move |sig| {
                if let Some(s) = me.upgrade()
                    && let Some((_, dict)) = parse_properties_changed(sig.parameters)
                {
                    let battery = {
                        let mut raw = s.raw_battery.borrow_mut();
                        apply_upower_props(&mut raw, &dict);
                        battery_state_from_raw(&raw)
                    };
                    s.set_battery(battery);
                }
            },
        ));
    }

    fn subscribe_network_manager(
        self: &Rc<Self>,
        bus: &gio::DBusConnection,
        subs: &mut Vec<gio::SignalSubscription>,
    ) {
        let me = Rc::downgrade(self);
        subs.push(bus.subscribe_to_signal(
            Some(NM_BUS),
            Some(PROPS_IFACE),
            Some("PropertiesChanged"),
            Some(NM_PATH),
            Some(NM_IFACE),
            gio::DBusSignalFlags::NONE,
            move |sig| {
                if let Some(s) = me.upgrade()
                    && let Some((_, dict)) = parse_properties_changed(sig.parameters)
                {
                    let mut changed = false;
                    {
                        let mut raw = s.raw_wifi.borrow_mut();
                        if let Ok(Some(hw)) = dict.lookup::<bool>("WirelessHardwareEnabled") {
                            raw.hardware_enabled = hw;
                            changed = true;
                        }
                        if let Ok(Some(en)) = dict.lookup::<bool>("WirelessEnabled") {
                            raw.enabled = en;
                            changed = true;
                        }
                    }
                    if changed {
                        s.sync_wifi_state();
                    }
                }
            },
        ));
        for member in ["DeviceAdded", "DeviceRemoved"] {
            let me = Rc::downgrade(self);
            subs.push(bus.subscribe_to_signal(
                Some(NM_BUS),
                Some(NM_IFACE),
                Some(member),
                Some(NM_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |_| {
                    if let Some(s) = me.upgrade() {
                        s.query_wifi();
                    }
                },
            ));
        }
    }

    fn subscribe_bluez(
        self: &Rc<Self>,
        bus: &gio::DBusConnection,
        subs: &mut Vec<gio::SignalSubscription>,
    ) {
        for member in ["InterfacesAdded", "InterfacesRemoved"] {
            let me = Rc::downgrade(self);
            subs.push(bus.subscribe_to_signal(
                Some(BLUEZ_BUS),
                Some(OBJ_MANAGER_IFACE),
                Some(member),
                Some(BLUEZ_ROOT_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |_| {
                    if let Some(s) = me.upgrade() {
                        s.query_bluetooth();
                    }
                },
            ));
        }
        let me = Rc::downgrade(self);
        subs.push(bus.subscribe_to_signal(
            Some(BLUEZ_BUS),
            Some(PROPS_IFACE),
            Some("PropertiesChanged"),
            None,
            Some(BLUEZ_ADAPTER_IFACE),
            gio::DBusSignalFlags::NONE,
            move |sig| {
                if let Some(s) = me.upgrade()
                    && let Some((_, dict)) = parse_properties_changed(sig.parameters)
                    && let Ok(Some(powered)) = dict.lookup::<bool>("Powered")
                {
                    let matches =
                        s.state.borrow().bluetooth.adapter_path.as_deref() == Some(sig.object_path);
                    if matches {
                        let changed = {
                            let mut st = s.state.borrow_mut();
                            if st.bluetooth.powered != powered {
                                st.bluetooth.powered = powered;
                                true
                            } else {
                                false
                            }
                        };
                        if changed {
                            s.notify();
                        }
                    }
                }
            },
        ));
    }

    fn subscribe_name_owners(
        self: &Rc<Self>,
        bus: &gio::DBusConnection,
        subs: &mut Vec<gio::SignalSubscription>,
    ) {
        for service in [UPOWER_BUS, NM_BUS, BLUEZ_BUS] {
            let me = Rc::downgrade(self);
            subs.push(bus.subscribe_to_signal(
                Some(DBUS_BUS),
                Some(DBUS_BUS),
                Some("NameOwnerChanged"),
                Some(DBUS_PATH),
                Some(service),
                gio::DBusSignalFlags::NONE,
                move |sig| {
                    let Some(s) = me.upgrade() else { return };
                    let new_owner = sig
                        .parameters
                        .get::<(String, String, String)>()
                        .map_or_default(|(_, _, new)| new);
                    match service {
                        UPOWER_BUS if new_owner.is_empty() => {
                            *s.raw_battery.borrow_mut() = RawBattery::default();
                            s.set_battery(BatteryState::default());
                        }
                        UPOWER_BUS => s.query_battery(),
                        NM_BUS if new_owner.is_empty() => {
                            *s.raw_wifi.borrow_mut() = RawWifi::default();
                            s.sync_wifi_state();
                        }
                        NM_BUS => s.query_wifi(),
                        BLUEZ_BUS if new_owner.is_empty() => {
                            s.set_bluetooth(BluetoothState::default());
                        }
                        BLUEZ_BUS => s.query_bluetooth(),
                        _ => {}
                    }
                },
            ));
        }
    }

    /// Re-read backlight and (if connected) all system bus indicators.
    pub fn refresh(self: &Rc<Self>) {
        self.reload_backlight();
        if self.bus.borrow().is_some() {
            self.query_battery();
            self.query_wifi();
            self.query_bluetooth();
        }
    }

    fn set_battery(&self, battery: BatteryState) {
        let changed = {
            let mut st = self.state.borrow_mut();
            if st.battery != battery {
                st.battery = battery;
                true
            } else {
                false
            }
        };
        if changed {
            self.notify();
        }
    }

    fn sync_wifi_state(&self) {
        let wifi = {
            let raw = self.raw_wifi.borrow();
            WifiState {
                available: raw.hardware_enabled && raw.has_wifi_device,
                enabled: raw.enabled,
            }
        };
        let changed = {
            let mut st = self.state.borrow_mut();
            if st.wifi != wifi {
                st.wifi = wifi;
                true
            } else {
                false
            }
        };
        if changed {
            self.notify();
        }
    }

    fn set_bluetooth(&self, bluetooth: BluetoothState) {
        let changed = {
            let mut st = self.state.borrow_mut();
            if st.bluetooth != bluetooth {
                st.bluetooth = bluetooth;
                true
            } else {
                false
            }
        };
        if changed {
            self.notify();
        }
    }

    fn query_battery(self: &Rc<Self>) {
        let Some(bus) = self.bus.borrow().clone() else {
            return;
        };
        let me = Rc::downgrade(self);
        bus.call(
            Some(UPOWER_BUS),
            UPOWER_DISPLAY_PATH,
            PROPS_IFACE,
            "GetAll",
            Some(&(UPOWER_DEVICE_IFACE,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
            move |reply| {
                let Some(s) = me.upgrade() else { return };
                match reply.ok().and_then(|v| parse_upower_get_all(&v)) {
                    Some(raw) => {
                        let battery = battery_state_from_raw(&raw);
                        *s.raw_battery.borrow_mut() = raw;
                        s.set_battery(battery);
                    }
                    None => {
                        *s.raw_battery.borrow_mut() = RawBattery::default();
                        s.set_battery(BatteryState::default());
                    }
                }
            },
        );
    }

    fn query_wifi(self: &Rc<Self>) {
        let Some(bus) = self.bus.borrow().clone() else {
            return;
        };
        let me = Rc::downgrade(self);
        let bus_clone = bus.clone();
        bus.call(
            Some(NM_BUS),
            NM_PATH,
            PROPS_IFACE,
            "GetAll",
            Some(&(NM_IFACE,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
            move |reply| {
                let Some(s) = me.upgrade() else { return };
                let Some((hw, enabled, devices)) = reply.ok().and_then(|v| parse_nm_get_all(&v))
                else {
                    *s.raw_wifi.borrow_mut() = RawWifi::default();
                    s.sync_wifi_state();
                    return;
                };
                {
                    let mut raw = s.raw_wifi.borrow_mut();
                    raw.hardware_enabled = hw;
                    raw.enabled = enabled;
                }
                if !devices.is_empty() {
                    s.probe_wifi_devices(&bus_clone, devices);
                } else {
                    // Fallback to GetDevices in case AllDevices was absent from GetAll
                    let me2 = Rc::downgrade(&s);
                    let bus2 = bus_clone.clone();
                    bus_clone.call(
                        Some(NM_BUS),
                        NM_PATH,
                        NM_IFACE,
                        "GetDevices",
                        None,
                        None,
                        gio::DBusCallFlags::NONE,
                        TIMEOUT_MS,
                        gio::Cancellable::NONE,
                        move |r| {
                            let Some(s) = me2.upgrade() else { return };
                            let paths = r
                                .ok()
                                .and_then(|v| parse_object_paths_reply(&v))
                                .unwrap_or_default();
                            s.probe_wifi_devices(&bus2, paths);
                        },
                    );
                }
            },
        );
    }

    fn probe_wifi_devices(self: &Rc<Self>, bus: &gio::DBusConnection, paths: Vec<String>) {
        let generation = self.wifi_probe_gen.get().wrapping_add(1);
        self.wifi_probe_gen.set(generation);
        if paths.is_empty() {
            self.raw_wifi.borrow_mut().has_wifi_device = false;
            self.sync_wifi_state();
            return;
        }
        let tracker = Rc::new(Cell::new((paths.len(), false)));
        for path in paths {
            let me = Rc::downgrade(self);
            let tracker = Rc::clone(&tracker);
            bus.call(
                Some(NM_BUS),
                &path,
                PROPS_IFACE,
                "Get",
                Some(&(NM_DEVICE_IFACE, "DeviceType").to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                TIMEOUT_MS,
                gio::Cancellable::NONE,
                move |reply| {
                    let Some(s) = me.upgrade() else { return };
                    if s.wifi_probe_gen.get() != generation {
                        return;
                    }
                    let is_wifi = reply
                        .ok()
                        .and_then(|v| parse_property_u32(&v))
                        .is_some_and(|t| t == NM_DEVICE_TYPE_WIFI);
                    let (rem, found) = tracker.get();
                    let rem = rem.saturating_sub(1);
                    let found = found || is_wifi;
                    tracker.set((rem, found));
                    if found || rem == 0 {
                        s.raw_wifi.borrow_mut().has_wifi_device = found;
                        s.sync_wifi_state();
                    }
                },
            );
        }
    }

    fn query_bluetooth(self: &Rc<Self>) {
        let Some(bus) = self.bus.borrow().clone() else {
            return;
        };
        let me = Rc::downgrade(self);
        bus.call(
            Some(BLUEZ_BUS),
            BLUEZ_ROOT_PATH,
            OBJ_MANAGER_IFACE,
            "GetManagedObjects",
            None,
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
            move |reply| {
                let Some(s) = me.upgrade() else { return };
                let bt = reply.ok().map_or_default(|v| parse_bluez_objects(&v));
                s.set_bluetooth(bt);
            },
        );
    }

    /// Enable or disable Wi-Fi via NetworkManager.
    pub fn set_wifi_enabled(self: &Rc<Self>, enabled: bool) {
        let Some(bus) = self.bus.borrow().clone() else {
            return;
        };
        let args = set_property_args(NM_IFACE, "WirelessEnabled", enabled.to_variant());
        let me = Rc::downgrade(self);
        bus.call(
            Some(NM_BUS),
            NM_PATH,
            PROPS_IFACE,
            "Set",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
            move |reply| {
                if reply.is_err()
                    && let Some(s) = me.upgrade()
                {
                    s.query_wifi();
                }
            },
        );
    }

    /// Power the active BlueZ adapter on or off.
    pub fn set_bluetooth_powered(self: &Rc<Self>, powered: bool) {
        let Some(bus) = self.bus.borrow().clone() else {
            return;
        };
        let Some(path) = self.state.borrow().bluetooth.adapter_path.clone() else {
            return;
        };
        let args = set_property_args(BLUEZ_ADAPTER_IFACE, "Powered", powered.to_variant());
        let me = Rc::downgrade(self);
        bus.call(
            Some(BLUEZ_BUS),
            &path,
            PROPS_IFACE,
            "Set",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
            move |reply| {
                if reply.is_err()
                    && let Some(s) = me.upgrade()
                {
                    s.query_bluetooth();
                }
            },
        );
    }

    /// Set display brightness (`0..=100` %) through `systemd-logind`.
    pub fn set_brightness_percent(&self, percent: u32) {
        let Some(bus) = self.bus.borrow().clone() else {
            return;
        };
        let (device, max) = {
            let st = self.state.borrow();
            let Some(dev) = st.brightness.device.clone() else {
                return;
            };
            (dev, st.brightness.max_brightness)
        };
        let raw = brightness_raw(percent, max);
        let args = set_brightness_args(&device, raw);
        self.state.borrow_mut().brightness.percent = percent.min(100);
        bus.call(
            Some(LOGIND_BUS),
            LOGIND_SESSION_PATH,
            LOGIND_SESSION_IFACE,
            "SetBrightness",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            TIMEOUT_MS,
            gio::Cancellable::NONE,
            |_| {},
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_sysfs(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("desk-shell-sysfs-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("class/backlight")).unwrap();
        dir
    }

    fn write_backlight(sysfs: &Path, name: &str, kind: &str, max: &str, current: &str) {
        let dev = sysfs.join("class/backlight").join(name);
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("type"), format!("{kind}\n")).unwrap();
        fs::write(dev.join("max_brightness"), format!("{max}\n")).unwrap();
        fs::write(dev.join("brightness"), format!("{current}\n")).unwrap();
    }

    #[test]
    fn backlight_prefers_firmware_over_platform_over_raw_and_ignores_zero_max() {
        let root = temp_sysfs("priority");
        write_backlight(&root, "intel_backlight", "raw", "1200", "600");
        write_backlight(&root, "acpi_video0", "platform", "100", "50");
        write_backlight(&root, "broken_firmware", "firmware", "0", "0");

        let picked = find_backlight(&root).expect("platform wins when firmware has max 0");
        assert_eq!(picked.name, "acpi_video0");
        assert_eq!(picked.kind, BacklightType::Platform);
        assert_eq!(picked.max_brightness, 100);
        assert_eq!(read_backlight_raw(&picked), Some(50));

        // Valid firmware device beats platform and raw
        write_backlight(&root, "amdgpu_bl1", "firmware", "255", "128");
        write_backlight(&root, "amdgpu_bl0", "firmware", "255", "64");
        let picked = find_backlight(&root).expect("firmware device");
        // Deterministic name tie-break: amdgpu_bl0 < amdgpu_bl1
        assert_eq!(picked.name, "amdgpu_bl0");
        assert_eq!(picked.kind, BacklightType::Firmware);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backlight_returns_none_when_empty_or_missing() {
        let root = temp_sysfs("empty");
        write_backlight(&root, "zero_bl", "raw", "0", "0");
        assert_eq!(find_backlight(&root), None);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(find_backlight(&root), None);
    }

    #[test]
    fn brightness_percent_and_raw_round_trip_and_clamp() {
        assert_eq!(brightness_percent(0, 0), 0);
        assert_eq!(brightness_percent(0, 255), 0);
        assert_eq!(brightness_percent(128, 255), 50);
        assert_eq!(brightness_percent(255, 255), 100);
        assert_eq!(brightness_percent(999, 255), 100);

        assert_eq!(brightness_raw(50, 0), 0);
        assert_eq!(brightness_raw(0, 255), 0);
        // 1% never drops to 0 when max > 0
        assert_eq!(brightness_raw(1, 10), 1);
        assert_eq!(brightness_raw(50, 200), 100);
        assert_eq!(brightness_raw(100, 255), 255);
        assert_eq!(brightness_raw(150, 255), 255);
    }

    #[test]
    fn battery_helpers_handle_charging_low_warning_and_icon_fallback() {
        let discharging_low = battery_state_from_raw(&RawBattery {
            present: true,
            percent: 15.0,
            state: 2, // Discharging
            icon_name: String::new(),
        });
        assert!(discharging_low.present);
        assert!(!discharging_low.charging);
        assert!(discharging_low.low);
        assert_eq!(discharging_low.icon_name, "battery-level-20-symbolic");

        // Charging at 10% is not marked low
        let charging_low = battery_state_from_raw(&RawBattery {
            present: true,
            percent: 10.0,
            state: 1, // Charging
            icon_name: String::new(),
        });
        assert!(charging_low.charging);
        assert!(!charging_low.low);
        assert_eq!(charging_low.icon_name, "battery-level-10-charging-symbolic");

        // Above threshold is not low, and explicit UPower icon name is kept
        let healthy = battery_state_from_raw(&RawBattery {
            present: true,
            percent: 76.4,
            state: 2,
            icon_name: "battery-good-symbolic".into(),
        });
        assert!(!healthy.low);
        assert_eq!(healthy.icon_name, "battery-good-symbolic");
        assert_eq!(battery_percent_rounded(healthy.percent), 76);

        // Absent battery is never low even at 0%
        let absent = battery_state_from_raw(&RawBattery {
            present: false,
            percent: 0.0,
            state: 0,
            icon_name: String::new(),
        });
        assert!(!absent.low);
        assert_eq!(battery_percent_rounded(f64::NAN), 0);
    }

    #[test]
    fn upower_variant_parsing_handles_get_all_and_partial_updates() {
        let dict = glib::VariantDict::new(None);
        dict.insert("IsPresent", true);
        dict.insert("Percentage", 42.5_f64);
        dict.insert("State", 1_u32);
        dict.insert("IconName", "battery-level-40-charging-symbolic");
        let reply = Variant::tuple_from_iter([dict.end()]);

        let mut raw = parse_upower_get_all(&reply).expect("valid UPower GetAll");
        assert_eq!(
            raw,
            RawBattery {
                present: true,
                percent: 42.5,
                state: 1,
                icon_name: "battery-level-40-charging-symbolic".into(),
            }
        );

        let delta = glib::VariantDict::new(None);
        delta.insert("Percentage", 12.0_f64);
        delta.insert("State", 2_u32);
        let sig = Variant::tuple_from_iter([
            UPOWER_DEVICE_IFACE.to_variant(),
            delta.end(),
            Vec::<String>::new().to_variant(),
        ]);
        let (iface, changed) = parse_properties_changed(&sig).expect("PropertiesChanged");
        assert_eq!(iface, UPOWER_DEVICE_IFACE);
        apply_upower_props(&mut raw, &changed);
        let state = battery_state_from_raw(&raw);
        assert!(state.present && !state.charging && state.low);
        assert_eq!(battery_percent_rounded(state.percent), 12);
    }

    #[test]
    fn network_manager_and_property_variants_parse() {
        let dict = glib::VariantDict::new(None);
        dict.insert("WirelessHardwareEnabled", true);
        dict.insert("WirelessEnabled", false);
        let paths = Variant::array_from_iter_with_type(
            glib::VariantTy::OBJECT_PATH,
            [
                glib::variant::ObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/1")
                    .unwrap()
                    .to_variant(),
                glib::variant::ObjectPath::try_from("/org/freedesktop/NetworkManager/Devices/2")
                    .unwrap()
                    .to_variant(),
            ],
        );
        dict.insert_value("AllDevices", &paths);
        let reply = Variant::tuple_from_iter([dict.end()]);

        let (hw, en, devs) = parse_nm_get_all(&reply).expect("valid NM GetAll");
        assert!(hw && !en);
        assert_eq!(
            devs,
            vec![
                "/org/freedesktop/NetworkManager/Devices/1",
                "/org/freedesktop/NetworkManager/Devices/2",
            ]
        );

        let dev_type = Variant::tuple_from_iter([2_u32.to_variant().to_variant()]);
        assert_eq!(parse_property_u32(&dev_type), Some(NM_DEVICE_TYPE_WIFI));
        assert_eq!(parse_property_u32(&("wrong",).to_variant()), None);
    }

    #[test]
    fn bluez_managed_objects_picks_first_adapter_deterministically() {
        let make_adapter = |path: &str, powered: bool| {
            let props = glib::VariantDict::new(None);
            props.insert("Powered", powered);
            let iface_entry =
                Variant::from_dict_entry(&BLUEZ_ADAPTER_IFACE.to_variant(), &props.end());
            let ifaces = Variant::array_from_iter_with_type(
                glib::VariantTy::new("{sa{sv}}").unwrap(),
                [iface_entry],
            );
            Variant::from_dict_entry(
                &glib::variant::ObjectPath::try_from(path)
                    .unwrap()
                    .to_variant(),
                &ifaces,
            )
        };
        let dict = Variant::array_from_iter_with_type(
            glib::VariantTy::new("{oa{sa{sv}}}").unwrap(),
            [
                make_adapter("/org/bluez/hci1", false),
                make_adapter("/org/bluez/hci0", true),
            ],
        );
        let reply = Variant::tuple_from_iter([dict]);
        assert_eq!(
            parse_bluez_objects(&reply),
            BluetoothState {
                available: true,
                powered: true,
                adapter_path: Some("/org/bluez/hci0".into()),
            }
        );

        let empty = Variant::array_from_iter_with_type(
            glib::VariantTy::new("{oa{sa{sv}}}").unwrap(),
            std::iter::empty::<Variant>(),
        );
        assert_eq!(
            parse_bluez_objects(&Variant::tuple_from_iter([empty])),
            BluetoothState::default()
        );
    }

    #[test]
    fn dbus_setter_arguments_have_expected_signatures() {
        let prop = set_property_args(NM_IFACE, "WirelessEnabled", true.to_variant());
        assert_eq!(prop.type_().as_str(), "(ssv)");
        assert_eq!(prop.child_value(0).str(), Some(NM_IFACE));
        assert_eq!(prop.child_value(1).str(), Some("WirelessEnabled"));
        assert_eq!(
            prop.child_value(2)
                .as_variant()
                .and_then(|v| v.get::<bool>()),
            Some(true)
        );

        let br = set_brightness_args("intel_backlight", 400);
        assert_eq!(br.type_().as_str(), "(ssu)");
        assert_eq!(br.child_value(0).str(), Some("backlight"));
        assert_eq!(br.child_value(1).str(), Some("intel_backlight"));
        assert_eq!(br.child_value(2).get::<u32>(), Some(400));
    }
}
