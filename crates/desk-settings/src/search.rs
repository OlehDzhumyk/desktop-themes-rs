// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings of the own search engine (stage 4, `docs/spec/features/search.md`): schema
//! `io.github.OlehDzhumyk.Desk.Search`, its shortcut (`search-shortcut`) and its page.

use crate::model::*;
use crate::registry::SHORTCUTS_SCHEMA;

pub const SEARCH_SCHEMA: &str = "io.github.OlehDzhumyk.Desk.Search";

pub static FEATURE_SEARCH: Feature = Feature {
    id: "search",
    title: n_("Search"),
    origins: &[Origin::Has(n_("In Windows 11 and macOS"))],
};

pub static SEARCH_ENABLED: Key = Key {
    schema: SEARCH_SCHEMA,
    name: "enabled",
    kind: Kind::Bool { default: true },
    label: n_("Search"),
    description: n_("Find applications, system actions and calculations from one search box"),
    summary: "Enable the own search engine",
    doc: "Off: deskd refuses search calls and the bridge binds no search shortcut.",
    feature: "search",
    widget: Widget::Edit,
    sensitive_if: None,
};

pub static SEARCH_WEB: Key = Key {
    schema: SEARCH_SCHEMA,
    name: "web-search",
    kind: Kind::Bool { default: false },
    label: n_("Offer web search"),
    description: n_("Add a result that opens the query in your browser"),
    summary: "Offer a result that opens the query in the web browser",
    doc: "Off by default (AC-SRCH-06). Even when on, deskd makes no network request: activating \
          the result only opens the default browser with the query.",
    feature: "search",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static SEARCH_MAX_RESULTS: Key = Key {
    schema: SEARCH_SCHEMA,
    name: "max-results",
    kind: Kind::UInt {
        default: 12,
        min: 5,
        max: 50,
        step: 1,
    },
    label: n_("Maximum results"),
    description: n_("How many results the search box lists at most"),
    summary: "The largest number of results of one query",
    doc: "From 5 to 50. A caller may ask for fewer in its Query call.",
    feature: "search",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static SEARCH_REMEMBER_USAGE: Key = Key {
    schema: SEARCH_SCHEMA,
    name: "remember-usage",
    kind: Kind::Bool { default: true },
    label: n_("Rank by usage"),
    description: n_("Remember what you open so that it ranks higher; stays on this computer"),
    summary: "Remember launched results to rank them higher",
    doc: "Kept in ~/.local/share/deskd/search-usage.json (mode 0600). Off: nothing is recorded \
          and the file is deleted (AC-SRCH-07).",
    feature: "search",
    widget: Widget::Edit,
    sensitive_if: Some("enabled"),
};

pub static SEARCH_SHORTCUT: Key = Key {
    schema: SEARCH_SCHEMA,
    name: "search-shortcut",
    kind: Kind::Accel { default: &[] },
    label: n_("Keyboard shortcut"),
    description: "",
    summary: "Shortcut that opens the search box (in force)",
    doc: "The accelerators the bridge binds. Empty by default, so it does not clash with the \
          launcher of the macOS look (spotlight@nin) or the overview until the own launcher \
          ships. An override in io.github.OlehDzhumyk.Desk.Shortcuts search-toggle replaces it. \
          The name ends in -shortcut because shell keybinding names are global.",
    feature: "search",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SC_SEARCH_TOGGLE: Key = Key {
    schema: SHORTCUTS_SCHEMA,
    name: "search-toggle",
    kind: Kind::Accel { default: &[] },
    label: n_("Open the search box"),
    description: "",
    summary: "Your shortcut for the search box (override)",
    doc: "Empty: follow the active theme. ['disabled']: no shortcut. Anything else wins over the \
          theme default and is kept when the theme changes.",
    feature: "search",
    widget: Widget::Hidden,
    sensitive_if: None,
};

pub static SHORTCUT_SEARCH: Shortcut = Shortcut {
    id: "search-toggle",
    label: n_("Open the search box"),
    feature: "search",
    key: &SEARCH_SHORTCUT,
    override_key: &SC_SEARCH_TOGGLE,
};

pub static SEARCH: Schema = Schema {
    id: SEARCH_SCHEMA,
    path: "/io/github/OlehDzhumyk/Desk/search/",
    rust_name: "SearchSettings",
    keys: &[
        &SEARCH_ENABLED,
        &SEARCH_SHORTCUT,
        &SEARCH_WEB,
        &SEARCH_MAX_RESULTS,
        &SEARCH_REMEMBER_USAGE,
    ],
    children: &[],
};

pub static PAGE_SEARCH: Page = Page {
    id: "search",
    title: n_("Search"),
    subtitle: n_("Find applications, actions and calculations"),
    description: n_(
        "One search box finds applications, system actions and calculations, tolerates typos \
         and a wrong keyboard layout, and ranks what you use more often higher. Queries never \
         leave this computer.",
    ),
    icon: "system-search-symbolic",
    section: Section::Features,
    kind: PageKind::Settings,
    feature: Some(&FEATURE_SEARCH),
    groups: &[
        Group {
            title: n_("Search box"),
            description: "",
            items: &[Item::Key(&SEARCH_ENABLED), Item::Key(&SEARCH_MAX_RESULTS)],
        },
        Group {
            title: n_("Privacy"),
            description: "",
            items: &[Item::Key(&SEARCH_REMEMBER_USAGE), Item::Key(&SEARCH_WEB)],
        },
        Group {
            title: n_("How to open"),
            description: n_("The shortcut is set on the «Keyboard shortcuts» page."),
            items: &[Item::Key(&crate::registry::SKIN)],
        },
    ],
};
