// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure NOAA solar sunrise/sunset calculation, HH:MM parsing, and schedule evaluation.
//!
//! No network, no external time crates, and no live system calls: callers pass the current local
//! date/time and UTC offset so every path is deterministically unit-testable.

/// Astronomical zenith for official sunrise/sunset (degrees, accounting for refraction).
const SUNRISE_ZENITH_DEG: f64 = 90.833;

/// Minutes in a 24-hour day.
pub const DAY_MINUTES: i32 = 1440;
/// Seconds in a 24-hour day.
const DAY_SECONDS: i64 = 86_400;

/// Effective GNOME colour scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    /// Value written to `org.gnome.desktop.interface color-scheme`.
    pub fn as_gsettings_str(self) -> &'static str {
        match self {
            Scheme::Light => "default",
            Scheme::Dark => "prefer-dark",
        }
    }

    /// Value reported on D-Bus (`Status`).
    pub fn as_dbus_str(self) -> &'static str {
        match self {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
    }

    /// Parse a D-Bus or GSettings colour-scheme string.
    pub fn parse(s: &str) -> Option<Scheme> {
        match s {
            "light" | "default" | "prefer-light" => Some(Scheme::Light),
            "dark" | "prefer-dark" => Some(Scheme::Dark),
            _ => None,
        }
    }
}

/// Appearance scheduling mode (mirrors `io.github.OlehDzhumyk.Desk.Appearance` `mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AppearanceMode {
    #[default]
    Manual,
    Light,
    Dark,
    AutoSun,
    AutoTime,
}

impl AppearanceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AppearanceMode::Manual => "manual",
            AppearanceMode::Light => "light",
            AppearanceMode::Dark => "dark",
            AppearanceMode::AutoSun => "auto-sun",
            AppearanceMode::AutoTime => "auto-time",
        }
    }

    pub fn is_auto(self) -> bool {
        matches!(self, AppearanceMode::AutoSun | AppearanceMode::AutoTime)
    }
}

/// Local calendar date, time of day, and UTC offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalNow {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Local UTC offset in minutes (e.g. `+180` for UTC+3 / EEST).
    pub tz_offset_min: i32,
}

impl LocalNow {
    /// Local seconds since 00:00 (`0..86400`).
    pub fn local_sec(self) -> i64 {
        i64::from(self.hour) * 3600 + i64::from(self.minute) * 60 + i64::from(self.second)
    }
}

/// Result of the NOAA sunrise/sunset calculation for a date and coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunTimes {
    /// Minutes after 00:00 UTC (may lie outside `0..1440` for longitudes far from UTC).
    pub sunrise_min_utc: Option<f64>,
    /// Minutes after 00:00 UTC.
    pub sunset_min_utc: Option<f64>,
    pub polar_day: bool,
    pub polar_night: bool,
}

/// Pure schedule configuration passed to [`evaluate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleConfig {
    pub mode: AppearanceMode,
    /// Minutes since 00:00 for `auto-time` dark start (default 20:00 = 1200).
    pub dark_from_min: u16,
    /// Minutes since 00:00 for `auto-time` light start (default 07:00 = 420).
    pub light_from_min: u16,
    /// Shift sunrise and sunset earlier (negative) or later (positive), in minutes.
    pub twilight_offset_min: i32,
}

/// Pure result of evaluating a schedule at a point in time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evaluation {
    /// Desired scheme (`None` in `Manual` mode).
    pub desired: Option<Scheme>,
    /// Next scheduled transition in local minutes since 00:00 (`0..1440`).
    pub next_transition_min: Option<u16>,
    /// Seconds until the next scheduled wake (`None` = disarm timer, 0 idle wakeups).
    pub secs_until_next: Option<u64>,
    /// True when in `AutoSun` and the sun neither rises nor sets today.
    pub polar: bool,
}

/// Howard Hinnant's civil-to-days algorithm: days since Unix epoch (1970-01-01).
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(month);
    let d = i64::from(day);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Compute sunrise and sunset times in minutes from 00:00 UTC using the standard NOAA
/// solar equations (Jean Meeus *Astronomical Algorithms*).
pub fn compute_sun_times(year: i32, month: u32, day: u32, lat_deg: f64, lon_deg: f64) -> SunTimes {
    let lat = lat_deg.clamp(-89.9, 89.9);
    let lon = lon_deg.clamp(-180.0, 180.0);

    // Approximate local solar noon as Julian Day (1970-01-01 12:00 UTC = JD 2440588.0).
    #[allow(clippy::cast_precision_loss)]
    let days = days_from_civil(year, month, day) as f64;
    let jd = days + 2_440_588.0 - lon / 360.0;
    let t = (jd - 2_451_545.0) / 36_525.0;

    let l0 = (280.46646 + t * (36_000.769_83 + 0.0003032 * t)).rem_euclid(360.0);
    let m = 357.52911 + t * (35_999.050_29 - 0.0001537 * t);
    let m_rad = m.to_radians();
    let ecc = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);

    let eq_center = (1.914602 - t * (0.004817 + 0.000014 * t)) * m_rad.sin()
        + (0.019993 - 0.000101 * t) * (2.0 * m_rad).sin()
        + 0.000289 * (3.0 * m_rad).sin();

    let true_long = l0 + eq_center;
    let omega = (125.04 - 1934.136 * t).to_radians();
    let app_long = true_long - 0.00569 - 0.00478 * omega.sin();

    let mean_obliq =
        23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - 0.001813 * t))) / 60.0) / 60.0;
    let obliq = mean_obliq + 0.00256 * omega.cos();
    let obliq_rad = obliq.to_radians();

    let decl = (obliq_rad.sin() * app_long.to_radians().sin()).asin();

    let y = (obliq_rad / 2.0).tan().powi(2);
    let l0_rad = l0.to_radians();
    let eq_time = 4.0
        * (y * (2.0 * l0_rad).sin() - 2.0 * ecc * m_rad.sin()
            + 4.0 * ecc * y * m_rad.sin() * (2.0 * l0_rad).cos()
            - 0.5 * y * y * (4.0 * l0_rad).sin()
            - 1.25 * ecc * ecc * (2.0 * m_rad).sin())
        .to_degrees();

    let lat_rad = lat.to_radians();
    let zenith_rad = SUNRISE_ZENITH_DEG.to_radians();
    let cos_ha = (zenith_rad.cos() / (lat_rad.cos() * decl.cos())) - lat_rad.tan() * decl.tan();

    if cos_ha < -1.0 {
        return SunTimes {
            sunrise_min_utc: None,
            sunset_min_utc: None,
            polar_day: true,
            polar_night: false,
        };
    }
    if cos_ha > 1.0 {
        return SunTimes {
            sunrise_min_utc: None,
            sunset_min_utc: None,
            polar_day: false,
            polar_night: true,
        };
    }

    let ha_deg = cos_ha.acos().to_degrees();
    let solar_noon_utc = 720.0 - 4.0 * lon - eq_time;
    SunTimes {
        sunrise_min_utc: Some(solar_noon_utc - 4.0 * ha_deg),
        sunset_min_utc: Some(solar_noon_utc + 4.0 * ha_deg),
        polar_day: false,
        polar_night: false,
    }
}

/// Parse `"HH:MM"` (24-hour, `00:00`..=`23:59`) into minutes since midnight (`0..1440`).
pub fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let hh: u16 = h.parse().ok()?;
    let mm: u16 = m.parse().ok()?;
    if hh >= 24 || mm >= 60 {
        return None;
    }
    Some(hh * 60 + mm)
}

/// Format minutes since midnight (`0..1440`) as `"HH:MM"`.
pub fn format_hhmm(min: u16) -> String {
    let m = min % 1440;
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn round_to_day_min(v: f64) -> u16 {
    #[allow(clippy::cast_possible_truncation)]
    let rounded = v.round() as i32;
    let norm = rounded.rem_euclid(DAY_MINUTES);
    u16::try_from(norm).unwrap_or(0)
}

fn evaluate_interval(now_sec: i64, light_min: u16, dark_min: u16) -> (Scheme, u16, u64) {
    let now_min = i32::try_from(now_sec / 60)
        .unwrap_or(0)
        .rem_euclid(DAY_MINUTES);
    let light = i32::from(light_min % 1440);
    let dark = i32::from(dark_min % 1440);

    let (scheme, next_min) = if light < dark {
        if now_min >= light && now_min < dark {
            (Scheme::Light, dark_min % 1440)
        } else {
            (Scheme::Dark, light_min % 1440)
        }
    } else if dark < light {
        if now_min >= dark && now_min < light {
            (Scheme::Dark, light_min % 1440)
        } else {
            (Scheme::Light, dark_min % 1440)
        }
    } else {
        (Scheme::Light, light_min % 1440)
    };

    let target_sec = i64::from(next_min) * 60;
    let diff = (target_sec - now_sec).rem_euclid(DAY_SECONDS);
    let wait_sec = if diff == 0 { DAY_SECONDS } else { diff };
    (scheme, next_min, u64::try_from(wait_sec).unwrap_or(60))
}

/// Evaluate the appearance schedule at `now` given `cfg` and today's `sun` times.
pub fn evaluate(now: LocalNow, cfg: ScheduleConfig, sun: SunTimes) -> Evaluation {
    match cfg.mode {
        AppearanceMode::Manual => Evaluation {
            desired: None,
            next_transition_min: None,
            secs_until_next: None,
            polar: false,
        },
        AppearanceMode::Light => Evaluation {
            desired: Some(Scheme::Light),
            next_transition_min: None,
            secs_until_next: None,
            polar: false,
        },
        AppearanceMode::Dark => Evaluation {
            desired: Some(Scheme::Dark),
            next_transition_min: None,
            secs_until_next: None,
            polar: false,
        },
        AppearanceMode::AutoTime => {
            let (scheme, next_min, wait_sec) =
                evaluate_interval(now.local_sec(), cfg.light_from_min, cfg.dark_from_min);
            Evaluation {
                desired: Some(scheme),
                next_transition_min: Some(next_min),
                secs_until_next: Some(wait_sec),
                polar: false,
            }
        }
        AppearanceMode::AutoSun => {
            if sun.polar_day || sun.polar_night {
                let until_midnight = (DAY_SECONDS - now.local_sec()).max(1) + 60;
                return Evaluation {
                    desired: Some(if sun.polar_day {
                        Scheme::Light
                    } else {
                        Scheme::Dark
                    }),
                    next_transition_min: None,
                    secs_until_next: Some(u64::try_from(until_midnight).unwrap_or(3600)),
                    polar: true,
                };
            }
            let offset = f64::from(now.tz_offset_min + cfg.twilight_offset_min);
            let sunrise_local = round_to_day_min(sun.sunrise_min_utc.unwrap_or(360.0) + offset);
            let sunset_local = round_to_day_min(sun.sunset_min_utc.unwrap_or(1080.0) + offset);
            let (scheme, next_min, wait_sec) =
                evaluate_interval(now.local_sec(), sunrise_local, sunset_local);
            Evaluation {
                desired: Some(scheme),
                next_transition_min: Some(next_min),
                secs_until_next: Some(wait_sec),
                polar: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_at(year: i32, month: u32, day: u32, hour: u32, minute: u32, tz: i32) -> LocalNow {
        LocalNow {
            year,
            month,
            day,
            hour,
            minute,
            second: 0,
            tz_offset_min: tz,
        }
    }

    #[test]
    fn kyiv_solstices_match_noaa_within_two_minutes() {
        // Kyiv (50.45 N, 30.52 E) on summer solstice 2026-06-21, UTC+3 (EEST = +180 min):
        // sunrise ~04:46, sunset ~21:13 local.
        let summer = compute_sun_times(2026, 6, 21, 50.45, 30.52);
        assert!(!summer.polar_day && !summer.polar_night);
        let sr = (summer.sunrise_min_utc.unwrap() + 180.0).round();
        let ss = (summer.sunset_min_utc.unwrap() + 180.0).round();
        assert!(
            (sr - f64::from(4 * 60 + 46)).abs() <= 2.0,
            "summer sunrise {sr}"
        );
        assert!(
            (ss - f64::from(21 * 60 + 13)).abs() <= 2.0,
            "summer sunset {ss}"
        );

        // Kyiv on winter solstice 2026-12-21, UTC+2 (EET = +120 min):
        // sunrise ~07:56, sunset ~15:56 local.
        let winter = compute_sun_times(2026, 12, 21, 50.45, 30.52);
        assert!(!winter.polar_day && !winter.polar_night);
        let sr = (winter.sunrise_min_utc.unwrap() + 120.0).round();
        let ss = (winter.sunset_min_utc.unwrap() + 120.0).round();
        assert!(
            (sr - f64::from(7 * 60 + 56)).abs() <= 2.0,
            "winter sunrise {sr}"
        );
        assert!(
            (ss - f64::from(15 * 60 + 56)).abs() <= 2.0,
            "winter sunset {ss}"
        );
    }

    #[test]
    fn tromso_polar_day_and_night() {
        let summer = compute_sun_times(2026, 6, 21, 69.65, 18.96);
        assert!(summer.polar_day);
        assert!(!summer.polar_night);
        assert!(summer.sunrise_min_utc.is_none());

        let ev_day = evaluate(
            local_at(2026, 6, 21, 23, 0, 120),
            ScheduleConfig {
                mode: AppearanceMode::AutoSun,
                dark_from_min: 1200,
                light_from_min: 420,
                twilight_offset_min: 0,
            },
            summer,
        );
        assert_eq!(ev_day.desired, Some(Scheme::Light));
        assert!(ev_day.polar);
        assert!(ev_day.next_transition_min.is_none());
        assert!(ev_day.secs_until_next.is_some());

        let winter = compute_sun_times(2026, 12, 21, 69.65, 18.96);
        assert!(winter.polar_night);
        assert!(!winter.polar_day);

        let ev_night = evaluate(
            local_at(2026, 12, 21, 12, 0, 60),
            ScheduleConfig {
                mode: AppearanceMode::AutoSun,
                dark_from_min: 1200,
                light_from_min: 420,
                twilight_offset_min: 0,
            },
            winter,
        );
        assert_eq!(ev_night.desired, Some(Scheme::Dark));
        assert!(ev_night.polar);
    }

    #[test]
    fn auto_time_handles_midnight_wrap_and_inverted_windows() {
        let dummy_sun = SunTimes {
            sunrise_min_utc: None,
            sunset_min_utc: None,
            polar_day: false,
            polar_night: false,
        };
        let cfg = ScheduleConfig {
            mode: AppearanceMode::AutoTime,
            dark_from_min: parse_hhmm("20:00").unwrap(),
            light_from_min: parse_hhmm("07:00").unwrap(),
            twilight_offset_min: 0,
        };
        // 23:30 is dark; next transition is 07:00 (7h30m = 27000 s away)
        let ev = evaluate(local_at(2026, 6, 21, 23, 30, 180), cfg, dummy_sun);
        assert_eq!(ev.desired, Some(Scheme::Dark));
        assert_eq!(ev.next_transition_min, Some(420));
        assert_eq!(ev.secs_until_next, Some(27_000));

        // 06:59 is still dark; 07:00 flips to light
        let ev_before = evaluate(local_at(2026, 6, 21, 6, 59, 180), cfg, dummy_sun);
        assert_eq!(ev_before.desired, Some(Scheme::Dark));
        assert_eq!(ev_before.secs_until_next, Some(60));

        let ev_at = evaluate(local_at(2026, 6, 21, 7, 0, 180), cfg, dummy_sun);
        assert_eq!(ev_at.desired, Some(Scheme::Light));
        assert_eq!(ev_at.next_transition_min, Some(1200));

        // Inverted window: dark 01:00..07:00, light 07:00..01:00
        let inv = ScheduleConfig {
            mode: AppearanceMode::AutoTime,
            dark_from_min: parse_hhmm("01:00").unwrap(),
            light_from_min: parse_hhmm("07:00").unwrap(),
            twilight_offset_min: 0,
        };
        assert_eq!(
            evaluate(local_at(2026, 6, 21, 23, 0, 0), inv, dummy_sun).desired,
            Some(Scheme::Light)
        );
        assert_eq!(
            evaluate(local_at(2026, 6, 21, 3, 0, 0), inv, dummy_sun).desired,
            Some(Scheme::Dark)
        );
    }

    #[test]
    fn twilight_offset_shifts_transitions() {
        let sun = compute_sun_times(2026, 6, 21, 50.45, 30.52);
        let base = evaluate(
            local_at(2026, 6, 21, 12, 0, 180),
            ScheduleConfig {
                mode: AppearanceMode::AutoSun,
                dark_from_min: 1200,
                light_from_min: 420,
                twilight_offset_min: 0,
            },
            sun,
        );
        let shifted = evaluate(
            local_at(2026, 6, 21, 12, 0, 180),
            ScheduleConfig {
                mode: AppearanceMode::AutoSun,
                dark_from_min: 1200,
                light_from_min: 420,
                twilight_offset_min: -30,
            },
            sun,
        );
        assert_eq!(
            shifted.next_transition_min.unwrap(),
            base.next_transition_min.unwrap() - 30
        );
    }

    #[test]
    fn static_modes_disarm_timer() {
        let sun = compute_sun_times(2026, 6, 21, 50.45, 30.52);
        for (mode, expected) in [
            (AppearanceMode::Manual, None),
            (AppearanceMode::Light, Some(Scheme::Light)),
            (AppearanceMode::Dark, Some(Scheme::Dark)),
        ] {
            let ev = evaluate(
                local_at(2026, 6, 21, 12, 0, 180),
                ScheduleConfig {
                    mode,
                    dark_from_min: 1200,
                    light_from_min: 420,
                    twilight_offset_min: 0,
                },
                sun,
            );
            assert_eq!(ev.desired, expected);
            assert_eq!(ev.secs_until_next, None);
        }
    }

    #[test]
    fn hhmm_parser_and_formatter() {
        assert_eq!(parse_hhmm("00:00"), Some(0));
        assert_eq!(parse_hhmm("07:05"), Some(425));
        assert_eq!(parse_hhmm("23:59"), Some(1439));
        assert_eq!(parse_hhmm("7:00"), Some(420));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("bad"), None);
        assert_eq!(format_hhmm(425), "07:05");
        assert_eq!(format_hhmm(1273), "21:13");
    }
}
