// SPDX-License-Identifier: GPL-3.0-or-later
//! The look of the shell: Windows 11 Fluent first (mica-like acrylic surfaces, rounded pills,
//! accent underline, 2 px focus ring), with light, dark and high-contrast palettes.
//!
//! Every shell window carries the classes of [`Look::classes`]; the palette lives in CSS custom
//! properties (GTK ≥ 4.16), so one stylesheet serves every combination. The `macos` and `default`
//! skins reuse the Windows 11 layout in this first version and only change the palette and the
//! corner radii (ADR 0012).

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use desk_settings::accessors::Skin;
use gtk::prelude::*;
use gtk::{gdk, gio};

/// What the classes of a shell window depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub skin: Skin,
    pub dark: bool,
    pub high_contrast: bool,
}

impl Look {
    /// The CSS classes of a shell window.
    pub fn classes(self) -> Vec<&'static str> {
        let skin = match self.skin {
            Skin::Windows11 => "skin-windows11",
            Skin::Macos => "skin-macos",
            Skin::Default => "skin-default",
        };
        let mut out = vec!["desk-shell", skin, if self.dark { "dark" } else { "light" }];
        if self.high_contrast {
            out.push("hc");
        }
        out
    }

    /// Put the classes on `widget`, replacing those of an earlier look.
    pub fn apply(self, widget: &impl IsA<gtk::Widget>) {
        for c in ALL_CLASSES {
            widget.remove_css_class(c);
        }
        for c in self.classes() {
            widget.add_css_class(c);
        }
    }
}

const ALL_CLASSES: [&str; 7] = [
    "desk-shell",
    "skin-windows11",
    "skin-macos",
    "skin-default",
    "dark",
    "light",
    "hc",
];

/// The stylesheet.
pub const CSS: &str = r#"
.desk-shell {
  --ds-bg: rgba(243, 243, 243, 0.92);
  --ds-flyout: rgba(249, 249, 249, 0.96);
  --ds-fg: #1b1b1b;
  --ds-dim: rgba(0, 0, 0, 0.62);
  --ds-hover: rgba(0, 0, 0, 0.055);
  --ds-active: rgba(0, 0, 0, 0.09);
  --ds-stroke: rgba(0, 0, 0, 0.08);
  --ds-field: rgba(255, 255, 255, 0.85);
  --ds-accent: #005fb8;
  --ds-on-accent: #ffffff;
  --ds-danger: #c42b1c;
  --ds-radius: 4px;
  --ds-flyout-radius: 8px;
  font-family: "Segoe UI Variable", "Segoe UI", "Inter", "Cantarell", sans-serif;
  color: var(--ds-fg);
}
.desk-shell.dark {
  --ds-bg: rgba(32, 32, 32, 0.90);
  --ds-flyout: rgba(44, 44, 44, 0.96);
  --ds-fg: #ffffff;
  --ds-dim: rgba(255, 255, 255, 0.72);
  --ds-hover: rgba(255, 255, 255, 0.06);
  --ds-active: rgba(255, 255, 255, 0.10);
  --ds-stroke: rgba(255, 255, 255, 0.09);
  --ds-field: rgba(255, 255, 255, 0.06);
  --ds-accent: #60cdff;
  --ds-on-accent: #000000;
  --ds-danger: #ff99a4;
}
.desk-shell.skin-windows11 { --ds-radius: 4px; --ds-flyout-radius: 8px; }
.desk-shell.skin-macos { --ds-accent: #007aff; --ds-radius: 6px; --ds-flyout-radius: 12px; }
.desk-shell.skin-macos.dark { --ds-accent: #0a84ff; --ds-on-accent: #ffffff; }
.desk-shell.skin-default { --ds-accent: #3584e4; --ds-on-accent: #ffffff; --ds-radius: 6px; --ds-flyout-radius: 12px; }
.desk-shell.hc {
  --ds-bg: #000000;
  --ds-flyout: #000000;
  --ds-fg: #ffffff;
  --ds-dim: #ffffff;
  --ds-hover: #1aebff;
  --ds-active: #1aebff;
  --ds-stroke: #ffffff;
  --ds-field: #000000;
  --ds-accent: #ffff00;
  --ds-on-accent: #000000;
  --ds-danger: #ffff00;
}
.desk-shell.hc.light {
  --ds-bg: #ffffff;
  --ds-flyout: #ffffff;
  --ds-fg: #000000;
  --ds-dim: #000000;
  --ds-hover: #37006e;
  --ds-active: #37006e;
  --ds-stroke: #000000;
  --ds-field: #ffffff;
  --ds-accent: #37006e;
  --ds-on-accent: #ffffff;
  --ds-danger: #37006e;
}

window.desk-shell { background: none; }

.desk-shell-taskbar {
  min-height: 48px;
  background-color: var(--ds-bg);
  box-shadow: inset 0 1px 0 var(--ds-stroke);
  padding: 0 8px;
}
.desk-shell-taskbar button {
  min-height: 40px;
  min-width: 40px;
  margin: 4px 2px;
  padding: 0 6px;
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
}
.desk-shell-taskbar button:hover { background-color: var(--ds-hover); }
.desk-shell-taskbar button:active,
.desk-shell-taskbar button:checked { background-color: var(--ds-active); }
.desk-shell-taskbar .pin image { -gtk-icon-size: 24px; }
.desk-shell-taskbar .pin.active { background-color: var(--ds-hover); }
.desk-shell-taskbar .app-indicator {
  min-width: 6px;
  min-height: 3px;
  margin-bottom: 1px;
  border-radius: 2px;
  background-color: var(--ds-dim);
}
.desk-shell-taskbar .app-indicator.active { min-width: 16px; background-color: var(--ds-accent); }

.desk-shell-start-tile { min-width: 8px; min-height: 8px; border-radius: 1px; background-color: #0078d4; }
.desk-shell.dark .desk-shell-start-tile { background-color: #4cc2ff; }
.desk-shell.hc .desk-shell-start-tile { background-color: var(--ds-accent); }

.desk-shell-search-pill {
  min-width: 180px;
  border-radius: 999px;
  background-color: var(--ds-field);
  box-shadow: inset 0 0 0 1px var(--ds-stroke);
}
.desk-shell-search-pill label { color: var(--ds-dim); }

.desk-shell-clock { padding: 0 8px; }
.desk-shell-clock label { font-size: 12px; }

.desk-shell-start-menu {
  background-color: var(--ds-flyout);
  border: 1px solid var(--ds-stroke);
  border-radius: var(--ds-flyout-radius);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.28);
}
.desk-shell-start-menu .start-body { padding: 24px 32px 12px 32px; }
.desk-shell-start-menu entry,
.desk-shell-start-menu searchbar entry {
  min-height: 36px;
  border-radius: 999px;
  background-color: var(--ds-field);
  border: 1px solid var(--ds-stroke);
  border-bottom: 2px solid var(--ds-accent);
  color: var(--ds-fg);
}
.desk-shell-start-menu .section-title { font-weight: 600; margin: 16px 0 8px 4px; }
.desk-shell-start-menu .tile {
  min-width: 96px;
  min-height: 80px;
  padding: 8px 4px;
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
}
.desk-shell-start-menu .tile image { -gtk-icon-size: 32px; }
.desk-shell-start-menu .tile label { font-size: 12px; }
.desk-shell-start-menu .tile:hover { background-color: var(--ds-hover); }
.desk-shell-start-menu .tile:active { background-color: var(--ds-active); }
.desk-shell-start-menu list { background: none; }
.desk-shell-start-menu list row { border-radius: var(--ds-radius); padding: 6px 8px; margin: 1px 0; }
.desk-shell-start-menu list row:hover { background-color: var(--ds-hover); }
.desk-shell-start-menu list row:selected {
  background-color: var(--ds-active);
  color: var(--ds-fg);
  box-shadow: inset 3px 0 0 var(--ds-accent);
}
.desk-shell-start-menu .result-subtitle { color: var(--ds-dim); font-size: 12px; }
.desk-shell-start-menu .empty { color: var(--ds-dim); margin: 24px; }
.desk-shell-start-menu .start-footer {
  min-height: 56px;
  padding: 0 32px;
  border-top: 1px solid var(--ds-stroke);
  background-color: var(--ds-hover);
  border-radius: 0 0 var(--ds-flyout-radius) var(--ds-flyout-radius);
}
.desk-shell-start-menu .start-footer button {
  min-height: 36px;
  min-width: 36px;
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
}
.desk-shell-start-menu .start-footer button:hover { background-color: var(--ds-hover); }

.desk-shell-tray-pill { padding: 0 10px; }
.desk-shell-taskbar .battery-low,
.desk-shell-flyout .battery-low { color: var(--ds-danger); }

.desk-shell-window-menu > contents {
  background-color: var(--ds-flyout);
  border: 1px solid var(--ds-stroke);
  border-radius: var(--ds-flyout-radius);
  padding: 4px;
  color: var(--ds-fg);
}
.desk-shell-window-menu button {
  min-height: 32px;
  padding: 0 12px;
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
}
.desk-shell-window-menu button:hover { background-color: var(--ds-hover); }
.desk-shell-window-menu button label { font-weight: normal; }

.desk-shell-flyout {
  background-color: var(--ds-flyout);
  border: 1px solid var(--ds-stroke);
  border-radius: var(--ds-flyout-radius);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.28);
  padding: 16px;
  color: var(--ds-fg);
}
.desk-shell-flyout button {
  border-radius: var(--ds-radius);
  background: none;
  border: none;
  box-shadow: none;
  color: var(--ds-fg);
  min-height: 32px;
  min-width: 32px;
}
.desk-shell-flyout button:hover { background-color: var(--ds-hover); }
.desk-shell-flyout button:active { background-color: var(--ds-active); }
.desk-shell-flyout .section-title { font-weight: 600; margin: 4px 0; }
.desk-shell-flyout .dim,
.desk-shell-flyout .qs-percent { color: var(--ds-dim); font-size: 12px; }
.desk-shell-flyout .empty { color: var(--ds-dim); margin: 24px; }
.desk-shell-flyout .qs-tile {
  min-height: 72px;
  padding: 8px 4px;
  background-color: var(--ds-field);
  border: 1px solid var(--ds-stroke);
}
.desk-shell-flyout .qs-tile image { -gtk-icon-size: 20px; }
.desk-shell-flyout .qs-tile label { font-size: 12px; }
.desk-shell-flyout .qs-tile:checked {
  background-color: var(--ds-accent);
  color: var(--ds-on-accent);
  border-color: var(--ds-accent);
}
.desk-shell-flyout .qs-tile:disabled { opacity: 0.5; }
.desk-shell-flyout .qs-brightness {
  border-top: 1px solid var(--ds-stroke);
  padding-top: 12px;
}
.desk-shell-flyout .qs-brightness image { min-width: 32px; }
.desk-shell-flyout .qs-audio {
  border-top: 1px solid var(--ds-stroke);
  padding-top: 12px;
}
.desk-shell-flyout .qs-footer {
  border-top: 1px solid var(--ds-stroke);
  padding-top: 10px;
}
.desk-shell-flyout .qs-battery {
  padding: 4px 8px;
  border-radius: var(--ds-radius);
  background-color: var(--ds-field);
}
.desk-shell-flyout .qs-battery label { font-size: 12px; }
.desk-shell-flyout scale trough { min-height: 4px; border-radius: 2px; background-color: var(--ds-stroke); }
.desk-shell-flyout scale highlight { background-color: var(--ds-accent); border-radius: 2px; }
.desk-shell-flyout scale slider {
  min-width: 18px;
  min-height: 18px;
  border-radius: 999px;
  background-color: var(--ds-accent);
  border: 4px solid var(--ds-flyout);
}
.desk-shell-flyout list { background: none; }
.desk-shell-flyout list row {
  border-radius: var(--ds-radius);
  padding: 8px;
  margin: 2px 0;
  background-color: var(--ds-field);
  border: 1px solid var(--ds-stroke);
}
.desk-shell-flyout list row:hover { background-color: var(--ds-hover); }
.desk-shell-flyout list row:selected {
  background-color: var(--ds-active);
  color: var(--ds-fg);
  box-shadow: inset 3px 0 0 var(--ds-accent);
}
.desk-shell-flyout .clip-pin-badge { color: var(--ds-accent); }
.desk-shell-flyout .clip-banner {
  padding: 8px;
  border-radius: var(--ds-radius);
  background-color: var(--ds-active);
  border-left: 3px solid var(--ds-accent);
}
.desk-shell-flyout .emoji-tabs button { min-width: 32px; padding: 0; }
.desk-shell-flyout .emoji-tabs button:checked {
  background-color: var(--ds-active);
  box-shadow: inset 0 -2px 0 var(--ds-accent);
}
.desk-shell-flyout .emoji-cell {
  min-width: 36px;
  min-height: 36px;
  padding: 0;
  font-size: 22px;
  font-family: "Noto Color Emoji", "Segoe UI Emoji", "Twemoji", sans-serif;
}
.desk-shell-flyout flowboxchild { padding: 0; }
.desk-shell-flyout entry {
  min-height: 32px;
  border-radius: var(--ds-radius);
  background-color: var(--ds-field);
  border: 1px solid var(--ds-stroke);
  border-bottom: 2px solid var(--ds-accent);
  color: var(--ds-fg);
}

.desk-shell-osd {
  min-height: 48px;
  min-width: 192px;
  padding: 0 16px;
  border-radius: var(--ds-flyout-radius);
  background-color: var(--ds-flyout);
  border: 1px solid var(--ds-stroke);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.24);
  color: var(--ds-fg);
}
.desk-shell-osd image { -gtk-icon-size: 20px; }
.desk-shell-osd.osd-level { min-width: 240px; }
.desk-shell-osd levelbar { min-width: 120px; }
.desk-shell-osd levelbar trough { min-height: 4px; border-radius: 2px; background-color: var(--ds-stroke); border: none; box-shadow: none; padding: 0; }
.desk-shell-osd levelbar block { border-radius: 2px; border: none; box-shadow: none; min-height: 4px; }
.desk-shell-osd levelbar block.filled { background-color: var(--ds-accent); }
.desk-shell-osd levelbar block.empty { background-color: transparent; }
.desk-shell-osd.osd-muted label { color: var(--ds-dim); }
.desk-shell-osd label { font-size: 14px; }
.desk-shell-osd.osd-in { animation: desk-shell-osd-in 160ms ease-out; }
@keyframes desk-shell-osd-in {
  from { opacity: 0; }
  to { opacity: 1; }
}
.desk-shell.hc .desk-shell-osd { border: 2px solid var(--ds-stroke); box-shadow: none; }

.desk-shell button:focus-visible,
.desk-shell row:focus-visible {
  outline: 2px solid var(--ds-fg);
  outline-offset: -2px;
}
.desk-shell.hc button:hover,
.desk-shell.hc row:selected,
.desk-shell.hc .qs-tile:checked { color: var(--ds-on-accent); }
"#;

/// Subdirectory under `$XDG_CONFIG_HOME` holding the theme-generated stylesheet.
pub const USER_CSS_DIR: &str = "desk-shell";
/// Filename of the theme-generated stylesheet.
pub const USER_CSS_FILE: &str = "desk-shell.css";
/// Priority for user/theme CSS overrides: above built-in CSS while `.hc` stays protected.
pub const USER_CSS_PRIORITY: u32 = gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1;
/// Bound on the user stylesheet size (1 MiB).
const MAX_USER_CSS_BYTES: u64 = 1024 * 1024;

/// Resolve `$XDG_CONFIG_HOME/desk-shell/desk-shell.css`, falling back to
/// `$HOME/.config/desk-shell/desk-shell.css`.
pub fn desk_shell_css_path_from(
    xdg_config_home: Option<&str>,
    home: Option<&str>,
) -> Option<PathBuf> {
    if let Some(xdg) = xdg_config_home.filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(xdg).join(USER_CSS_DIR).join(USER_CSS_FILE));
    }
    home.filter(|s| !s.is_empty()).map(|h| {
        PathBuf::from(h)
            .join(".config")
            .join(USER_CSS_DIR)
            .join(USER_CSS_FILE)
    })
}

/// Resolve the user stylesheet path from the process environment.
pub fn desk_shell_css_path() -> Option<PathBuf> {
    desk_shell_css_path_from(
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// Read the user stylesheet from `path`, returning an empty string when absent or unreadable.
pub fn read_user_css(path: &Path) -> String {
    let Ok(meta) = std::fs::metadata(path) else {
        return String::new();
    };
    if !meta.is_file() || meta.len() > MAX_USER_CSS_BYTES {
        return String::new();
    }
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Whether a directory monitor event touching `file` / `other` affects `target_name`.
pub fn matches_file_name(file: Option<&Path>, other: Option<&Path>, target_name: &str) -> bool {
    let target = OsStr::new(target_name);
    [file, other]
        .into_iter()
        .flatten()
        .any(|p| p.file_name() == Some(target))
}

/// Live watcher for the optional theme-generated `desk-shell.css`.
///
/// Must be kept alive for the lifetime of the shell so its `gio::FileMonitor` handles stay active.
pub struct CssWatcher {
    provider: gtk::CssProvider,
    path: PathBuf,
    dir_monitor: RefCell<Option<gio::FileMonitor>>,
    _parent_monitor: Option<gio::FileMonitor>,
}

impl CssWatcher {
    fn new(display: &gdk::Display, path: PathBuf) -> Rc<CssWatcher> {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&read_user_css(&path));
        gtk::style_context_add_provider_for_display(display, &provider, USER_CSS_PRIORITY);

        let parent_monitor = path.parent().and_then(Path::parent).and_then(|cfg| {
            gio::File::for_path(cfg)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                .ok()
        });

        let watcher = Rc::new(CssWatcher {
            provider,
            path,
            dir_monitor: RefCell::default(),
            _parent_monitor: parent_monitor,
        });
        watcher.watch_dir();

        if let Some(pm) = &watcher._parent_monitor {
            let me = Rc::downgrade(&watcher);
            pm.connect_changed(move |_, file, other, _| {
                if matches_file_name(
                    file.path().as_deref(),
                    other.and_then(gio::File::path).as_deref(),
                    USER_CSS_DIR,
                ) && let Some(w) = me.upgrade()
                {
                    w.watch_dir();
                    w.reload();
                }
            });
        }
        watcher
    }

    fn watch_dir(self: &Rc<Self>) {
        let Some(dir) = self.path.parent() else {
            return;
        };
        if !dir.is_dir() {
            *self.dir_monitor.borrow_mut() = None;
            return;
        }
        let Ok(monitor) = gio::File::for_path(dir)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        else {
            return;
        };
        let me = Rc::downgrade(self);
        monitor.connect_changed(move |_, file, other, event| {
            let deleted_dir = matches!(
                event,
                gio::FileMonitorEvent::Deleted | gio::FileMonitorEvent::Unmounted
            ) && file.path().as_deref()
                == me
                    .upgrade()
                    .and_then(|w| w.path.parent().map(Path::to_path_buf))
                    .as_deref();
            if (deleted_dir
                || matches_file_name(
                    file.path().as_deref(),
                    other.and_then(gio::File::path).as_deref(),
                    USER_CSS_FILE,
                ))
                && let Some(w) = me.upgrade()
            {
                w.reload();
            }
        });
        *self.dir_monitor.borrow_mut() = Some(monitor);
    }

    /// Reload `desk-shell.css` from disk, clearing overrides when the file is missing.
    pub fn reload(&self) {
        self.provider.load_from_string(&read_user_css(&self.path));
    }
}

/// Load built-in [`CSS`] and attach a live watcher for `$XDG_CONFIG_HOME/desk-shell/desk-shell.css`.
pub fn install(display: &gdk::Display) -> Option<Rc<CssWatcher>> {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    desk_shell_css_path().map(|path| CssWatcher::new(display, path))
}

/// The current look: `skin`, the scheme deskd reports (`deskd_dark`, it wins: on labwc GTK may
/// not see deskd's change), else the system's colour scheme, and the contrast preference.
pub fn current(skin: Skin, deskd_dark: Option<bool>) -> Look {
    let settings = gtk::Settings::default();
    let dark = deskd_dark.unwrap_or_else(|| {
        settings
            .as_ref()
            .is_some_and(|s| s.gtk_interface_color_scheme() == gtk::InterfaceColorScheme::Dark)
    });
    let high_contrast = settings
        .as_ref()
        .is_some_and(|s| s.gtk_interface_contrast() == gtk::InterfaceContrast::More);
    Look {
        skin,
        dark,
        high_contrast,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_cover_every_combination() {
        for skin in Skin::ALL {
            for dark in [false, true] {
                for high_contrast in [false, true] {
                    let c = Look {
                        skin,
                        dark,
                        high_contrast,
                    }
                    .classes();
                    assert_eq!(c[0], "desk-shell");
                    assert!(c.iter().all(|x| ALL_CLASSES.contains(x)), "{c:?}");
                    assert_eq!(c.contains(&"hc"), high_contrast);
                    // every class the code sets has a rule in the stylesheet
                    for class in &c {
                        assert!(CSS.contains(&format!(".{class}")), "{class}");
                    }
                }
            }
        }
    }

    /// The declarations of every rule whose selector starts with `selector`.
    fn rules_of(selector: &str) -> Vec<&'static str> {
        CSS.split('}')
            .filter_map(|rule| rule.split_once('{'))
            .filter(|(sel, _)| sel.trim().starts_with(selector))
            .map(|(_, body)| body)
            .collect()
    }

    /// The declarations of the rule with exactly `selector`.
    fn rule(selector: &str) -> &'static str {
        let (_, body) = CSS
            .split('}')
            .filter_map(|rule| rule.split_once('{'))
            .find(|(sel, _)| sel.trim() == selector)
            .unwrap_or_else(|| panic!("no rule {selector}"));
        body
    }

    #[test]
    fn the_taskbar_is_exactly_48_px_high() {
        // ADR 0012: 48 px surface and exclusive zone. A vertical border, padding or margin on
        // the bar, or a border on a 40 px button with 4 px margins, makes it taller (labwc
        // reported 49 px with a 1 px top border), so lines and indicators are inset shadows
        // and overlays instead.
        assert!(rule(".desk-shell-taskbar").contains("min-height: 48px"));
        for selector in [
            ".desk-shell-taskbar",
            ".desk-shell-search-pill",
            ".desk-shell-tray-pill",
            ".desk-shell-clock",
        ] {
            for body in rules_of(selector) {
                for bad in ["border-top", "border-bottom", "border: 1px", "border:1px"] {
                    assert!(!body.contains(bad), "{selector}: {bad}");
                }
            }
        }
        let button = rule(".desk-shell-taskbar button");
        assert!(button.contains("min-height: 40px") && button.contains("margin: 4px 2px"));
        assert!(button.contains("padding: 0 6px") && button.contains("border: none"));
    }

    #[test]
    fn stylesheet_is_balanced_and_names_the_surfaces() {
        assert_eq!(CSS.matches('{').count(), CSS.matches('}').count());
        for surface in [
            ".desk-shell-taskbar",
            ".desk-shell-start-menu",
            ".desk-shell-flyout",
            ".qs-tile:checked",
            ".clip-banner",
            ":focus-visible",
            ".desk-shell.hc",
            ".desk-shell.dark",
            ".app-indicator.active",
            ".desk-shell-window-menu",
            ".desk-shell-osd",
            ".osd-in",
            ".emoji-cell",
            ".emoji-tabs",
            ".battery-low",
            ".qs-brightness",
            ".qs-footer",
            ".qs-battery",
        ] {
            assert!(CSS.contains(surface), "{surface}");
        }
    }

    #[test]
    fn desk_shell_css_path_respects_xdg_config_home_and_home() {
        assert_eq!(
            desk_shell_css_path_from(Some("/tmp/xdg"), Some("/home/user")),
            Some(PathBuf::from("/tmp/xdg/desk-shell/desk-shell.css"))
        );
        // Empty XDG_CONFIG_HOME falls back to $HOME/.config
        assert_eq!(
            desk_shell_css_path_from(Some(""), Some("/home/user")),
            Some(PathBuf::from(
                "/home/user/.config/desk-shell/desk-shell.css"
            ))
        );
        assert_eq!(
            desk_shell_css_path_from(None, Some("/home/user")),
            Some(PathBuf::from(
                "/home/user/.config/desk-shell/desk-shell.css"
            ))
        );
        assert_eq!(desk_shell_css_path_from(None, None), None);
        assert_eq!(desk_shell_css_path_from(Some(""), Some("")), None);
    }

    #[test]
    fn user_css_reader_and_event_matcher_handle_missing_and_renames() {
        let dir = std::env::temp_dir().join(format!("desk-shell-css-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(USER_CSS_FILE);
        assert_eq!(read_user_css(&file), "");
        std::fs::write(&file, ".desk-shell { --ds-accent: #123456; }").unwrap();
        assert_eq!(
            read_user_css(&file),
            ".desk-shell { --ds-accent: #123456; }"
        );
        std::fs::remove_file(&file).unwrap();
        assert_eq!(read_user_css(&file), "");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(matches_file_name(
            Some(Path::new("/home/user/.config/desk-shell/desk-shell.css")),
            None,
            USER_CSS_FILE
        ));
        // Atomic rename: temp file -> desk-shell.css
        assert!(matches_file_name(
            Some(Path::new("/home/user/.config/desk-shell/.tmp123")),
            Some(Path::new("/home/user/.config/desk-shell/desk-shell.css")),
            USER_CSS_FILE
        ));
        assert!(!matches_file_name(
            Some(Path::new("/home/user/.config/desk-shell/other.css")),
            None,
            USER_CSS_FILE
        ));
    }
}
