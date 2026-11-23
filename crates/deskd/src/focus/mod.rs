// SPDX-License-Identifier: GPL-3.0-or-later
//! Focus modes engine (`io.github.OlehDzhumyk.Desk.Focus`, docs/spec/features/focus.md).
//!
//! deskd owns the state machine and the schedules; it writes
//! `org.gnome.desktop.notifications show-banners` and, for modes that mute sounds,
//! `org.gnome.desktop.sound event-sounds`. The value it overrode is kept in the state file first,
//! so ending a mode, a restart or a crash restores exactly that value, and only while the key
//! still holds what deskd wrote (the user wins). The bridge draws the tile and the status icon and
//! lets allowed apps through.
//!
//! Side effects and the clock are injected ([`Effects`], [`Now`]): tests never touch live
//! GSettings or wall-clock time.

pub mod model;
pub mod service;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use desk_settings::i18n::gettext;
use model::{Active, Mode, Now, Source, Store};

pub type Shared = Arc<Mutex<FocusEngine>>;

pub(crate) fn lock(e: &Shared) -> MutexGuard<'_, FocusEngine> {
    e.lock().unwrap_or_else(|p| p.into_inner())
}

/// User configuration mirrored from GSettings `io.github.OlehDzhumyk.Desk.Focus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    /// Minutes.
    pub session_length: u32,
    /// Minutes, 0 = none.
    pub session_break: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            session_length: 25,
            session_break: 5,
        }
    }
}

/// What the engine changes outside itself.
pub trait Effects: Send {
    fn set_show_banners(&mut self, on: bool);
    fn set_event_sounds(&mut self, on: bool);
    /// Mirror of the active mode id in GSettings (`active-mode`).
    fn set_active_mode(&mut self, id: &str);
    /// Write the state file. Called before the GSettings writes that it makes undoable.
    fn persist(&mut self, store: &Store);
}

/// Snapshot returned by D-Bus `Status()`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub mode: String,
    pub name: String,
    pub icon: String,
    /// "manual", "schedule", "session" or "" (none).
    pub source: String,
    /// End of the active mode (Unix seconds), 0 = until turned off or none.
    pub until: i64,
    /// End of the break after a focus session, 0 = none.
    pub break_until: i64,
    /// Name of the mode whose schedule starts next, and when (0 = none).
    pub next_mode: String,
    pub next_start: i64,
    pub allowed_apps: Vec<String>,
}

/// The display name of a mode: built-in names are gettext ids.
pub fn display_name(m: &Mode) -> String {
    if m.builtin() {
        gettext(&m.name)
    } else {
        m.name.clone()
    }
}

pub struct FocusEngine {
    cfg: Config,
    store: Store,
    /// Last known live values (from start-up and change notifications).
    banners: bool,
    sounds: bool,
    active_key: String,
    session_ended: bool,
    last_status: Status,
    fx: Box<dyn Effects>,
    clock: Box<dyn Fn() -> Now + Send>,
}

impl FocusEngine {
    /// Build the engine and reconcile the live session with the stored state (AC-FOCUS-05).
    ///
    /// `store`: `None` when the state file is missing or corrupt. `active_key`: the `active-mode`
    /// value found at start-up; when the file is lost while it is set, deskd had blocked banners,
    /// so they are restored (never left blocked without an active mode).
    pub fn new(
        cfg: Config,
        store: Option<Store>,
        show_banners: bool,
        event_sounds: bool,
        active_key: String,
        fx: impl Effects + 'static,
        clock: impl Fn() -> Now + Send + 'static,
    ) -> Self {
        let lost = store.is_none();
        let mut eng = Self {
            cfg,
            store: store.unwrap_or_default(),
            banners: show_banners,
            sounds: event_sounds,
            active_key,
            session_ended: false,
            last_status: Status::default(),
            fx: Box::new(fx),
            clock: Box::new(clock),
        };
        if lost && !eng.active_key.is_empty() && !eng.banners {
            eng.banners = true;
            eng.fx.set_show_banners(true);
        }
        // the user changed the key while deskd was not running: the user wins
        if eng.store.saved.show_banners.is_some() && eng.banners && eng.store.active.is_some() {
            eng.store.saved.show_banners = None;
            eng.store.active = None;
        }
        if eng.store.saved.event_sounds.is_some() && eng.sounds {
            eng.store.saved.event_sounds = None;
        }
        eng.refresh();
        eng.sync_outside(true);
        eng
    }

    /// Evaluate schedules and expiry; returns `(next_wake_delay, status_changed)`.
    pub fn tick(&mut self) -> (Option<Duration>, bool) {
        let delay = self.refresh();
        (delay, self.sync_outside(false))
    }

    pub fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    /// Start a mode by hand. `until`: Unix seconds, `None` = until turned off.
    pub fn start(&mut self, id: &str, until: Option<i64>) -> Result<bool, String> {
        if !self.cfg.enabled {
            return Err("focus modes are turned off".into());
        }
        if self.store.mode(id).is_none() {
            return Err(format!("no focus mode '{id}'"));
        }
        let now = (self.clock)();
        if until.is_some_and(|u| u <= now.unix) {
            return Err("the end time is in the past".into());
        }
        self.store.break_until = None;
        self.activate(id, Source::Manual, now.unix, until);
        self.refresh();
        Ok(self.sync_outside(false))
    }

    /// Start a focus session (do not disturb for `minutes`, 0 = the configured length).
    pub fn start_session(&mut self, minutes: u32) -> Result<bool, String> {
        if !self.cfg.enabled {
            return Err("focus modes are turned off".into());
        }
        let minutes = if minutes == 0 {
            self.cfg.session_length
        } else {
            minutes
        }
        .clamp(1, 24 * 60);
        let now = (self.clock)();
        self.store.break_until = None;
        self.activate(
            model::DND,
            Source::Session,
            now.unix,
            Some(now.unix + i64::from(minutes) * 60),
        );
        self.refresh();
        Ok(self.sync_outside(false))
    }

    /// End the active mode (and a session break). A scheduled window stays handled: it does not
    /// start again until its next occurrence.
    pub fn stop(&mut self) -> bool {
        self.store.break_until = None;
        self.deactivate();
        self.refresh();
        self.sync_outside(false)
    }

    /// Add or replace a mode.
    pub fn save_mode(&mut self, m: Mode) -> Result<bool, String> {
        m.validate()?;
        match self.store.modes.iter_mut().find(|x| x.id == m.id) {
            Some(x) => *x = m,
            None => self.store.modes.push(m),
        }
        self.refresh();
        Ok(self.sync_outside(true))
    }

    pub fn delete_mode(&mut self, id: &str) -> Result<bool, String> {
        if model::is_builtin(id) {
            return Err(format!("the built-in mode '{id}' cannot be deleted"));
        }
        if self.store.mode(id).is_none() {
            return Err(format!("no focus mode '{id}'"));
        }
        if self.store.active.as_ref().is_some_and(|a| a.mode == id) {
            self.deactivate();
        }
        self.store.modes.retain(|m| m.id != id);
        self.refresh();
        Ok(self.sync_outside(true))
    }

    /// `show-banners` changed outside (GNOME's own switch, or the echo of our write).
    pub fn on_external_show_banners(&mut self, on: bool) -> bool {
        if on == self.banners {
            return false;
        }
        self.banners = on;
        if on && self.store.saved.show_banners.is_some() {
            // the user turned do-not-disturb off during a mode: the mode ends (AC-FOCUS-08)
            self.store.saved.show_banners = None;
            self.deactivate();
            self.refresh();
            return self.sync_outside(true);
        }
        false
    }

    /// `event-sounds` changed outside: the user's value wins and is no longer restored.
    pub fn on_external_event_sounds(&mut self, on: bool) -> bool {
        if on == self.sounds {
            return false;
        }
        self.sounds = on;
        if on && self.store.saved.event_sounds.take().is_some() {
            return self.sync_outside(true);
        }
        false
    }

    /// True once after a focus session ran out (the bridge shows a quiet notification).
    pub fn take_session_ended(&mut self) -> bool {
        std::mem::take(&mut self.session_ended)
    }

    pub fn status(&self) -> Status {
        self.last_status.clone()
    }

    /// The modes as JSON for clients: the stored fields plus `label` (translated) and `builtin`.
    pub fn modes_json(&self) -> String {
        let v: Vec<serde_json::Value> = self
            .store
            .modes
            .iter()
            .map(|m| {
                let mut o = serde_json::to_value(m).unwrap_or_default();
                if let Some(map) = o.as_object_mut() {
                    map.insert("label".into(), display_name(m).into());
                    map.insert("builtin".into(), m.builtin().into());
                }
                o
            })
            .collect();
        serde_json::to_string(&v).unwrap_or_else(|_| "[]".into())
    }

    fn activate(&mut self, id: &str, source: Source, started: i64, until: Option<i64>) {
        self.store.active = Some(Active {
            mode: id.to_string(),
            source,
            started,
            until,
        });
    }

    fn deactivate(&mut self) {
        if let Some(a) = self.store.active.take()
            && a.source == Source::Schedule
            && !self
                .store
                .handled
                .iter()
                .any(|(m, s)| *m == a.mode && *s == a.started)
        {
            self.store.handled.push((a.mode, a.started));
        }
    }

    /// The state machine step; returns the delay to the next transition.
    fn refresh(&mut self) -> Option<Duration> {
        let now = (self.clock)();
        if !self.cfg.enabled {
            self.store.break_until = None;
            self.deactivate();
            return None;
        }
        if self.store.break_until.is_some_and(|b| b <= now.unix) {
            self.store.break_until = None;
        }
        if let Some(a) = self.store.active.clone() {
            let ended = match (a.source, self.store.mode(&a.mode)) {
                (_, None) => true,
                (Source::Schedule, Some(m)) => m
                    .schedule
                    .as_ref()
                    .and_then(|s| s.window_at(now))
                    .is_none_or(|w| w.start != a.started),
                (_, Some(_)) => a.until.is_some_and(|u| u <= now.unix),
            };
            if ended {
                if a.source == Source::Session && a.until.is_some_and(|u| u <= now.unix) {
                    self.session_ended = true;
                    let end = a.until.unwrap_or(now.unix) + i64::from(self.cfg.session_break) * 60;
                    self.store.break_until = (end > now.unix).then_some(end);
                }
                self.deactivate();
            }
        }
        // schedules: the window that started last wins; each window starts its mode once
        let manual = self
            .store
            .active
            .as_ref()
            .is_some_and(|a| a.source != Source::Schedule);
        if !manual {
            let started = self
                .store
                .modes
                .iter()
                .filter_map(|m| Some((m.id.clone(), m.schedule.as_ref()?.window_at(now)?)))
                .filter(|(id, w)| {
                    !self
                        .store
                        .handled
                        .iter()
                        .any(|(m, s)| m == id && *s == w.start)
                })
                .max_by_key(|(_, w)| w.start);
            if let Some((id, w)) = started {
                self.deactivate();
                self.store.handled.push((id.clone(), w.start));
                self.activate(&id, Source::Schedule, w.start, Some(w.end));
            }
        }
        // forget handled windows that ended more than a week ago
        let horizon = now.unix - 8 * 86_400;
        self.store.handled.retain(|(_, s)| *s >= horizon);
        self.next_transition(now)
            .map(|t| Duration::from_secs(u64::try_from((t - now.unix).max(1)).unwrap_or(1)))
    }

    fn next_schedule(&self, now: Now) -> Option<(&Mode, i64)> {
        self.store
            .modes
            .iter()
            .filter_map(|m| Some((m, m.schedule.as_ref()?.next_start(now)?)))
            .min_by_key(|(_, t)| *t)
    }

    fn next_transition(&self, now: Now) -> Option<i64> {
        let active_end = self.store.active.as_ref().and_then(|a| a.until);
        [
            active_end,
            self.store.break_until,
            self.next_schedule(now).map(|(_, t)| t),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Bring the live keys, the GSettings mirror, the state file and the status in line with the
    /// state. Returns whether the status changed.
    fn sync_outside(&mut self, force_persist: bool) -> bool {
        let mode = self
            .store
            .active
            .as_ref()
            .and_then(|a| self.store.mode(&a.mode))
            .cloned();
        let block = mode.as_ref().is_some_and(|m| m.block_banners);
        let mute = mode.as_ref().is_some_and(|m| m.mute_sounds);
        let mut writes: Vec<(bool, bool)> = vec![]; // (is_banners, value)
        let before = self.store.saved.clone();
        if block {
            self.store.saved.show_banners.get_or_insert(self.banners);
            if self.banners {
                writes.push((true, false));
            }
        } else if let Some(prev) = self.store.saved.show_banners.take()
            && prev
            && !self.banners
        {
            writes.push((true, true));
        }
        if mute {
            self.store.saved.event_sounds.get_or_insert(self.sounds);
            if self.sounds {
                writes.push((false, false));
            }
        } else if let Some(prev) = self.store.saved.event_sounds.take()
            && prev
            && !self.sounds
        {
            writes.push((false, true));
        }
        let status = self.compute_status();
        let changed = status != self.last_status;
        // the state file first: a crash after it still knows what to restore
        if changed || force_persist || before != self.store.saved || !writes.is_empty() {
            self.fx.persist(&self.store);
        }
        for (is_banners, v) in writes {
            if is_banners {
                self.banners = v;
                self.fx.set_show_banners(v);
            } else {
                self.sounds = v;
                self.fx.set_event_sounds(v);
            }
        }
        if status.mode != self.active_key {
            self.active_key.clone_from(&status.mode);
            self.fx.set_active_mode(&status.mode);
        }
        self.last_status = status;
        changed
    }

    fn compute_status(&self) -> Status {
        let now = (self.clock)();
        let mut st = Status {
            break_until: self.store.break_until.unwrap_or(0),
            ..Status::default()
        };
        if let Some((m, t)) = self.next_schedule(now) {
            st.next_mode = display_name(m);
            st.next_start = t;
        }
        if let Some(a) = &self.store.active
            && let Some(m) = self.store.mode(&a.mode)
        {
            st.mode.clone_from(&m.id);
            st.name = display_name(m);
            st.icon.clone_from(&m.icon);
            st.source = a.source.as_str().to_string();
            st.until = a.until.unwrap_or(0);
            st.allowed_apps.clone_from(&m.allowed_apps);
        }
        st
    }
}

/// The state file: `$XDG_DATA_HOME/deskd/focus.json`.
pub fn default_store_path() -> PathBuf {
    let (clip, _) = crate::clipboard::Engine::default_dirs();
    clip.parent()
        .map_or_else(|| clip.clone(), Path::to_path_buf)
        .join("focus.json")
}

/// Read the state file: `None` when it is missing or corrupt (a corrupt file is kept aside as
/// `focus.json.bad` for inspection).
pub fn load_store(path: &Path) -> Option<Store> {
    let bytes = std::fs::read(path).ok()?;
    match Store::parse(&bytes) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("deskd: focus state file is corrupt ({e}); modes reset to the built-in ones");
            let _ = std::fs::rename(path, path.with_extension("json.bad"));
            None
        }
    }
}

/// Write the state file (0600, in a 0700 directory, atomically).
pub fn save_store(path: &Path, store: &Store) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        crate::clipboard::ensure_private_dir(dir)?;
    }
    let json = serde_json::to_vec_pretty(store).map_err(std::io::Error::other)?;
    crate::clipboard::write_private(path, &json)
}

/// The local clock via GLib.
pub fn now_system() -> Now {
    match gio::glib::DateTime::now_local() {
        Ok(dt) => Now {
            unix: dt.to_unix(),
            weekday: u8::try_from(dt.day_of_week() - 1).unwrap_or(0),
            secs_of_day: u32::try_from(dt.hour() * 3600 + dt.minute() * 60 + dt.second())
                .unwrap_or(0),
        },
        Err(_) => Now {
            unix: 0,
            weekday: 3,
            secs_of_day: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::model::tests::at;
    use super::model::{Schedule, WORK};
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Ev {
        Banners(bool),
        Sounds(bool),
        Key(String),
    }

    #[derive(Clone, Default)]
    struct Rec {
        ev: Arc<Mutex<Vec<Ev>>>,
        saved: Arc<Mutex<Option<Store>>>,
    }

    impl Effects for Rec {
        fn set_show_banners(&mut self, on: bool) {
            self.ev.lock().unwrap().push(Ev::Banners(on));
        }
        fn set_event_sounds(&mut self, on: bool) {
            self.ev.lock().unwrap().push(Ev::Sounds(on));
        }
        fn set_active_mode(&mut self, id: &str) {
            self.ev.lock().unwrap().push(Ev::Key(id.into()));
        }
        fn persist(&mut self, store: &Store) {
            *self.saved.lock().unwrap() = Some(store.clone());
        }
    }

    impl Rec {
        fn take(&self) -> Vec<Ev> {
            std::mem::take(&mut *self.ev.lock().unwrap())
        }
    }

    type Clock = Arc<Mutex<Now>>;

    fn engine(store: Option<Store>, banners: bool, key: &str) -> (FocusEngine, Rec, Clock) {
        let rec = Rec::default();
        let clock = Arc::new(Mutex::new(at(0, 12, 0)));
        let c = clock.clone();
        let eng = FocusEngine::new(
            Config::default(),
            store,
            banners,
            true,
            key.into(),
            rec.clone(),
            move || *c.lock().unwrap(),
        );
        (eng, rec, clock)
    }

    fn night_store() -> Store {
        let mut st = Store::default();
        st.modes
            .iter_mut()
            .find(|m| m.id == model::SLEEP)
            .unwrap()
            .schedule = Some(Schedule {
            days: model::DAY_NAMES.iter().map(|d| (*d).to_string()).collect(),
            start: "22:00".into(),
            end: "07:00".into(),
        });
        st
    }

    #[test]
    fn manual_mode_restores_exactly_what_it_changed() {
        let (mut eng, rec, _) = engine(None, true, "");
        rec.take();
        // AC-FOCUS-01/02: banners off while active, back to the earlier value after
        eng.start(model::SLEEP, None).unwrap();
        assert_eq!(
            rec.take(),
            [
                Ev::Banners(false),
                Ev::Sounds(false),
                Ev::Key("sleep".into())
            ]
        );
        let saved = rec.saved.lock().unwrap().clone().unwrap();
        assert_eq!(
            saved.saved.show_banners,
            Some(true),
            "persisted before the write"
        );
        // switching to a mode that keeps sounds restores only the sounds
        eng.start(WORK, None).unwrap();
        assert_eq!(rec.take(), [Ev::Sounds(true), Ev::Key("work".into())]);
        eng.stop();
        assert_eq!(rec.take(), [Ev::Banners(true), Ev::Key(String::new())]);

        // banners already off before the mode: ending it leaves them off
        let (mut eng, rec, _) = engine(None, false, "");
        rec.take();
        eng.start(model::DND, None).unwrap();
        eng.stop();
        assert_eq!(rec.take(), [Ev::Key("dnd".into()), Ev::Key(String::new())]);
    }

    #[test]
    fn schedule_starts_and_ends_without_user_action() {
        // AC-FOCUS-03 and AC-FOCUS-06 (resume after the end): one timer per transition
        let (mut eng, rec, clock) = engine(Some(night_store()), true, "");
        let (delay, _) = eng.tick();
        assert_eq!(
            delay,
            Some(Duration::from_secs(10 * 3600)),
            "armed for 22:00"
        );
        *clock.lock().unwrap() = at(0, 22, 0);
        let (delay, changed) = eng.tick();
        assert!(changed);
        assert_eq!(eng.status().source, "schedule");
        assert_eq!(eng.status().until, at(1, 7, 0).unix);
        assert_eq!(
            delay,
            Some(Duration::from_secs(9 * 3600)),
            "armed for 07:00"
        );
        rec.take();
        // suspended across 07:00, resumed at 08:30
        *clock.lock().unwrap() = at(1, 8, 30);
        eng.tick();
        assert_eq!(eng.status().mode, "");
        assert_eq!(
            rec.take(),
            [Ev::Banners(true), Ev::Sounds(true), Ev::Key(String::new())]
        );
    }

    #[test]
    fn a_window_starts_its_mode_once() {
        let (mut eng, rec, clock) = engine(Some(night_store()), true, "");
        *clock.lock().unwrap() = at(0, 22, 0);
        eng.tick();
        // the user ends it early: it stays off for the rest of the window
        eng.stop();
        *clock.lock().unwrap() = at(0, 23, 0);
        eng.tick();
        assert_eq!(eng.status().mode, "");
        // clock set back before the start and forward again: no double start
        *clock.lock().unwrap() = at(0, 21, 0);
        eng.tick();
        *clock.lock().unwrap() = at(0, 22, 30);
        eng.tick();
        assert_eq!(eng.status().mode, "");
        rec.take();
        // the next night starts again
        *clock.lock().unwrap() = at(1, 22, 0);
        eng.tick();
        assert_eq!(eng.status().mode, model::SLEEP);
    }

    #[test]
    fn manual_start_overrides_schedule_until_it_ends() {
        let (mut eng, _, clock) = engine(Some(night_store()), true, "");
        eng.start(WORK, Some(at(0, 23, 0).unix)).unwrap();
        *clock.lock().unwrap() = at(0, 22, 0);
        eng.tick();
        assert_eq!(eng.status().mode, WORK);
        // the manual end inside the window: the scheduled mode takes over
        *clock.lock().unwrap() = at(0, 23, 0);
        eng.tick();
        assert_eq!(eng.status().mode, model::SLEEP);
        assert_eq!(eng.status().source, "schedule");
    }

    #[test]
    fn user_switching_banners_on_ends_the_mode() {
        // AC-FOCUS-08: the echo of our own write changes nothing; the user's switch ends the mode
        let (mut eng, rec, _) = engine(None, true, "");
        eng.start(model::SLEEP, None).unwrap();
        rec.take();
        assert!(!eng.on_external_show_banners(false));
        assert!(eng.on_external_show_banners(true));
        assert_eq!(eng.status().mode, "");
        assert_eq!(rec.take(), [Ev::Sounds(true), Ev::Key(String::new())]);
    }

    #[test]
    fn restart_reconciles_with_the_stored_state() {
        // AC-FOCUS-05: a mode that should still run continues
        let (mut eng, rec, _) = engine(None, true, "");
        eng.start(model::DND, Some(at(0, 13, 0).unix)).unwrap();
        let stored = rec.saved.lock().unwrap().clone();
        let (eng2, rec2, _) = engine(stored.clone(), false, "dnd");
        assert_eq!(eng2.status().mode, model::DND);
        assert!(rec2.take().is_empty(), "nothing to write");
        // it expired while deskd was down: banners restored
        let rec3 = Rec::default();
        let eng3 = FocusEngine::new(
            Config::default(),
            stored,
            false,
            true,
            "dnd".into(),
            rec3.clone(),
            || at(0, 14, 0),
        );
        assert_eq!(eng3.status().mode, "");
        assert_eq!(rec3.take(), [Ev::Banners(true), Ev::Key(String::new())]);
        // state file lost while active-mode was set: never leave banners blocked
        let (eng4, rec4, _) = engine(None, false, "dnd");
        assert_eq!(eng4.status().mode, "");
        assert_eq!(rec4.take(), [Ev::Banners(true), Ev::Key(String::new())]);
        // no state, no mark: the user's own do-not-disturb is left alone
        let (_, rec5, _) = engine(None, false, "");
        assert!(rec5.take().is_empty());
        drop(eng);
    }

    #[test]
    fn session_ends_with_a_break() {
        let (mut eng, rec, clock) = engine(None, true, "");
        eng.start_session(0).unwrap();
        assert_eq!(eng.status().source, "session");
        assert_eq!(eng.status().until, at(0, 12, 25).unix);
        rec.take();
        *clock.lock().unwrap() = at(0, 12, 25);
        let (delay, _) = eng.tick();
        assert!(eng.take_session_ended());
        assert!(!eng.take_session_ended());
        assert_eq!(eng.status().break_until, at(0, 12, 30).unix);
        assert_eq!(delay, Some(Duration::from_secs(300)));
        assert_eq!(rec.take(), [Ev::Banners(true), Ev::Key(String::new())]);
        *clock.lock().unwrap() = at(0, 12, 30);
        let (delay, _) = eng.tick();
        assert_eq!(eng.status().break_until, 0);
        assert_eq!(delay, None, "nothing armed: no idle wakeups");
    }

    #[test]
    fn modes_are_edited_and_built_ins_kept() {
        let (mut eng, _, _) = engine(None, true, "");
        let gym = Mode {
            id: "gym".into(),
            name: "Gym".into(),
            allowed_apps: vec!["org.gnome.Clocks".into()],
            ..Mode::default()
        };
        eng.save_mode(gym).unwrap();
        eng.start("gym", None).unwrap();
        assert_eq!(eng.status().allowed_apps, ["org.gnome.Clocks"]);
        assert!(eng.delete_mode(model::DND).is_err());
        eng.delete_mode("gym").unwrap();
        assert_eq!(eng.status().mode, "", "deleting the active mode ends it");
        assert!(eng.modes_json().contains("\"builtin\":true"));
        eng.set_config(Config {
            enabled: false,
            ..Config::default()
        });
        assert!(eng.start(model::DND, None).is_err());
    }
}
