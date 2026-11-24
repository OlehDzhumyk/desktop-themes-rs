// SPDX-License-Identifier: GPL-3.0-or-later
//! The system actions provider (`> text`). The core only lists and ranks them; the adapter
//! performs one by its id (the GNOME bridge calls the session manager, the screen saver or the
//! settings).

use crate::n_;

/// One built-in action. `title` and `keywords` are English gettext ids (marked with [`n_`]);
/// `keywords_uk` are Ukrainian search words, so a Ukrainian query finds the action even when the
/// interface is English (never shown, hence `i18n-ok`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemAction {
    /// Stable id (`lock`, `suspend`, ...); the result id is `action:<id>`.
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    pub keywords: &'static [&'static str],
    pub keywords_uk: &'static [&'static str],
}

/// The built-in actions, in the order shown for an empty `>` query.
pub static SYSTEM_ACTIONS: &[SystemAction] = &[
    SystemAction {
        id: "lock",
        title: n_("Lock screen"),
        icon: "system-lock-screen-symbolic",
        keywords: &["lock", "screen", "away"],
        keywords_uk: &["заблокувати", "блокування", "екран"], // i18n-ok
    },
    SystemAction {
        id: "suspend",
        title: n_("Suspend"),
        icon: "media-playback-pause-symbolic",
        keywords: &["sleep", "suspend", "standby"],
        keywords_uk: &["сон", "призупинити", "режим сну"], // i18n-ok
    },
    SystemAction {
        id: "logout",
        title: n_("Log out"),
        icon: "system-log-out-symbolic",
        keywords: &["log out", "logout", "sign out", "exit"],
        keywords_uk: &["вийти", "вихід", "завершити сеанс"], // i18n-ok
    },
    SystemAction {
        id: "restart",
        title: n_("Restart"),
        icon: "system-reboot-symbolic",
        keywords: &["restart", "reboot"],
        keywords_uk: &["перезапустити", "перезавантажити", "перезавантаження"], // i18n-ok
    },
    SystemAction {
        id: "power-off",
        title: n_("Power off"),
        icon: "system-shutdown-symbolic",
        keywords: &["power off", "shutdown", "shut down", "turn off"],
        keywords_uk: &["вимкнути", "вимкнення", "завершення роботи"], // i18n-ok
    },
    SystemAction {
        id: "dark-mode",
        title: n_("Toggle dark mode"),
        icon: "weather-clear-night-symbolic",
        keywords: &["dark", "light", "theme", "night", "appearance"],
        keywords_uk: &["темний", "світлий", "тема", "нічний", "вигляд"], // i18n-ok
    },
    SystemAction {
        id: "clipboard-private",
        title: n_("Toggle clipboard private mode"),
        icon: "edit-paste-symbolic",
        keywords: &["clipboard", "private", "incognito", "history", "pause"],
        keywords_uk: &["буфер обміну", "приватний", "історія", "пауза"], // i18n-ok
    },
];

/// An action by its id.
pub fn action(id: &str) -> Option<&'static SystemAction> {
    SYSTEM_ACTIONS.iter().find(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_known() {
        let ids: Vec<&str> = SYSTEM_ACTIONS.iter().map(|a| a.id).collect();
        assert_eq!(
            ids,
            [
                "lock",
                "suspend",
                "logout",
                "restart",
                "power-off",
                "dark-mode",
                "clipboard-private"
            ]
        );
        assert_eq!(action("lock").unwrap().title, "Lock screen");
        assert!(action("rm").is_none());
        for a in SYSTEM_ACTIONS {
            assert!(
                !a.keywords.is_empty() && !a.keywords_uk.is_empty(),
                "{}",
                a.id
            );
            assert!(a.icon.ends_with("-symbolic"), "{}", a.id);
        }
    }
}
