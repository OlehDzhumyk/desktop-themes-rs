// SPDX-License-Identifier: GPL-3.0-or-later
//! The Windows 11 on-screen display: a small pill centred above the taskbar
//! (`Layer::Overlay`, no keyboard, 60 px over the bottom edge) that shows the output volume when
//! it changes (icon, bar, percent) or a short message of a one-shot command (OCR, colour picker,
//! screenshot): icon and text. It hides itself after [`HIDE_AFTER_MS`]; a new message replaces
//! the shown one and starts the time again.
//!
//! The volume comes from deskd's `Mixer.Changed`. The first value deskd gives is only remembered
//! ([`VolumeTracker`]), so the shell never shows the display when it starts or when deskd
//! (re)starts. Messages come from the `show-osd` action (`desk-shell --show-osd`), whose
//! parameter is [`encode`]d as `icon<TAB>text`.
//!
//! Without layer-shell the display is not shown: a normal window would take the focus.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use desk_settings::tr;
use gtk::glib;
use gtk::prelude::*;

use crate::deskd::{self, MixerDevice};
use crate::layer_shell::{self, Edge, KeyboardMode, Layer, Placement};

const PLACEMENT: Placement = Placement {
    namespace: "desk-shell-osd",
    layer: Layer::Overlay,
    anchors: &[Edge::Bottom],
    // above the 48 px taskbar, with the 12 px gap of the flyouts
    margins: &[(Edge::Bottom, 60)],
    exclusive_zone: false,
    keyboard: KeyboardMode::None,
};

/// How long the display stays (ms).
pub const HIDE_AFTER_MS: u64 = 1800;
/// The icon of a message without a valid one.
pub const DEFAULT_ICON: &str = "dialog-information-symbolic";
/// The longest message, in characters.
const MAX_TEXT: usize = 80;
/// The longest icon name that is accepted.
const MAX_ICON: usize = 64;

/// What the display shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// The output volume: whole percent (0..=150) and whether it is muted.
    Level { percent: u32, muted: bool },
    /// A message of a one-shot command.
    Toast { icon: String, text: String },
}

impl Content {
    /// The symbolic icon.
    pub fn icon(&self) -> String {
        match self {
            Content::Level { percent, muted } => {
                deskd::volume_icon(f64::from(*percent) / 100.0, *muted).to_owned()
            }
            Content::Toast { icon, .. } => icon.clone(),
        }
    }

    /// The fill of the level bar (0.0..=1.0); `None` for a message.
    pub fn fraction(&self) -> Option<f64> {
        match self {
            Content::Level { muted: true, .. } => Some(0.0),
            Content::Level { percent, .. } => Some(f64::from((*percent).min(100)) / 100.0),
            Content::Toast { .. } => None,
        }
    }

    /// The text next to the icon (the bare number for the volume, as on Windows 11).
    pub fn label(&self) -> String {
        match self {
            Content::Level { percent, .. } => percent.to_string(),
            Content::Toast { text, .. } => text.clone(),
        }
    }
}

/// The parameter of the `show-osd` action: `icon<TAB>text`.
pub fn encode(icon: &str, text: &str) -> String {
    format!("{icon}\t{text}")
}

/// A theme icon name: ASCII letters, digits, `-`, `_` and `.`, not empty, not too long.
fn valid_icon(icon: &str) -> bool {
    !icon.is_empty()
        && icon.len() <= MAX_ICON
        && icon
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// The message of a `show-osd` parameter: one line of at most 80 characters, and the icon when
/// it is a theme icon name (else [`DEFAULT_ICON`]). Without a tab the whole text is the
/// message. `None` when there is nothing to say.
pub fn decode(param: &str) -> Option<Content> {
    let (icon, text) = param.split_once('\t').unwrap_or(("", param));
    let text = deskd::preview(text, MAX_TEXT);
    if text.is_empty() {
        return None;
    }
    let icon = if valid_icon(icon) { icon } else { DEFAULT_ICON };
    Some(Content::Toast {
        icon: icon.to_owned(),
        text,
    })
}

/// The output volume as deskd reported it last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Volume {
    device: u32,
    percent: u32,
    muted: bool,
}

/// Decides when the volume is shown: only for a change of the default output after a first,
/// remembered value. The value at start-up, a new default device, or a lost device give
/// nothing; so does an update with the same volume (deskd also signals other changes).
#[derive(Clone, Debug, Default)]
pub struct VolumeTracker {
    last: Option<Volume>,
}

impl VolumeTracker {
    /// Take the default output `device` of a new `ListDevices` answer.
    pub fn update(&mut self, device: Option<&MixerDevice>) -> Option<Content> {
        let now = device.map(|d| Volume {
            device: d.id,
            percent: deskd::volume_percent(d.volume),
            muted: d.muted,
        });
        let before = std::mem::replace(&mut self.last, now);
        match (before, now) {
            (Some(b), Some(n)) if b.device == n.device && b != n => Some(Content::Level {
                percent: n.percent,
                muted: n.muted,
            }),
            _ => None,
        }
    }
}

/// The display.
pub struct Osd {
    window: gtk::Window,
    pill: gtk::Box,
    icon: gtk::Image,
    bar: gtk::LevelBar,
    label: gtk::Label,
    /// The layer surface could be set up (else the display is never shown).
    placed: bool,
    tracker: RefCell<VolumeTracker>,
    /// The pending hide; replaced by every new message.
    timer: RefCell<Option<glib::SourceId>>,
    /// Bumped by every `Mixer.Changed`: answers of an older one are dropped.
    generation: Cell<u64>,
}

impl Osd {
    /// Build the (hidden) display and follow the volume. `quiet` says when not to show the
    /// volume (the quick settings flyout is open: its slider shows the value already).
    pub fn new(app: &gtk::Application, quiet: impl Fn() -> bool + 'static) -> Rc<Osd> {
        let window = gtk::Window::builder()
            .application(app)
            .title(tr!("On-screen display"))
            .resizable(false)
            .focusable(false)
            .can_focus(false)
            .decorated(false)
            .build();
        let pill = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        pill.add_css_class("desk-shell-osd");
        pill.set_can_target(false);
        let icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
        let bar = gtk::LevelBar::builder()
            .min_value(0.0)
            .max_value(1.0)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        // no "low"/"high" offsets: one accent colour
        for offset in ["low", "high", "full"] {
            bar.remove_offset_value(Some(offset));
        }
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(40)
            .build();
        pill.append(&icon);
        pill.append(&bar);
        pill.append(&label);
        window.set_child(Some(&pill));
        let placed = layer_shell::place(&window, &PLACEMENT);
        let osd = Rc::new(Osd {
            window,
            pill,
            icon,
            bar,
            label,
            placed,
            tracker: RefCell::default(),
            timer: RefCell::default(),
            generation: Cell::new(0),
        });
        osd.follow_volume(quiet);
        osd
    }

    /// Remember the volume now (never shown), then show each change.
    fn follow_volume(self: &Rc<Self>, quiet: impl Fn() -> bool + 'static) {
        let quiet: Rc<dyn Fn() -> bool> = Rc::new(quiet);
        self.read_volume(Rc::new(|| true));
        let me = Rc::downgrade(self);
        deskd::watch_mixer(move || {
            if let Some(o) = me.upgrade() {
                o.read_volume(Rc::clone(&quiet));
            }
        });
    }

    fn read_volume(self: &Rc<Self>, quiet: Rc<dyn Fn() -> bool>) {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let me = Rc::downgrade(self);
        deskd::mixer_outputs(move |devices| {
            let Some(o) = me.upgrade().filter(|o| o.generation.get() == generation) else {
                return;
            };
            // no answer (deskd gone): keep the last value, so its return shows nothing
            let Some(devices) = devices else { return };
            let change = o
                .tracker
                .borrow_mut()
                .update(deskd::default_device(&devices));
            if let Some(content) = change.filter(|_| !quiet()) {
                o.show(&content);
            }
        });
    }

    /// Show `content` for [`HIDE_AFTER_MS`], replacing what is shown.
    pub fn show(self: &Rc<Self>, content: &Content) {
        if !self.placed {
            return;
        }
        self.icon.set_icon_name(Some(&content.icon()));
        let fraction = content.fraction();
        self.bar.set_visible(fraction.is_some());
        self.bar.set_value(fraction.unwrap_or(0.0));
        self.label.set_label(&content.label());
        let level = matches!(content, Content::Level { .. });
        self.label.set_width_chars(if level { 3 } else { -1 });
        let name = match content {
            Content::Level { muted: true, .. } => tr!("Volume muted"),
            Content::Level { percent, .. } => tr!("Volume {percent} %", percent = percent),
            Content::Toast { text, .. } => text.clone(),
        };
        self.pill
            .update_property(&[gtk::accessible::Property::Label(&name)]);
        for (class, on) in [
            ("osd-level", level),
            ("osd-toast", !level),
            (
                "osd-muted",
                matches!(content, Content::Level { muted: true, .. }),
            ),
        ] {
            if on {
                self.pill.add_css_class(class);
            } else {
                self.pill.remove_css_class(class);
            }
        }
        let animate = gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations());
        // restart the fade-in (a class change restarts the CSS animation)
        self.pill.remove_css_class("osd-in");
        if animate {
            self.pill.add_css_class("osd-in");
        }
        self.window.set_visible(true);
        if let Some(old) = self.timer.borrow_mut().take() {
            old.remove();
        }
        let me = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(
            std::time::Duration::from_millis(HIDE_AFTER_MS),
            move || {
                if let Some(o) = me.upgrade() {
                    // this source has run: forget it, so it is not removed again
                    o.timer.borrow_mut().take();
                    o.window.set_visible(false);
                }
            },
        );
        *self.timer.borrow_mut() = Some(id);
    }

    /// Show the message of a `show-osd` parameter (nothing for an empty one).
    pub fn show_param(self: &Rc<Self>, param: &str) {
        if let Some(content) = decode(param) {
            self.show(&content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(id: u32, volume: f64, muted: bool) -> MixerDevice {
        MixerDevice {
            id,
            description: "Speakers".into(),
            icon: String::new(),
            volume,
            muted,
            is_default: true,
        }
    }

    #[test]
    fn the_first_volume_is_only_remembered() {
        let mut t = VolumeTracker::default();
        // start-up: no display
        assert_eq!(t.update(Some(&output(41, 0.5, false))), None);
        // the same value again (deskd signals app changes too): no display
        assert_eq!(t.update(Some(&output(41, 0.5, false))), None);
        assert_eq!(
            t.update(Some(&output(41, 0.55, false))),
            Some(Content::Level {
                percent: 55,
                muted: false
            })
        );
        assert_eq!(
            t.update(Some(&output(41, 0.55, true))),
            Some(Content::Level {
                percent: 55,
                muted: true
            })
        );
        // a new default device is a new start, not a change
        assert_eq!(t.update(Some(&output(50, 0.9, false))), None);
        assert_eq!(
            t.update(Some(&output(50, 0.8, false))),
            Some(Content::Level {
                percent: 80,
                muted: false
            })
        );
        // the device goes away and comes back: nothing shown
        assert_eq!(t.update(None), None);
        assert_eq!(t.update(Some(&output(50, 0.3, false))), None);
        // a change below one percent is no change
        assert_eq!(t.update(Some(&output(50, 0.301, false))), None);
    }

    #[test]
    fn levels_show_icon_bar_and_percent() {
        let l = Content::Level {
            percent: 42,
            muted: false,
        };
        assert_eq!(l.label(), "42");
        assert_eq!(l.icon(), "audio-volume-medium-symbolic");
        assert!((l.fraction().unwrap() - 0.42).abs() < 1e-9);
        let muted = Content::Level {
            percent: 42,
            muted: true,
        };
        assert_eq!(muted.icon(), "audio-volume-muted-symbolic");
        assert_eq!(muted.fraction(), Some(0.0));
        // above 100 % the bar is full, the number tells the rest
        let loud = Content::Level {
            percent: 130,
            muted: false,
        };
        assert_eq!((loud.fraction(), loud.label().as_str()), (Some(1.0), "130"));
        assert_eq!(
            Content::Level {
                percent: 0,
                muted: false
            }
            .icon(),
            "audio-volume-muted-symbolic"
        );
    }

    #[test]
    fn messages_round_trip_and_are_cleaned() {
        let toast = |icon: &str, text: &str| Content::Toast {
            icon: icon.into(),
            text: text.into(),
        };
        assert_eq!(
            decode(&encode("color-select-symbolic", "#FF0000 (red)")),
            Some(toast("color-select-symbolic", "#FF0000 (red)"))
        );
        assert_eq!(
            decode("Text copied"),
            Some(toast(DEFAULT_ICON, "Text copied"))
        );
        // a tab in the text stays text; whitespace runs collapse to one line
        assert_eq!(
            decode("edit-copy-symbolic\ta\tb\n c"),
            Some(toast("edit-copy-symbolic", "a b c"))
        );
        // an icon that is not a theme icon name falls back
        for bad in ["/etc/passwd", "../x", "a b", "", &"x".repeat(65)] {
            assert_eq!(
                decode(&encode(bad, "hi")),
                Some(toast(DEFAULT_ICON, "hi")),
                "{bad}"
            );
        }
        assert_eq!(decode("icon\t  "), None);
        assert_eq!(decode(""), None);
        let long = "y".repeat(500);
        let Some(Content::Toast { text, .. }) = decode(&long) else {
            panic!("no toast");
        };
        assert_eq!(text.chars().count(), MAX_TEXT);
        assert_eq!(toast("i", "t").fraction(), None);
        assert_eq!(toast("i", "t").label(), "t");
    }

    #[test]
    fn the_display_sits_above_the_taskbar_without_keyboard() {
        assert_eq!(PLACEMENT.layer, Layer::Overlay);
        assert_eq!(PLACEMENT.keyboard, KeyboardMode::None);
        assert_eq!(PLACEMENT.anchors, &[Edge::Bottom]);
        assert_eq!(PLACEMENT.margins, &[(Edge::Bottom, 60)]);
        const { assert!(!PLACEMENT.exclusive_zone) };
        assert_eq!(PLACEMENT.namespace, "desk-shell-osd");
    }
}
