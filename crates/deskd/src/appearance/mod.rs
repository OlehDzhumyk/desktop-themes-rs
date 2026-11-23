// SPDX-License-Identifier: GPL-3.0-or-later
//! Automatic light/dark appearance engine (`io.github.OlehDzhumyk.Desk.Appearance`).
//!
//! Pure schedule evaluation lives in [`solar`] and offline location resolution in [`cities`].
//! Side effects (writing `org.gnome.desktop.interface color-scheme`, reading the local clock and
//! timezone) are injected so unit tests never touch live GSettings or wall-clock time.

pub mod cities;
pub mod service;
pub mod solar;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use solar::{AppearanceMode, LocalNow, ScheduleConfig, Scheme, parse_hhmm};

pub type Shared = Arc<Mutex<AppearanceEngine>>;

pub(crate) fn lock(e: &Shared) -> MutexGuard<'_, AppearanceEngine> {
    e.lock().unwrap_or_else(|p| p.into_inner())
}

const DEFAULT_DARK_MIN: u16 = 20 * 60;
const DEFAULT_LIGHT_MIN: u16 = 7 * 60;

/// User configuration mirrored from `io.github.OlehDzhumyk.Desk.Appearance`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub mode: AppearanceMode,
    pub dark_from: String,
    pub light_from: String,
    pub location: String,
    pub twilight_offset: i32,
    pub override_until_next: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: AppearanceMode::Manual,
            dark_from: "20:00".to_string(),
            light_from: "07:00".to_string(),
            location: String::new(),
            twilight_offset: 0,
            override_until_next: true,
        }
    }
}

impl Config {
    fn schedule_config(&self) -> ScheduleConfig {
        ScheduleConfig {
            mode: self.mode,
            dark_from_min: parse_hhmm(&self.dark_from).unwrap_or(DEFAULT_DARK_MIN),
            light_from_min: parse_hhmm(&self.light_from).unwrap_or(DEFAULT_LIGHT_MIN),
            twilight_offset_min: self.twilight_offset.clamp(-60, 60),
        }
    }
}

/// Snapshot returned by D-Bus `Status()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub mode: String,
    pub scheme: String,
    pub next_transition: String,
    pub location_label: String,
    pub polar: bool,
    pub overridden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManualOverride {
    scheme: Scheme,
    superseded_scheduled: Scheme,
}

/// Stateful automatic light/dark engine.
pub struct AppearanceEngine {
    cfg: Config,
    current_scheme: Scheme,
    manual_override: Option<ManualOverride>,
    last_status: StatusSnapshot,
    apply_scheme: Box<dyn FnMut(Scheme) + Send>,
    clock: Box<dyn Fn() -> LocalNow + Send>,
    system_tz: Box<dyn Fn() -> Option<String> + Send>,
}

impl AppearanceEngine {
    pub fn new(
        cfg: Config,
        initial_scheme: Scheme,
        apply_scheme: impl FnMut(Scheme) + Send + 'static,
        clock: impl Fn() -> LocalNow + Send + 'static,
        system_tz: impl Fn() -> Option<String> + Send + 'static,
    ) -> Self {
        let initial_status = StatusSnapshot {
            mode: cfg.mode.as_str().to_string(),
            scheme: initial_scheme.as_dbus_str().to_string(),
            next_transition: String::new(),
            location_label: String::new(),
            polar: false,
            overridden: false,
        };
        let mut eng = Self {
            cfg,
            current_scheme: initial_scheme,
            manual_override: None,
            last_status: initial_status,
            apply_scheme: Box::new(apply_scheme),
            clock: Box::new(clock),
            system_tz: Box::new(system_tz),
        };
        let _ = eng.refresh(false);
        eng
    }

    /// Evaluate the current schedule, apply `color-scheme` only if the effective scheme changes,
    /// and return `(next_wake_delay, status_changed)`.
    pub fn tick(&mut self) -> (Option<Duration>, bool) {
        self.refresh(true)
    }

    /// Force an immediate recomputation of the schedule.
    pub fn recompute(&mut self) -> (Option<Duration>, bool) {
        self.refresh(true)
    }

    /// Update configuration from GSettings. Changing `mode` clears any active manual override.
    pub fn set_config(&mut self, cfg: Config) {
        if cfg.mode != self.cfg.mode {
            self.manual_override = None;
        }
        self.cfg = cfg;
    }

    /// Handle a manual `SetScheme` request over D-Bus.
    pub fn set_scheme(&mut self, scheme: Scheme) -> bool {
        let (_, ev) = self.evaluate_now();
        if self.cfg.mode.is_auto() {
            if self.cfg.override_until_next {
                if let Some(sched) = ev.desired
                    && sched != scheme
                {
                    self.manual_override = Some(ManualOverride {
                        scheme,
                        superseded_scheduled: sched,
                    });
                } else {
                    self.manual_override = None;
                }
            } else {
                self.manual_override = None;
                self.cfg.mode = AppearanceMode::Manual;
            }
        }
        if scheme != self.current_scheme {
            self.current_scheme = scheme;
            (self.apply_scheme)(scheme);
        }
        self.refresh(false).1
    }

    /// Handle an external change to `org.gnome.desktop.interface color-scheme` (e.g. GNOME Quick
    /// Settings). Ignores echoes of our own writes.
    pub fn on_external_scheme_change(&mut self, scheme: Scheme) -> bool {
        if scheme == self.current_scheme {
            return false;
        }
        self.current_scheme = scheme;
        if self.cfg.mode.is_auto() {
            let (_, ev) = self.evaluate_now();
            if self.cfg.override_until_next {
                if let Some(sched) = ev.desired
                    && sched != scheme
                {
                    self.manual_override = Some(ManualOverride {
                        scheme,
                        superseded_scheduled: sched,
                    });
                } else {
                    self.manual_override = None;
                }
            } else {
                self.manual_override = None;
                self.cfg.mode = AppearanceMode::Manual;
            }
        }
        self.refresh(false).1
    }

    pub fn status(&self) -> StatusSnapshot {
        self.last_status.clone()
    }

    fn evaluate_now(&self) -> (cities::ResolvedLocation, solar::Evaluation) {
        let tz = (self.system_tz)();
        let loc = cities::parse_location(&self.cfg.location, tz.as_deref());
        let now = (self.clock)();
        let sun = solar::compute_sun_times(now.year, now.month, now.day, loc.lat, loc.lon);
        let ev = solar::evaluate(now, self.cfg.schedule_config(), sun);
        (loc, ev)
    }

    fn refresh(&mut self, may_apply: bool) -> (Option<Duration>, bool) {
        let (loc, ev) = self.evaluate_now();

        if !self.cfg.mode.is_auto() {
            self.manual_override = None;
        }

        if may_apply && let Some(scheduled) = ev.desired {
            let effective = match self.manual_override {
                Some(ov) if ov.superseded_scheduled == scheduled => ov.scheme,
                _ => {
                    self.manual_override = None;
                    scheduled
                }
            };
            if effective != self.current_scheme {
                self.current_scheme = effective;
                (self.apply_scheme)(effective);
            }
        }

        let next_status = StatusSnapshot {
            mode: self.cfg.mode.as_str().to_string(),
            scheme: self.current_scheme.as_dbus_str().to_string(),
            next_transition: ev
                .next_transition_min
                .map_or_else(String::new, solar::format_hhmm),
            location_label: loc.label,
            polar: ev.polar,
            overridden: self.manual_override.is_some(),
        };
        let changed = next_status != self.last_status;
        self.last_status = next_status;
        (ev.secs_until_next.map(Duration::from_secs), changed)
    }
}

/// Read the system local date, time, and UTC offset using GLib.
pub fn local_now_system() -> LocalNow {
    match gio::glib::DateTime::now_local() {
        Ok(dt) => LocalNow {
            year: dt.year(),
            month: u32::try_from(dt.month()).unwrap_or(1),
            day: u32::try_from(dt.day_of_month()).unwrap_or(1),
            hour: u32::try_from(dt.hour()).unwrap_or(0),
            minute: u32::try_from(dt.minute()).unwrap_or(0),
            second: u32::try_from(dt.second()).unwrap_or(0),
            tz_offset_min: i32::try_from(dt.utc_offset().as_minutes()).unwrap_or(0),
        },
        Err(_) => LocalNow {
            year: 2026,
            month: 1,
            day: 1,
            hour: 12,
            minute: 0,
            second: 0,
            tz_offset_min: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_now(hour: u32, minute: u32) -> LocalNow {
        LocalNow {
            year: 2026,
            month: 6,
            day: 21,
            hour,
            minute,
            second: 0,
            tz_offset_min: 180,
        }
    }

    #[test]
    fn manual_override_holds_until_next_transition_then_resumes() {
        let clock_state = Arc::new(Mutex::new(mk_now(22, 0)));
        let applied = Arc::new(Mutex::new(Vec::<Scheme>::new()));

        let c_ref = clock_state.clone();
        let a_ref = applied.clone();
        let cfg = Config {
            mode: AppearanceMode::AutoSun,
            location: "50.45,30.52".into(),
            override_until_next: true,
            ..Config::default()
        };
        let mut eng = AppearanceEngine::new(
            cfg,
            Scheme::Light,
            move |s| a_ref.lock().unwrap().push(s),
            move || *c_ref.lock().unwrap(),
            || Some("Europe/Kyiv".into()),
        );

        // 22:00 is after sunset (~21:13): switches to Dark.
        let (delay, _) = eng.tick();
        assert!(delay.is_some());
        assert_eq!(*applied.lock().unwrap(), vec![Scheme::Dark]);
        assert_eq!(eng.status().scheme, "dark");
        assert!(!eng.status().overridden);

        // Second tick at 23:00 does NOT re-write color-scheme.
        *clock_state.lock().unwrap() = mk_now(23, 0);
        eng.tick();
        assert_eq!(applied.lock().unwrap().len(), 1);

        // User toggles Light manually during the dark phase: stays Light while still dark phase.
        eng.set_scheme(Scheme::Light);
        assert_eq!(*applied.lock().unwrap(), vec![Scheme::Dark, Scheme::Light]);
        assert_eq!(eng.status().scheme, "light");
        assert!(eng.status().overridden);

        *clock_state.lock().unwrap() = mk_now(2, 0);
        eng.tick();
        assert_eq!(eng.status().scheme, "light");
        assert!(eng.status().overridden);

        // Sunrise passes (~04:46): at 05:00 scheduled is Light -> override clears, auto resumes.
        *clock_state.lock().unwrap() = mk_now(5, 0);
        eng.tick();
        assert_eq!(eng.status().scheme, "light");
        assert!(!eng.status().overridden);

        // Next sunset (~21:13): at 21:30 auto switches back to Dark!
        *clock_state.lock().unwrap() = mk_now(21, 30);
        eng.tick();
        assert_eq!(eng.status().scheme, "dark");
        assert_eq!(
            *applied.lock().unwrap(),
            vec![Scheme::Dark, Scheme::Light, Scheme::Dark]
        );
    }

    #[test]
    fn external_change_creates_override_without_echo_write() {
        let applied = Arc::new(Mutex::new(Vec::<Scheme>::new()));
        let a_ref = applied.clone();
        let cfg = Config {
            mode: AppearanceMode::AutoTime,
            dark_from: "20:00".into(),
            light_from: "07:00".into(),
            override_until_next: true,
            ..Config::default()
        };
        let mut eng = AppearanceEngine::new(
            cfg,
            Scheme::Light,
            move |s| a_ref.lock().unwrap().push(s),
            || mk_now(21, 0),
            || None,
        );
        eng.tick();
        assert_eq!(*applied.lock().unwrap(), vec![Scheme::Dark]);

        // Echo of our own write is ignored.
        assert!(!eng.on_external_scheme_change(Scheme::Dark));
        assert!(!eng.status().overridden);

        // User changes color-scheme via GNOME Quick Settings to Light.
        assert!(eng.on_external_scheme_change(Scheme::Light));
        assert!(eng.status().overridden);
        assert_eq!(eng.status().scheme, "light");
        assert_eq!(applied.lock().unwrap().len(), 1, "must not re-write");
    }

    #[test]
    fn static_modes_return_no_timer_delay() {
        let mut eng = AppearanceEngine::new(
            Config {
                mode: AppearanceMode::Light,
                ..Config::default()
            },
            Scheme::Dark,
            |_| {},
            || mk_now(12, 0),
            || None,
        );
        let (delay, _) = eng.tick();
        assert_eq!(delay, None);
        assert_eq!(eng.status().scheme, "light");
    }
}
