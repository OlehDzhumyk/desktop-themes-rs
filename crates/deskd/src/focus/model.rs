// SPDX-License-Identifier: GPL-3.0-or-later
//! Focus modes, their schedules and the state file (`~/.local/share/deskd/focus.json`, 0600).
//!
//! Pure: no clock, no GSettings. Time comes in as [`Now`] (local wall clock plus Unix time), so
//! schedule evaluation is tested without waiting.

use desk_settings::n_;
use serde::{Deserialize, Serialize};

pub const STORE_VERSION: u32 = 1;
const DAY: i64 = 86_400;
pub const DAY_NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// Built-in mode ids: never deleted, re-added when missing.
pub const DND: &str = "dnd";
pub const WORK: &str = "work";
pub const SLEEP: &str = "sleep";

/// The local time and the matching Unix time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub unix: i64,
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u8,
    /// Seconds since local midnight.
    pub secs_of_day: u32,
}

impl Now {
    fn midnight(self) -> i64 {
        self.unix - i64::from(self.secs_of_day)
    }
}

/// "HH:MM" (24-hour) → minutes after midnight.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Weekdays plus a start and an end time; an end at or before the start wraps past midnight.
/// The window belongs to the day it starts on (Mon 22:00–07:00 ends on Tuesday morning).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// "mon" … "sun".
    pub days: Vec<String>,
    pub start: String,
    pub end: String,
}

/// One occurrence of a schedule: `[start, end)` in Unix seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: i64,
    pub end: i64,
}

impl Schedule {
    /// Checked form: unknown day names or bad times make the schedule invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.days.is_empty() {
            return Err("a schedule needs at least one day".into());
        }
        for d in &self.days {
            if !DAY_NAMES.contains(&d.as_str()) {
                return Err(format!("unknown day '{d}' (expected mon … sun)"));
            }
        }
        for t in [&self.start, &self.end] {
            if parse_hhmm(t).is_none() {
                return Err(format!("invalid time '{t}' (expected HH:MM)"));
            }
        }
        Ok(())
    }

    fn has_day(&self, weekday: i64) -> bool {
        let w = usize::try_from(weekday.rem_euclid(7)).unwrap_or(0);
        self.days.iter().any(|d| d == DAY_NAMES[w])
    }

    fn times(&self) -> Option<(i64, i64)> {
        let s = i64::from(parse_hhmm(&self.start)?) * 60;
        let e = i64::from(parse_hhmm(&self.end)?) * 60;
        let len = if e > s { e - s } else { e + DAY - s };
        Some((s, len))
    }

    fn window_on(&self, now: Now, day_offset: i64) -> Option<Window> {
        let (s, len) = self.times()?;
        if !self.has_day(i64::from(now.weekday) + day_offset) {
            return None;
        }
        let start = now.midnight() + day_offset * DAY + s;
        Some(Window {
            start,
            end: start + len,
        })
    }

    /// The occurrence that contains `now`, if any.
    pub fn window_at(&self, now: Now) -> Option<Window> {
        [0, -1]
            .into_iter()
            .filter_map(|d| self.window_on(now, d))
            .find(|w| w.start <= now.unix && now.unix < w.end)
    }

    /// The first occurrence that starts after `now`.
    pub fn next_start(&self, now: Now) -> Option<i64> {
        (0..=7)
            .filter_map(|d| self.window_on(now, d))
            .map(|w| w.start)
            .find(|&s| s > now.unix)
    }
}

/// A focus mode. Built-in modes keep their English name as a gettext id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mode {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub block_banners: bool,
    pub mute_sounds: bool,
    /// App ids (desktop file names without `.desktop`) whose banners still show.
    pub allowed_apps: Vec<String>,
    /// Stored for the shell; not enforced yet (GNOME shows no app badges by default).
    pub hide_badges: bool,
    pub schedule: Option<Schedule>,
}

impl Default for Mode {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            icon: "weather-clear-night-symbolic".into(),
            block_banners: true,
            mute_sounds: false,
            allowed_apps: vec![],
            hide_badges: false,
            schedule: None,
        }
    }
}

impl Mode {
    pub fn builtin(&self) -> bool {
        is_builtin(&self.id)
    }

    pub fn validate(&self) -> Result<(), String> {
        let ok_id = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !ok_id {
            return Err(format!(
                "invalid mode id '{}' (letters, digits, '-' and '_' only)",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            return Err("a mode needs a name".into());
        }
        if let Some(s) = &self.schedule {
            s.validate()?;
        }
        Ok(())
    }
}

pub fn is_builtin(id: &str) -> bool {
    [DND, WORK, SLEEP].contains(&id)
}

pub fn builtin_modes() -> Vec<Mode> {
    vec![
        Mode {
            id: DND.into(),
            name: n_("Do not disturb").into(),
            icon: "notifications-disabled-symbolic".into(),
            ..Mode::default()
        },
        Mode {
            id: WORK.into(),
            name: n_("Work").into(),
            icon: "briefcase-symbolic".into(),
            ..Mode::default()
        },
        Mode {
            id: SLEEP.into(),
            name: n_("Sleep").into(),
            icon: "weather-clear-night-symbolic".into(),
            mute_sounds: true,
            ..Mode::default()
        },
    ]
}

/// Why a mode is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Manual,
    Schedule,
    Session,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Manual => "manual",
            Source::Schedule => "schedule",
            Source::Session => "session",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Active {
    pub mode: String,
    pub source: Source,
    /// For a scheduled mode: the start of its window (identifies the occurrence).
    pub started: i64,
    /// End (Unix seconds); `None`: until turned off.
    pub until: Option<i64>,
}

/// Values deskd overrode, to restore when the mode ends (or after a crash).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub show_banners: Option<bool>,
    pub event_sounds: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    pub modes: Vec<Mode>,
    pub active: Option<Active>,
    pub saved: Saved,
    /// Schedule windows already started once `(mode, window start)`: a window never starts its
    /// mode twice (clock set backwards, the user ended it early).
    pub handled: Vec<(String, i64)>,
    /// End of the break after a focus session.
    pub break_until: Option<i64>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            modes: builtin_modes(),
            active: None,
            saved: Saved::default(),
            handled: vec![],
            break_until: None,
        }
    }
}

impl Store {
    /// Parse the state file; built-in modes that are missing are added back, invalid modes dropped.
    pub fn parse(json: &[u8]) -> Result<Store, String> {
        let mut st: Store = serde_json::from_slice(json).map_err(|e| e.to_string())?;
        st.modes.retain(|m| m.validate().is_ok());
        let mut seen = std::collections::HashSet::new();
        st.modes.retain(|m| seen.insert(m.id.clone()));
        for b in builtin_modes().into_iter().rev() {
            if !st.modes.iter().any(|m| m.id == b.id) {
                st.modes.insert(0, b);
            }
        }
        st.version = STORE_VERSION;
        Ok(st)
    }

    pub fn mode(&self, id: &str) -> Option<&Mode> {
        self.modes.iter().find(|m| m.id == id)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Monday 2026-11-23 00:00 UTC = 1_795_392_000.
    pub const MON0: i64 = 1_795_392_000;

    pub fn at(day: u8, h: u32, m: u32) -> Now {
        let secs = h * 3600 + m * 60;
        Now {
            unix: MON0 + i64::from(day) * DAY + i64::from(secs),
            weekday: day,
            secs_of_day: secs,
        }
    }

    fn night(days: &[&str]) -> Schedule {
        Schedule {
            days: days.iter().map(|d| (*d).to_string()).collect(),
            start: "22:00".into(),
            end: "07:00".into(),
        }
    }

    #[test]
    fn wrapping_window_belongs_to_its_start_day() {
        let s = night(&["mon"]);
        // Monday 23:00 and Tuesday 06:59 are inside; Tuesday 07:00 and Monday 21:59 are not.
        let w = s.window_at(at(0, 23, 0)).unwrap();
        assert_eq!(w, s.window_at(at(1, 6, 59)).unwrap());
        assert_eq!(w.start, at(0, 22, 0).unix);
        assert_eq!(w.end, at(1, 7, 0).unix);
        assert!(s.window_at(at(1, 7, 0)).is_none());
        assert!(s.window_at(at(0, 21, 59)).is_none());
        // Tuesday 22:00 is not a Monday window.
        assert!(s.window_at(at(1, 23, 0)).is_none());
    }

    #[test]
    fn next_start_skips_disabled_days() {
        let s = night(&["fri"]);
        assert_eq!(s.next_start(at(0, 12, 0)), Some(at(4, 22, 0).unix));
        // exactly at the start: the next one is a week later
        assert_eq!(
            s.next_start(at(4, 22, 0)),
            Some(at(4, 22, 0).unix + 7 * DAY)
        );
    }

    #[test]
    fn same_day_window_and_validation() {
        let s = Schedule {
            days: vec!["sat".into()],
            start: "09:00".into(),
            end: "17:30".into(),
        };
        let w = s.window_at(at(5, 12, 0)).unwrap();
        assert_eq!(w.end - w.start, 8 * 3600 + 1800);
        assert!(s.validate().is_ok());
        assert!(
            Schedule {
                start: "25:00".into(),
                ..s.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            Schedule {
                days: vec!["xyz".into()],
                ..s.clone()
            }
            .validate()
            .is_err()
        );
        assert!(Schedule { days: vec![], ..s }.validate().is_err());
    }

    #[test]
    fn corrupt_or_partial_store() {
        assert!(Store::parse(b"{not json").is_err());
        // a store without modes gets the built-ins back; a bad custom mode is dropped
        let st =
            Store::parse(br#"{"modes":[{"id":"bad id","name":"x"},{"id":"gym","name":"Gym"}]}"#)
                .unwrap();
        let ids: Vec<&str> = st.modes.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, [DND, WORK, SLEEP, "gym"]);
        assert!(st.mode(SLEEP).unwrap().mute_sounds);
    }
}
