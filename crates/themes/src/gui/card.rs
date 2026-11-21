// SPDX-License-Identifier: GPL-3.0-or-later
//! Картка теми.

use adw::prelude::*;
use gtk::{gdk, gio};
use themes_core::modules::Module;

#[derive(Clone, Debug)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub preview: String,
    pub preview_dark: String,
    pub builtin: bool,
    pub modules: Vec<Module>,
    pub active_modules: Vec<Module>,
    pub extensions: usize,
}

impl ThemeInfo {
    pub fn fully_active(&self) -> bool {
        self.active_modules.len() == Module::ALL.len()
    }
}

#[derive(Clone)]
pub struct ThemeCard {
    pub root: gtk::Box,
    pub info: ThemeInfo,
    picture: gtk::Picture,
    stack: gtk::Stack,
    veil: gtk::Box,
}

impl ThemeCard {
    pub fn new(info: ThemeInfo) -> ThemeCard {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["card", "theme-card"])
            .width_request(320)
            .build();
        if info.fully_active() {
            root.add_css_class("active");
        }
        let overlay = gtk::Overlay::builder()
            .overflow(gtk::Overflow::Hidden)
            .css_classes(["preview"])
            .build();
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .height_request(180)
            .width_request(320)
            .build();
        let placeholder = gtk::Image::builder()
            .icon_name("preferences-desktop-appearance-symbolic")
            .pixel_size(64)
            .opacity(0.4)
            .height_request(180)
            .build();
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&placeholder, Some("placeholder"));
        stack.add_named(&picture, Some("picture"));
        overlay.set_child(Some(&stack));

        // значки активних модулів
        if !info.active_modules.is_empty() {
            let badges = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(4)
                .halign(gtk::Align::End)
                .valign(gtk::Align::Start)
                .css_classes(["badges"])
                .build();
            if info.fully_active() {
                badges.append(
                    &gtk::Label::builder()
                        .label("активна")
                        .css_classes(["badge"])
                        .build(),
                );
            } else {
                for m in &info.active_modules {
                    badges.append(
                        &gtk::Label::builder()
                            .label(m.label())
                            .css_classes(["badge", "partial"])
                            .tooltip_text(format!("Модуль «{}» узято з цієї теми", m.label()))
                            .build(),
                    );
                }
            }
            overlay.add_overlay(&badges);
        }
        let veil = gtk::Box::builder()
            .css_classes(["busy-veil"])
            .visible(false)
            .build();
        veil.append(
            &adw::Spinner::builder()
                .width_request(48)
                .height_request(48)
                .halign(gtk::Align::Center)
                .valign(gtk::Align::Center)
                .hexpand(true)
                .vexpand(true)
                .build(),
        );
        overlay.add_overlay(&veil);
        root.append(&overlay);

        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .css_classes(["info"])
            .vexpand(true)
            .build();
        let texts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .build();
        texts.append(
            &gtk::Label::builder()
                .label(&info.name)
                .xalign(0.0)
                .css_classes(["heading"])
                .max_width_chars(22)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build(),
        );
        if !info.description.is_empty() {
            texts.append(
                &gtk::Label::builder()
                    .label(&info.description)
                    .xalign(0.0)
                    .wrap(true)
                    .lines(2)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .css_classes(["dim-label", "caption"])
                    .max_width_chars(26)
                    .width_chars(20)
                    .tooltip_text(&info.description)
                    .build(),
            );
        }
        let mods: Vec<String> = info
            .modules
            .iter()
            .map(|m| m.label().to_lowercase())
            .collect();
        let mut meta = if mods.is_empty() {
            "Модулі: —".to_string()
        } else {
            format!("Модулі: {}", mods.join(" · "))
        };
        if info.extensions > 0 {
            meta.push_str(&format!("   Розширень: {}", info.extensions));
        }
        texts.append(
            &gtk::Label::builder()
                .label(meta)
                .xalign(0.0)
                .css_classes(["dim-label", "caption", "module-chip"])
                .build(),
        );
        row.append(&texts);

        let menu = gio::Menu::new();
        let apply_sec = gio::Menu::new();
        for m in &info.modules {
            apply_sec.append(
                Some(&format!("Застосувати лише «{}»", m.label())),
                Some(&format!("win.apply-module(('{}','{}'))", info.id, m.id())),
            );
        }
        menu.append_section(None, &apply_sec);
        if !info.builtin && info.active_modules.is_empty() {
            let del = gio::Menu::new();
            del.append(
                Some("Видалити тему…"),
                Some(&format!("win.delete-theme('{}')", info.id)),
            );
            menu.append_section(None, &del);
        }
        let mb = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&menu)
            .valign(gtk::Align::Start)
            .css_classes(["flat", "circular"])
            .tooltip_text("Дії з темою")
            .build();
        row.append(&mb);
        let rc = gtk::GestureClick::builder()
            .button(gdk::BUTTON_SECONDARY)
            .build();
        {
            let mb = mb.clone();
            rc.connect_pressed(move |_, _, _, _| mb.popup());
        }
        root.add_controller(rc);
        root.append(&row);
        root.set_tooltip_text(Some(if info.fully_active() {
            "Активна тема"
        } else {
            "Натисніть, щоб застосувати всю тему"
        }));
        ThemeCard {
            root,
            info,
            picture,
            stack,
            veil,
        }
    }

    pub async fn load_preview(&self, dark: bool) {
        let path = if dark && !self.info.preview_dark.is_empty() {
            &self.info.preview_dark
        } else {
            &self.info.preview
        };
        if let Some(tex) = super::preview::load(path.clone(), 360).await {
            self.picture.set_paintable(Some(&tex));
            self.stack.set_visible_child_name("picture");
        }
    }

    pub fn set_busy(&self, b: bool) {
        self.veil.set_visible(b);
    }
}
