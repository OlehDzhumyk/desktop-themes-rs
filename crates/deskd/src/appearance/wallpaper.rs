// SPDX-License-Identifier: GPL-3.0-or-later
//! Live "accent from wallpaper" (docs/spec/features/theme-from-wallpaper.md).
//!
//! When `accent-from-wallpaper` is on, deskd follows `org.gnome.desktop.background picture-uri`
//! and `picture-uri-dark` (GSettings signals, no polling), waits 500 ms for the changes to
//! settle, and sets `org.gnome.desktop.interface accent-color` to the GNOME named accent nearest
//! the wallpaper's seed colour. The image is decoded by a `themes tokens seed --json` child
//! process: a malformed file can crash the child, never deskd.
//!
//! The accent in force before the first write is kept in `~/.cache/desktop-themes/
//! wallpaper-accent.json`; turning the setting off restores it (when nobody changed the accent
//! since) and deletes the file. A left-over file at start-up with the setting off (deskd was
//! stopped in between) is reverted the same way.
//!
//! For a token theme the libadwaita palette follows too (ADR 0010): after each accepted seed a
//! `themes tokens overlay SEED` child puts the seed's palette into the generated `gtk.css` copy;
//! turning the setting off (or a left-over overlay at start-up) runs `themes tokens overlay
//! --remove`. One worker thread runs these children in order, so "off" after "on" always wins.
//! themes-core owns the overlay (its lock, its record in state.json); deskd only reads the
//! record for the status.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use gio::glib;
use gio::prelude::*;
use serde::{Deserialize, Serialize};

pub type Shared = Arc<Mutex<WallpaperEngine>>;

pub(crate) fn lock(e: &Shared) -> MutexGuard<'_, WallpaperEngine> {
    e.lock().unwrap_or_else(|p| p.into_inner())
}

/// At most one regeneration per this interval (slideshows, several keys written at once).
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// GNOME's named accents (`accent-color` enum values without `default`).
const NAMED: [&str; 9] = [
    "blue", "teal", "green", "yellow", "orange", "red", "pink", "purple", "slate",
];

/// Which wallpaper variant gives the accent (`accent-source`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Active,
    Light,
    Dark,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub source: Source,
}

/// The wallpaper keys and the colour scheme, as read from GSettings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    pub picture_uri: String,
    pub picture_uri_dark: String,
    pub prefer_dark: bool,
}

/// The URI whose image gives the accent, or `None` (off, or no wallpaper).
pub fn wanted_uri(cfg: &Config, i: &Inputs) -> Option<String> {
    if !cfg.enabled {
        return None;
    }
    let dark = match cfg.source {
        Source::Active => i.prefer_dark,
        Source::Light => false,
        Source::Dark => true,
    };
    let uri = if dark && !i.picture_uri_dark.is_empty() {
        &i.picture_uri_dark
    } else {
        &i.picture_uri
    };
    (!uri.is_empty()).then(|| uri.clone())
}

/// What `themes tokens seed --json` reports (other fields are ignored).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Seed {
    pub seed: String,
    pub accent: String,
    pub monochrome: bool,
}

/// Parses the child's output; an unknown accent name is refused (it would be written to dconf).
pub fn parse_seed(stdout: &str) -> Option<Seed> {
    let s: Seed = serde_json::from_str(stdout.trim()).ok()?;
    NAMED.contains(&s.accent.as_str()).then_some(s)
}

/// The cached state of the live mode.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Saved {
    /// `accent-color` before the live mode first wrote it (restored when it is turned off).
    pub original: String,
    /// The accent the live mode wrote last.
    pub written: String,
    pub seed: String,
    pub uri: String,
    pub monochrome: bool,
}

/// What D-Bus `WallpaperAccent()` returns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub enabled: bool,
    pub seed: String,
    pub accent: String,
    pub monochrome: bool,
    pub uri: String,
    pub error: String,
    /// The token theme whose libadwaita palette follows the wallpaper ("": accent only).
    pub palette: String,
}

/// Directory of the overlay next to `wallpaper-accent.json` (themes-core `overlay::DIR`).
pub const OVERLAY_DIR: &str = "wallpaper-overlay";

/// A request to `themes tokens overlay`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    Install(String),
    Remove,
}

/// The theme of the overlay recorded in the manager's state.json ("" without one).
pub fn overlay_theme(state_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(state_json)
        .ok()
        .and_then(|v| v["wallpaper_overlay"]["theme"].as_str().map(str::to_string))
        .unwrap_or_default()
}

pub struct WallpaperEngine {
    cfg: Config,
    saved: Option<Saved>,
    state_path: PathBuf,
    /// The manager's state.json (read-only: the overlay record), if known.
    manager_state: Option<PathBuf>,
    error: String,
}

impl WallpaperEngine {
    pub fn new(cfg: Config, state_path: PathBuf) -> Self {
        let saved = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        Self {
            cfg,
            saved,
            state_path,
            manager_state: None,
            error: String::new(),
        }
    }

    /// Where the manager keeps state.json (for the palette in the status).
    pub fn with_manager_state(mut self, p: PathBuf) -> Self {
        self.manager_state = Some(p);
        self
    }

    fn recorded_palette(&self) -> String {
        self.manager_state
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map_or_default(|t| overlay_theme(&t))
    }

    /// Whether an overlay may be installed: either its cache directory exists or the manager's
    /// `state.json` still records one (the record itself is themes-core's; `--remove` sorts out
    /// the rest, including after an externally cleared cache).
    pub fn overlay_left(&self) -> bool {
        self.state_path.with_file_name(OVERLAY_DIR).exists() || !self.recorded_palette().is_empty()
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    pub fn status(&self) -> Status {
        let s = self.saved.clone().unwrap_or_default();
        Status {
            enabled: self.cfg.enabled,
            seed: s.seed,
            accent: s.written,
            monochrome: s.monochrome,
            uri: s.uri,
            error: self.error.clone(),
            palette: self.recorded_palette(),
        }
    }

    /// With the setting off: forget the cached state and return the accent to restore, if the
    /// live mode's accent is still in force (a theme or the user may have changed it since).
    pub fn turn_off_if_disabled(&mut self, current_accent: &str) -> Option<String> {
        if self.cfg.enabled {
            return None;
        }
        self.error.clear();
        let saved = self.saved.take()?;
        if let Err(e) = std::fs::remove_file(&self.state_path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("deskd: cannot remove {}: {e}", self.state_path.display());
        }
        (current_accent == saved.written && saved.original != current_accent)
            .then_some(saved.original)
    }

    /// The seed of `uri` arrived. `wanted` is the URI the settings ask for now: a result for an
    /// older wallpaper is dropped. Returns the accent to write, if it differs from the current.
    pub fn on_seed(
        &mut self,
        uri: &str,
        wanted: Option<&str>,
        seed: Seed,
        current_accent: &str,
    ) -> Option<String> {
        if wanted != Some(uri) {
            return None;
        }
        self.error.clear();
        let original = match &self.saved {
            Some(s) => s.original.clone(),
            None => current_accent.to_string(),
        };
        let accent = seed.accent.clone();
        self.saved = Some(Saved {
            original,
            written: accent.clone(),
            seed: seed.seed,
            uri: uri.to_string(),
            monochrome: seed.monochrome,
        });
        self.save();
        (accent != current_accent).then_some(accent)
    }

    /// Decoding `uri` failed; the accent stays as it is. Returns whether the status changed.
    pub fn on_error(&mut self, uri: &str, wanted: Option<&str>, message: String) -> bool {
        if wanted != Some(uri) || self.error == message {
            return false;
        }
        self.error = message;
        true
    }

    /// `accent-color` changed. A value that is not ours (a theme was applied, the user picked
    /// an accent) becomes the accent to return to; it is kept until the wallpaper changes.
    pub fn on_external_accent(&mut self, current_accent: &str) -> bool {
        let Some(s) = self.saved.as_mut() else {
            return false;
        };
        if !self.cfg.enabled || s.written == current_accent {
            return false;
        }
        s.original = current_accent.to_string();
        s.written = current_accent.to_string();
        self.save();
        true
    }

    fn save(&self) {
        let Some(s) = &self.saved else {
            return;
        };
        let write = || -> std::io::Result<()> {
            if let Some(d) = self.state_path.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&self.state_path, serde_json::to_string_pretty(s)?)
        };
        if let Err(e) = write() {
            eprintln!("deskd: cannot write {}: {e}", self.state_path.display());
        }
    }
}

/// The manager's state.json (`DESKTOP_THEMES_ROOT`, else `~/.local/share/desktop-themes`).
pub fn manager_state_path() -> PathBuf {
    std::env::var_os("DESKTOP_THEMES_ROOT")
        .map_or_else(
            || glib::user_data_dir().join("desktop-themes"),
            PathBuf::from,
        )
        .join("state.json")
}

fn cache_dir_from(
    cache_env: Option<PathBuf>,
    root_env: Option<PathBuf>,
    default_root: &Path,
    user_cache: &Path,
) -> PathBuf {
    if let Some(p) = cache_env.filter(|p| !p.as_os_str().is_empty()) {
        return p;
    }
    if let Some(root) = root_env.filter(|p| !p.as_os_str().is_empty() && p != default_root) {
        return root.join(".cache");
    }
    user_cache.join("desktop-themes")
}

/// Cache root of themes-core (`DESKTOP_THEMES_CACHE`, custom `DESKTOP_THEMES_ROOT/.cache`, else
/// `$XDG_CACHE_HOME/desktop-themes`) + `wallpaper-accent.json`.
pub fn state_path() -> PathBuf {
    cache_dir_from(
        std::env::var_os("DESKTOP_THEMES_CACHE").map(PathBuf::from),
        std::env::var_os("DESKTOP_THEMES_ROOT").map(PathBuf::from),
        &glib::home_dir().join(".local/share/desktop-themes"),
        &glib::user_cache_dir(),
    )
    .join("wallpaper-accent.json")
}

// ------------------------------------------------------------------ GSettings and the child

const BACKGROUND_SCHEMA: &str = "org.gnome.desktop.background";
const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
const ACCENT_KEY: &str = "accent-color";

fn schema(id: &str) -> Option<gio::Settings> {
    desk_settings::schema_installed(id).then(|| gio::Settings::new(id))
}

/// `accent-color` exists since GNOME 47; older systems have nothing to follow.
fn has_accent(s: &gio::Settings) -> bool {
    s.settings_schema().is_some_and(|sc| sc.has_key(ACCENT_KEY))
}

fn read_inputs(bg: &gio::Settings, iface: &gio::Settings) -> Inputs {
    Inputs {
        picture_uri: bg.string("picture-uri").into(),
        picture_uri_dark: bg.string("picture-uri-dark").into(),
        prefer_dark: iface.string("color-scheme") == "prefer-dark",
    }
}

/// The `themes` binary next to deskd (an install puts both in one directory), else from PATH.
fn themes_bin() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("themes")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("themes"))
}

/// Runs `themes tokens seed --json PATH` (blocking; called on a worker thread).
fn decode(path: &Path) -> Result<Seed, String> {
    let out = std::process::Command::new(themes_bin())
        .args(["--json", "tokens", "seed"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("themes tokens seed: {}", out.status)
        } else {
            err
        });
    }
    parse_seed(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| "themes tokens seed: unexpected output".to_string())
}

/// Runs `themes --json tokens overlay SEED | --remove` (blocking; on the overlay worker).
fn run_overlay(job: &Job) -> Result<(), String> {
    let mut cmd = std::process::Command::new(themes_bin());
    cmd.args(["--json", "tokens", "overlay"]);
    match job {
        Job::Install(seed) => cmd.arg(seed),
        Job::Remove => cmd.arg("--remove"),
    };
    let out = cmd
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// One thread that runs the overlay children in the order they were asked for.
fn overlay_worker(notify: Arc<dyn Fn() + Send + Sync>) -> Jobs {
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    std::thread::spawn(move || {
        for job in rx {
            if let Err(e) = run_overlay(&job) {
                eprintln!("deskd: themes tokens overlay ({job:?}): {e}");
            }
            notify();
        }
    });
    tx
}

type Jobs = std::sync::mpsc::Sender<Job>;

/// Main-thread side of the live mode: GSettings signals, the debounce, the child process.
pub struct Watch {
    _bg: gio::Settings,
    iface: gio::Settings,
    pending: std::rc::Rc<std::cell::RefCell<Option<glib::SourceId>>>,
    eng: Shared,
    notify: Arc<dyn Fn() + Send + Sync>,
    jobs: Jobs,
}

impl Watch {
    /// Starts following the wallpaper; `None` without the GNOME schemas or `accent-color`.
    /// `notify` is called (on any thread) when the status changes.
    pub fn start(eng: Shared, notify: impl Fn() + Send + Sync + 'static) -> Option<Self> {
        let bg = schema(BACKGROUND_SCHEMA)?;
        let iface = schema(INTERFACE_SCHEMA).filter(has_accent)?;
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(notify);
        let w = Self {
            _bg: bg.clone(),
            iface: iface.clone(),
            pending: Default::default(),
            eng,
            jobs: overlay_worker(notify.clone()),
            notify,
        };
        for key in ["picture-uri", "picture-uri-dark"] {
            let k = w.kicker();
            bg.connect_changed(Some(key), move |_, _| k());
        }
        {
            let k = w.kicker();
            iface.connect_changed(Some("color-scheme"), move |_, _| k());
        }
        {
            let (eng, notify, jobs) = (w.eng.clone(), w.notify.clone(), w.jobs.clone());
            iface.connect_changed(Some(ACCENT_KEY), move |s, _| {
                let (changed, remove) = {
                    let mut e = lock(&eng);
                    let changed = e.on_external_accent(&s.string(ACCENT_KEY));
                    let remove = changed && e.overlay_left();
                    (changed, remove)
                };
                if remove {
                    let _ = jobs.send(Job::Remove);
                }
                if changed {
                    notify();
                }
            });
        }
        w.kick();
        Some(w)
    }

    /// Re-evaluate after the settings changed (debounced).
    pub fn kick(&self) {
        (self.kicker())();
    }

    fn kicker(&self) -> impl Fn() + 'static {
        let (pending, eng, notify) = (self.pending.clone(), self.eng.clone(), self.notify.clone());
        let jobs = self.jobs.clone();
        move || {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let (p2, eng, notify, jobs) =
                (pending.clone(), eng.clone(), notify.clone(), jobs.clone());
            let id = glib::timeout_add_local_once(DEBOUNCE, move || {
                // this source has fired: drop its id without removing it
                p2.borrow_mut().take();
                evaluate(&eng, &notify, &jobs);
            });
            *pending.borrow_mut() = Some(id);
        }
    }

    /// The setting was turned off: restore at once (no debounce; nothing to decode).
    pub fn settings_changed(&self, cfg: Config) {
        let off = !cfg.enabled;
        lock(&self.eng).set_config(cfg);
        if off {
            restore(&self.eng, &self.iface, &self.notify, &self.jobs);
        }
        self.kick();
    }
}

fn restore(eng: &Shared, iface: &gio::Settings, notify: &Arc<dyn Fn() + Send + Sync>, jobs: &Jobs) {
    let current = iface.string(ACCENT_KEY);
    let had = lock(eng).saved.is_some();
    // the palette first: the theme's own stylesheet is back when the accent returns
    if lock(eng).overlay_left() {
        let _ = jobs.send(Job::Remove);
    }
    if let Some(a) = lock(eng).turn_off_if_disabled(&current) {
        let _ = iface.set_string(ACCENT_KEY, &a);
    }
    if had {
        notify();
    }
}

/// Reads the settings (main thread) and, when a wallpaper is wanted, decodes it on a worker
/// thread; the result comes back to the main thread to be written.
fn evaluate(eng: &Shared, notify: &Arc<dyn Fn() + Send + Sync>, jobs: &Jobs) {
    let (Some(bg), Some(iface)) = (schema(BACKGROUND_SCHEMA), schema(INTERFACE_SCHEMA)) else {
        return;
    };
    if !lock(eng).config().enabled {
        restore(eng, &iface, notify, jobs);
        return;
    }
    let cfg = lock(eng).config().clone();
    let Some(uri) = wanted_uri(&cfg, &read_inputs(&bg, &iface)) else {
        return;
    };
    let Some(path) = gio::File::for_uri(&uri).path() else {
        return;
    };
    let (eng, notify, jobs) = (eng.clone(), notify.clone(), jobs.clone());
    std::thread::spawn(move || {
        let res = decode(&path);
        glib::MainContext::default().invoke(move || {
            let (Some(bg), Some(iface)) = (schema(BACKGROUND_SCHEMA), schema(INTERFACE_SCHEMA))
            else {
                return;
            };
            let mut e = lock(&eng);
            let wanted = wanted_uri(e.config(), &read_inputs(&bg, &iface));
            let changed = match res {
                Ok(seed) => {
                    let current = iface.string(ACCENT_KEY);
                    let hex = seed.seed.clone();
                    let accepted = wanted.as_deref() == Some(uri.as_str());
                    if let Some(a) = e.on_seed(&uri, wanted.as_deref(), seed, &current) {
                        let _ = iface.set_string(ACCENT_KEY, &a);
                    }
                    if accepted {
                        // token themes: the libadwaita palette follows too (idempotent)
                        let _ = jobs.send(Job::Install(hex));
                    }
                    accepted
                }
                Err(msg) => e.on_error(&uri, wanted.as_deref(), msg),
            };
            drop(e);
            if changed {
                notify();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deskd-wall-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("wallpaper-accent.json")
    }

    fn seed(accent: &str) -> Seed {
        Seed {
            seed: "#123456".into(),
            accent: accent.into(),
            monochrome: false,
        }
    }

    const ON: Config = Config {
        enabled: true,
        source: Source::Active,
    };

    #[test]
    fn wanted_uri_follows_source_and_scheme() {
        let i = Inputs {
            picture_uri: "file:///l.png".into(),
            picture_uri_dark: "file:///d.png".into(),
            prefer_dark: true,
        };
        let w = |source, i: &Inputs| {
            wanted_uri(
                &Config {
                    enabled: true,
                    source,
                },
                i,
            )
        };
        assert_eq!(w(Source::Active, &i).as_deref(), Some("file:///d.png"));
        assert_eq!(w(Source::Light, &i).as_deref(), Some("file:///l.png"));
        let light = Inputs {
            prefer_dark: false,
            ..i.clone()
        };
        assert_eq!(w(Source::Active, &light).as_deref(), Some("file:///l.png"));
        assert_eq!(w(Source::Dark, &light).as_deref(), Some("file:///d.png"));
        // no dark variant: the light one serves both
        let one = Inputs {
            picture_uri_dark: String::new(),
            ..i.clone()
        };
        assert_eq!(w(Source::Dark, &one).as_deref(), Some("file:///l.png"));
        assert_eq!(wanted_uri(&Config::default(), &i), None);
        assert_eq!(w(Source::Active, &Inputs::default()), None);
    }

    #[test]
    fn parse_seed_accepts_only_named_accents() {
        let ok = r##"{"seed":"#a1b2c3","accent":"teal","monochrome":false,"light":{}}"##;
        assert_eq!(
            parse_seed(ok),
            Some(Seed {
                seed: "#a1b2c3".into(),
                accent: "teal".into(),
                monochrome: false
            })
        );
        assert_eq!(
            parse_seed(r##"{"seed":"#a1b2c3","accent":"'; rm","monochrome":false}"##),
            None
        );
        assert_eq!(parse_seed("not json"), None);
    }

    #[test]
    fn follow_then_turn_off_restores_the_original_accent() {
        let path = tmp("cycle");
        let mut e = WallpaperEngine::new(ON, path.clone());
        let u = "file:///a.png";
        assert_eq!(
            e.on_seed(u, Some(u), seed("teal"), "blue").as_deref(),
            Some("teal")
        );
        assert!(path.is_file(), "state is cached");
        // a new wallpaper keeps the first original
        let v = "file:///b.png";
        assert_eq!(
            e.on_seed(v, Some(v), seed("red"), "teal").as_deref(),
            Some("red")
        );
        // the same accent again: nothing to write
        assert_eq!(e.on_seed(v, Some(v), seed("red"), "red"), None);
        // the status survives a restart
        let e2 = WallpaperEngine::new(ON, path.clone());
        assert_eq!(e2.status().accent, "red");
        e.set_config(Config::default());
        assert_eq!(e.turn_off_if_disabled("red").as_deref(), Some("blue"));
        assert!(!path.exists(), "the cache file is removed");
        assert_eq!(e.turn_off_if_disabled("blue"), None);
    }

    #[test]
    fn stale_results_and_foreign_accents() {
        let path = tmp("stale");
        let mut e = WallpaperEngine::new(ON, path.clone());
        let (a, b) = ("file:///a.png", "file:///b.png");
        // the wallpaper changed while `a` was decoded
        assert_eq!(e.on_seed(a, Some(b), seed("teal"), "blue"), None);
        assert!(!e.on_error(a, Some(b), "bad".into()));
        assert!(e.on_error(b, Some(b), "bad".into()));
        assert_eq!(e.status().error, "bad");
        assert_eq!(
            e.on_seed(b, Some(b), seed("teal"), "blue").as_deref(),
            Some("teal")
        );
        assert_eq!(e.status().error, "");
        // our own write echoes back: ignored
        assert!(!e.on_external_accent("teal"));
        // a theme sets purple: that is what "off" returns to, and nothing is written over it
        assert!(e.on_external_accent("purple"));
        e.set_config(Config::default());
        assert_eq!(e.turn_off_if_disabled("purple"), None);
        assert!(!path.exists());
    }

    #[test]
    fn overlay_record_and_leftover_cache() {
        assert_eq!(
            overlay_theme(
                r##"{"modules":{},"wallpaper_overlay":{"theme":"lavender","seed":"#2a7f62"}}"##
            ),
            "lavender"
        );
        assert_eq!(overlay_theme(r#"{"modules":{}}"#), "");
        assert_eq!(overlay_theme("broken"), "");
        let path = tmp("overlay");
        let e = WallpaperEngine::new(ON, path.clone());
        assert!(!e.overlay_left());
        assert_eq!(e.status().palette, "", "no manager state: accent only");
        let dir = path.with_file_name(OVERLAY_DIR).join("lavender");
        std::fs::create_dir_all(&dir).unwrap();
        let st = path.with_file_name("state.json");
        std::fs::write(&st, r#"{"wallpaper_overlay":{"theme":"lavender"}}"#).unwrap();
        let mut e = e.with_manager_state(st);
        assert!(e.overlay_left(), "a left-over cache asks for --remove");
        assert_eq!(e.status().palette, "lavender");
        // even after the cache directory is cleaned externally, state.json still triggers removal
        std::fs::remove_dir_all(path.with_file_name(OVERLAY_DIR)).unwrap();
        assert!(e.overlay_left());
        // an external accent change on an active wallpaper overlay also marks it for removal
        let u = "file:///a.png";
        assert_eq!(
            e.on_seed(u, Some(u), seed("teal"), "blue").as_deref(),
            Some("teal")
        );
        assert!(e.on_external_accent("purple"));
        assert!(e.overlay_left());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn cache_dir_matches_themes_core_rules() {
        let def = PathBuf::from("/home/user/.local/share/desktop-themes");
        let xdg = PathBuf::from("/home/user/.cache");
        assert_eq!(
            cache_dir_from(
                Some(PathBuf::from("/tmp/override")),
                Some(PathBuf::from("/tmp/root")),
                &def,
                &xdg,
            ),
            PathBuf::from("/tmp/override")
        );
        assert_eq!(
            cache_dir_from(None, Some(PathBuf::from("/tmp/root")), &def, &xdg),
            PathBuf::from("/tmp/root/.cache")
        );
        assert_eq!(
            cache_dir_from(None, Some(def.clone()), &def, &xdg),
            PathBuf::from("/home/user/.cache/desktop-themes")
        );
        assert_eq!(
            cache_dir_from(None, None, &def, &xdg),
            PathBuf::from("/home/user/.cache/desktop-themes")
        );
    }

    #[test]
    fn leftover_state_with_the_setting_off_is_reverted_at_start() {
        let path = tmp("leftover");
        let mut e = WallpaperEngine::new(ON, path.clone());
        let u = "file:///a.png";
        e.on_seed(u, Some(u), seed("green"), "orange");
        let mut e = WallpaperEngine::new(Config::default(), path.clone());
        assert_eq!(e.turn_off_if_disabled("green").as_deref(), Some("orange"));
        assert!(!path.exists());
    }
}
