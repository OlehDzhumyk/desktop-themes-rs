// SPDX-License-Identifier: GPL-3.0-or-later
//! The frame of the small Windows 11 flyouts at the right end of the taskbar (quick settings,
//! clipboard history): a layer surface anchored to the bottom-right corner (`Layer::Overlay`,
//! keyboard on demand, 12 px above the taskbar), closed by `Esc` and by a focus loss, and
//! mirrored by a toggle button on the taskbar.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::layer_shell::{self, Edge, KeyboardMode, Layer, Placement};

/// The gap between a flyout and the taskbar or the right screen edge.
pub const MARGIN: i32 = 12;
/// A click on the taskbar button right after a focus loss closed the flyout must not reopen it.
const REOPEN_GUARD_USEC: i64 = 300_000;

/// The placement of a bottom-right flyout with the layer-shell `namespace`.
pub const fn placement(namespace: &'static str) -> Placement {
    Placement {
        namespace,
        layer: Layer::Overlay,
        anchors: &[Edge::Bottom, Edge::Right],
        margins: &[(Edge::Bottom, MARGIN), (Edge::Right, MARGIN)],
        exclusive_zone: false,
        keyboard: KeyboardMode::OnDemand,
    }
}

/// A reopen right after a focus-loss close is the same click: `false` means stay closed.
pub fn may_reopen(now_usec: i64, closed_at_usec: i64) -> bool {
    now_usec - closed_at_usec >= REOPEN_GUARD_USEC
}

/// A hidden flyout window and the taskbar button that mirrors it.
pub struct Flyout {
    pub window: gtk::Window,
    button: RefCell<Option<glib::WeakRef<gtk::ToggleButton>>>,
    /// When a focus loss last closed the flyout (monotonic µs).
    closed_at: Cell<i64>,
}

impl Flyout {
    /// Build the (hidden) window with `child` in a `.desk-shell-flyout` surface.
    pub fn new(
        app: &gtk::Application,
        title: &str,
        placement: &Placement,
        width: i32,
        child: &impl IsA<gtk::Widget>,
    ) -> Rc<Flyout> {
        let window = gtk::Window::builder()
            .application(app)
            .title(title)
            .resizable(false)
            .default_width(width)
            .hide_on_close(true)
            .build();
        let surface = gtk::Box::new(gtk::Orientation::Vertical, 0);
        surface.add_css_class("desk-shell-flyout");
        surface.set_size_request(width, -1);
        surface.append(child);
        window.set_child(Some(&surface));
        let flyout = Rc::new(Flyout {
            window,
            button: RefCell::default(),
            closed_at: Cell::new(0),
        });
        flyout.connect();
        layer_shell::place(&flyout.window, placement);
        flyout
    }

    fn connect(self: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| match me.upgrade() {
            Some(f) if key == gdk::Key::Escape => {
                f.hide();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        self.window.add_controller(keys);
        let me = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |w| {
            if let Some(f) = me.upgrade()
                && !w.is_active()
                && w.is_visible()
            {
                f.closed_at.set(glib::monotonic_time());
                f.hide();
            }
        });
        let me = Rc::downgrade(self);
        self.window.connect_hide(move |_| {
            if let Some(f) = me.upgrade() {
                f.set_button_state(false);
            }
        });
    }

    fn set_button_state(&self, active: bool) {
        if let Some(b) = self
            .button
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
        {
            b.set_active(active);
        }
    }

    /// The taskbar button mirrors whether the flyout is open.
    pub fn set_button(&self, button: &gtk::ToggleButton) {
        *self.button.borrow_mut() = Some(button.downgrade());
    }

    pub fn is_open(&self) -> bool {
        self.window.is_visible()
    }

    pub fn show(&self) {
        self.window.present();
        self.set_button_state(true);
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
        self.set_button_state(false);
    }

    /// Open or close; `before_show` refreshes the content first.
    pub fn toggle(&self, before_show: impl FnOnce()) {
        if self.is_open() {
            self.hide();
        } else if !may_reopen(glib::monotonic_time(), self.closed_at.get()) {
            // this click is what took the focus away: it closed the flyout already
            self.set_button_state(false);
        } else {
            before_show();
            self.show();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flyouts_sit_in_the_bottom_right_corner() {
        let p = placement("desk-shell-test");
        assert_eq!(p.layer, Layer::Overlay);
        assert_eq!(p.keyboard, KeyboardMode::OnDemand);
        assert_eq!(p.anchors, &[Edge::Bottom, Edge::Right]);
        assert!(p.margins.contains(&(Edge::Bottom, MARGIN)));
        assert!(!p.exclusive_zone);
    }

    #[test]
    fn the_closing_click_does_not_reopen() {
        assert!(!may_reopen(1_000_000, 900_000));
        assert!(may_reopen(1_300_000, 1_000_000));
        assert!(may_reopen(REOPEN_GUARD_USEC, 0));
    }
}
