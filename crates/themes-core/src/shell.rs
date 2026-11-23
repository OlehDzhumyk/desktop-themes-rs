// SPDX-License-Identifier: GPL-3.0-or-later
//! Зв'язок із GNOME Shell через D-Bus (org.gnome.Shell.Extensions), сповіщення.

use desk_settings::tr;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use zbus::{Connection, OwnedVariant};

use crate::error::Result;

#[zbus::proxy(
    interface = "org.gnome.Shell.Extensions",
    default_service = "org.gnome.Shell",
    default_path = "/org/gnome/Shell"
)]
trait ShellExtensions {
    async fn get_extension_info(&self, uuid: &str) -> zbus::Result<HashMap<String, OwnedVariant>>;
    async fn get_extension_errors(&self, uuid: &str) -> zbus::Result<Vec<String>>;
    async fn enable_extension(&self, uuid: &str) -> zbus::Result<bool>;
    async fn disable_extension(&self, uuid: &str) -> zbus::Result<bool>;
    #[zbus(property)]
    fn shell_version(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn user_extensions_enabled(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, OwnedVariant>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Стани розширень GNOME Shell (js/misc/extensionUtils.js).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtState {
    Active,
    Inactive,
    Error,
    OutOfDate,
    Downloading,
    Initialized,
    Deactivating,
    Activating,
    Uninstalled,
    Unknown(i32),
}

impl ExtState {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a small state number from D-Bus; the float to int cast saturates"
    )]
    pub fn from_f64(v: f64) -> ExtState {
        match v as i32 {
            1 => ExtState::Active,
            2 => ExtState::Inactive,
            3 => ExtState::Error,
            4 => ExtState::OutOfDate,
            5 => ExtState::Downloading,
            6 => ExtState::Initialized,
            7 => ExtState::Deactivating,
            8 => ExtState::Activating,
            99 => ExtState::Uninstalled,
            n => ExtState::Unknown(n),
        }
    }
    pub fn name(self) -> String {
        match self {
            ExtState::Active => "ACTIVE".into(),
            ExtState::Inactive => "INACTIVE".into(),
            ExtState::Error => "ERROR".into(),
            ExtState::OutOfDate => "OUT_OF_DATE".into(),
            ExtState::Downloading => "DOWNLOADING".into(),
            ExtState::Initialized => "INITIALIZED".into(),
            ExtState::Deactivating => "DEACTIVATING".into(),
            ExtState::Activating => "ACTIVATING".into(),
            ExtState::Uninstalled => "UNINSTALLED".into(),
            ExtState::Unknown(n) => format!("STATE_{n}"),
        }
    }
    pub fn transitional(self) -> bool {
        matches!(
            self,
            ExtState::Downloading | ExtState::Deactivating | ExtState::Activating
        )
    }
    pub fn broken(self) -> bool {
        matches!(self, ExtState::Error | ExtState::OutOfDate)
    }
}

#[derive(Clone, Debug)]
pub struct ExtInfo {
    pub state: ExtState,
    pub error: String,
    pub version: String,
}

fn var_str(m: &HashMap<String, OwnedVariant>, k: &str) -> String {
    m.get(k)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| String::try_from(v).ok())
        .unwrap_or_default()
}

/// Синхронна обгортка (кожен виклик — `zbus::block_on`).
#[derive(Clone)]
pub struct Shell {
    conn: Connection,
}

impl Shell {
    /// Підключення до сесійної шини, лише якщо GNOME Shell зараз запущено.
    pub fn connect() -> Option<Shell> {
        let conn = session()?;
        let running = zbus::block_on(async {
            let dbus = zbus::fdo::DBusProxy::new(&conn).await.ok()?;
            dbus.name_has_owner("org.gnome.Shell".try_into().ok()?)
                .await
                .ok()
        })
        .unwrap_or(false);
        running.then_some(Shell { conn })
    }

    /// Чекати на запуск оболонки до `timeout`.
    pub fn wait(timeout: Duration) -> Option<Shell> {
        let start = Instant::now();
        loop {
            if let Some(s) = Shell::connect()
                && s.version().is_some()
            {
                return Some(s);
            }
            if start.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    fn proxy(&self) -> Option<ShellExtensionsProxy<'static>> {
        zbus::block_on(ShellExtensionsProxy::new(&self.conn)).ok()
    }

    pub fn version(&self) -> Option<String> {
        let p = self.proxy()?;
        zbus::block_on(p.shell_version()).ok()
    }

    /// Екран заблоковано (org.gnome.ScreenSaver): у цьому режимі більшість розширень неактивні.
    pub fn screen_locked(&self) -> bool {
        zbus::block_on(async {
            let m = self
                .conn
                .call_method(
                    Some("org.gnome.ScreenSaver"),
                    "/org/gnome/ScreenSaver",
                    Some("org.gnome.ScreenSaver"),
                    "GetActive",
                    &(),
                )
                .await
                .ok()?;
            m.body().deserialize::<bool>().ok()
        })
        .unwrap_or(false)
    }

    pub fn user_extensions_enabled(&self) -> Option<bool> {
        let p = self.proxy()?;
        zbus::block_on(p.user_extensions_enabled()).ok()
    }

    /// None — оболонка не знає про таке розширення (не встановлене або ще не проскановане).
    pub fn ext_info(&self, uuid: &str) -> Option<ExtInfo> {
        let p = self.proxy()?;
        let m = zbus::block_on(p.get_extension_info(uuid)).ok()?;
        if m.is_empty() {
            return None;
        }
        let state = m
            .get("state")
            .and_then(|v| f64::try_from(v).ok())
            .map_or(ExtState::Unknown(-1), ExtState::from_f64);
        let version = m
            .get("version")
            .and_then(|v| f64::try_from(v).ok().map(|f| format!("{f}")))
            .or_else(|| Some(var_str(&m, "version-name")))
            .unwrap_or_default();
        Some(ExtInfo {
            state,
            error: var_str(&m, "error"),
            version,
        })
    }

    pub fn ext_errors(&self, uuid: &str) -> Vec<String> {
        self.proxy()
            .and_then(|p| zbus::block_on(p.get_extension_errors(uuid)).ok())
            .unwrap_or_default()
    }

    pub fn enable(&self, uuid: &str) -> bool {
        self.proxy()
            .and_then(|p| zbus::block_on(p.enable_extension(uuid)).ok())
            .unwrap_or(false)
    }

    pub fn disable(&self, uuid: &str) -> bool {
        self.proxy()
            .and_then(|p| zbus::block_on(p.disable_extension(uuid)).ok())
            .unwrap_or(false)
    }

    /// Чекає, доки `on` стануть ACTIVE (або ERROR/OUT_OF_DATE), а `off` перестануть бути ACTIVE.
    /// Перше ввімкнення розширення асинхронне: спершу INITIALIZED, потім ACTIVE.
    pub fn settle_targets(
        &self,
        on: &[String],
        off: &[String],
        timeout: Duration,
    ) -> HashMap<String, Option<ExtInfo>> {
        let start = Instant::now();
        loop {
            let infos: HashMap<String, Option<ExtInfo>> = on
                .iter()
                .chain(off)
                .map(|u| (u.clone(), self.ext_info(u)))
                .collect();
            let on_done = on.iter().all(|u| match &infos[u] {
                None => true, // невідоме оболонці — чекати марно
                Some(i) => i.state == ExtState::Active || i.state.broken(),
            });
            let off_done = off.iter().all(|u| {
                infos[u]
                    .as_ref()
                    .is_none_or(|i| i.state != ExtState::Active && !i.state.transitional())
            });
            if (on_done && off_done) || start.elapsed() >= timeout {
                return infos;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Чекає, доки розширення вийдуть із перехідних станів, і повертає їхні стани.
    pub fn settle(&self, uuids: &[String], timeout: Duration) -> HashMap<String, Option<ExtInfo>> {
        let start = Instant::now();
        loop {
            let infos: HashMap<String, Option<ExtInfo>> = uuids
                .iter()
                .map(|u| (u.clone(), self.ext_info(u)))
                .collect();
            let busy = infos
                .values()
                .any(|i| i.as_ref().is_some_and(|i| i.state.transitional()));
            if !busy || start.elapsed() >= timeout {
                return infos;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

pub(crate) fn session() -> Option<Connection> {
    crate::util::ensure_bus();
    zbus::block_on(Connection::session()).ok()
}

/// Перезавантажити тему оболонки: user-theme стежить за своїм ключем `name` і перечитує
/// таблицю стилів сам; якщо ключ не змінився, але файли теми оновилися, «перемикаємо» його.
pub fn reload_user_theme(shell: &Shell, name: &str) -> bool {
    let uuid = "user-theme@gnome-shell-extensions.gcampax.github.com";
    if shell.ext_info(uuid).map(|i| i.state) != Some(ExtState::Active) {
        return false;
    }
    let key = crate::SHELL_THEME_KEY;
    let val = crate::dconf::read(key);
    let _ = crate::dconf::write(key, "''");
    std::thread::sleep(Duration::from_millis(150));
    match val {
        Some(v) => crate::dconf::write(key, &v).is_ok(),
        None => {
            crate::dconf::reset(&[key.to_string()]);
            let _ = name;
            true
        }
    }
}

/// Сповіщення на робочому столі (тихо ігнорує помилки).
pub fn notify(summary: &str, body: &str, icon: &str) -> Result<()> {
    let Some(conn) = session() else { return Ok(()) };
    zbus::block_on(async {
        let p = NotificationsProxy::new(&conn).await?;
        // без підказки desktop-entry: із нею GNOME Shell 51 у тестах не показував банер
        let hints: HashMap<&str, OwnedVariant> = HashMap::new();
        p.notify(&tr!("Themes"), 0, icon, summary, body, &[], hints, -1)
            .await
    })?;
    Ok(())
}
