// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the logo menu (macOS skin): schema `io.github.OlehDzhumyk.Desk.LogoMenu` and
//! its page, declared in the registry like the others and kept in their own module so that
//! features added in parallel do not collide.

use crate::model::*;

pub const LOGO_MENU_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.LogoMenu";

pub static FEATURE_LOGO_MENU: Feature = Feature {
    id: "logo-menu",
    title: n_("Logo menu"),
    origins: &[Origin::Has(n_("From macOS"))],
};

const fn logo_menu_bool(
    name: &'static str,
    default: bool,
    label: &'static str,
    description: &'static str,
    summary: &'static str,
) -> Key {
    Key {
        schema: LOGO_MENU_SCHEMA,
        name,
        kind: Kind::Bool { default },
        label,
        description,
        summary,
        doc: "",
        feature: "logo-menu",
        widget: Widget::Edit,
        sensitive_if: None,
    }
}

pub static LOGO_MENU_ENABLED: Key = logo_menu_bool(
    "enabled",
    true,
    n_("Show logo menu"),
    n_("Show a system menu on the left of the top bar in the macOS skin."),
    "Show the logo menu on the left of the top bar in the macOS skin",
);

pub static LOGO_MENU_HIDE_ACTIVITIES: Key = Key {
    sensitive_if: Some("enabled"),
    ..logo_menu_bool(
        "hide-activities",
        true,
        n_("Hide Activities button"),
        n_("Hide the default Activities indicator while the logo menu is shown."),
        "Hide the default Activities indicator while the logo menu is shown",
    )
};

pub static LOGO_MENU: Schema = Schema {
    id: LOGO_MENU_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/logo-menu/",
    rust_name: "LogoMenuSettings",
    keys: &[&LOGO_MENU_ENABLED, &LOGO_MENU_HIDE_ACTIVITIES],
    children: &[],
};

pub static PAGE_LOGO_MENU: Page = Page {
    id: "logo-menu",
    title: n_("Logo menu"),
    subtitle: n_("System menu on the left of the top bar in the macOS skin"),
    description: n_(
        "A macOS-style system menu on the left of the top bar with quick actions for About This \
         System, System Settings, Applications, Force Quit and power controls. Drawn by the shell \
         bridge when the macOS skin is active.",
    ),
    icon: "start-here-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_LOGO_MENU),
    groups: &[Group {
        title: n_("Top bar"),
        description: "",
        items: &[
            Item::Key(&LOGO_MENU_ENABLED),
            Item::Key(&LOGO_MENU_HIDE_ACTIVITIES),
        ],
    }],
};
