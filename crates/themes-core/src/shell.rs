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

    /// A stream of the signals that match `rule`, subscribed now (so nothing sent after this
    /// call is missed).
    fn signals(&self, rule: &str) -> Option<zbus::MessageStream> {
        zbus::block_on(zbus::MessageStream::for_match_rule(rule, &self.conn, None)).ok()
    }

    /// Чекає, доки `on` стануть ACTIVE (або ERROR/OUT_OF_DATE), а `off` перестануть бути ACTIVE.
    /// Перше ввімкнення розширення асинхронне: спершу INITIALIZED, потім ACTIVE. Waits on the
    /// shell's `ExtensionStateChanged` signal (no polling), at most `timeout`.
    pub fn settle_targets(
        &self,
        on: &[String],
        off: &[String],
        timeout: Duration,
    ) -> HashMap<String, Option<ExtInfo>> {
        self.wait_states(on.iter().chain(off).cloned().collect(), timeout, |infos| {
            let on_done = on.iter().all(|u| match &infos[u] {
                None => true, // невідоме оболонці — чекати марно
                Some(i) => i.state == ExtState::Active || i.state.broken(),
            });
            let off_done = off.iter().all(|u| {
                infos[u]
                    .as_ref()
                    .is_none_or(|i| i.state != ExtState::Active && !i.state.transitional())
            });
            on_done && off_done
        })
    }

    /// Чекає, доки розширення вийдуть із перехідних станів, і повертає їхні стани.
    pub fn settle(&self, uuids: &[String], timeout: Duration) -> HashMap<String, Option<ExtInfo>> {
        self.wait_states(uuids.to_vec(), timeout, |infos| {
            !infos
                .values()
                .any(|i| i.as_ref().is_some_and(|i| i.state.transitional()))
        })
    }

    /// Reads the states of `uuids` until `done` says so, re-reading on every
    /// `ExtensionStateChanged`; gives up at `timeout` and returns the last states.
    fn wait_states(
        &self,
        uuids: Vec<String>,
        timeout: Duration,
        done: impl Fn(&HashMap<String, Option<ExtInfo>>) -> bool,
    ) -> HashMap<String, Option<ExtInfo>> {
        let alarm = Alarm::new(Instant::now() + timeout);
        // subscribed before the first read: a change between the read and the wait is not lost
        let mut changes = self.signals(
            "type='signal',sender='org.gnome.Shell',interface='org.gnome.Shell.Extensions',\
             member='ExtensionStateChanged'",
        );
        loop {
            let infos: HashMap<String, Option<ExtInfo>> = uuids
                .iter()
                .map(|u| (u.clone(), self.ext_info(u)))
                .collect();
            if done(&infos) {
                return infos;
            }
            let Some(stream) = changes.as_mut() else {
                return infos; // no subscription: nothing to wait on
            };
            if alarm.run(next_message(stream)).flatten().is_none() {
                return infos;
            }
        }
    }

    /// Subscribes to dconf change notifications (`ca.desrt.dconf.Writer.Notify`). Call it before
    /// writing, then [`Shell::after_dconf_change`].
    pub fn watch_dconf(&self) -> Option<DconfWatch> {
        self.signals("type='signal',interface='ca.desrt.dconf.Writer',member='Notify'")
            .map(DconfWatch)
    }

    /// Waits until the shell has processed a dconf change of one of `keys`, at most `timeout`:
    /// first the writer's `Notify` covering a key (the bus has then queued it for the shell as
    /// well), then a call the shell answers from its main loop, which runs after the queued
    /// notification. False when the notification did not come in time.
    pub fn after_dconf_change(
        &self,
        watch: DconfWatch,
        keys: &[String],
        timeout: Duration,
    ) -> bool {
        let alarm = Alarm::new(Instant::now() + timeout);
        let mut stream = watch.0;
        let seen = loop {
            match alarm.run(next_message(&mut stream)).flatten() {
                Some(m) => {
                    let covers = m
                        .body()
                        .deserialize::<(String, Vec<String>, String)>()
                        .is_ok_and(|(prefix, changes, _)| notify_covers(&prefix, &changes, keys));
                    if covers {
                        break true;
                    }
                }
                None => break false,
            }
        };
        let _ = self.version();
        seen
    }
}

/// A subscription made by [`Shell::watch_dconf`].
pub struct DconfWatch(zbus::MessageStream);

/// Whether a dconf `Notify(prefix, changes)` covers one of `keys`: the changed paths are
/// `prefix + change` (a path ending in `/` is a directory), or `prefix` alone when `changes` is
/// empty.
fn notify_covers(prefix: &str, changes: &[String], keys: &[String]) -> bool {
    let paths: Vec<String> = if changes.is_empty() {
        vec![prefix.to_string()]
    } else {
        changes.iter().map(|c| format!("{prefix}{c}")).collect()
    };
    keys.iter().any(|k| {
        paths
            .iter()
            .any(|p| k == p || (p.ends_with('/') && k.starts_with(p.as_str())))
    })
}

/// The next message of a stream (`None` when it ended or failed).
async fn next_message(stream: &mut zbus::MessageStream) -> Option<zbus::Message> {
    use zbus::export::futures_core::Stream;
    std::future::poll_fn(|cx| std::pin::Pin::new(&mut *stream).poll_next(cx))
        .await
        .and_then(|r| r.ok())
}

/// A deadline that wakes whatever waits on it: one sleeping helper thread per deadline, so
/// waits on signals need no polling. [`Alarm::run`] can be called many times.
pub(crate) struct Alarm {
    inner: std::sync::Arc<AlarmInner>,
}

struct AlarmInner {
    fired: std::sync::atomic::AtomicBool,
    waker: std::sync::Mutex<Option<std::task::Waker>>,
    cancelled: std::sync::Mutex<bool>,
    cv: std::sync::Condvar,
}

impl Drop for Alarm {
    fn drop(&mut self) {
        let mut g = self
            .inner
            .cancelled
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *g = true;
        self.inner.cv.notify_one();
    }
}

impl Alarm {
    pub(crate) fn new(deadline: Instant) -> Alarm {
        use std::sync::atomic::Ordering;
        let inner = std::sync::Arc::new(AlarmInner {
            fired: std::sync::atomic::AtomicBool::new(false),
            waker: std::sync::Mutex::new(None),
            cancelled: std::sync::Mutex::new(false),
            cv: std::sync::Condvar::new(),
        });
        let i = inner.clone();
        std::thread::spawn(move || {
            let mut g = i.cancelled.lock().unwrap_or_else(|p| p.into_inner());
            while !*g {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                g =
                    i.cv.wait_timeout(g, left)
                        .unwrap_or_else(|p| p.into_inner())
                        .0;
            }
            if *g {
                return;
            }
            drop(g);
            i.fired.store(true, Ordering::SeqCst);
            if let Some(w) = i.waker.lock().unwrap_or_else(|p| p.into_inner()).take() {
                w.wake();
            }
        });
        Alarm { inner }
    }

    /// Runs `fut` to completion or until the deadline, whichever comes first (`None`: time is
    /// up).
    pub(crate) fn run<T>(&self, fut: impl std::future::Future<Output = T>) -> Option<T> {
        use std::sync::atomic::Ordering;
        use std::task::Poll;
        let mut fut = std::pin::pin!(fut);
        zbus::block_on(std::future::poll_fn(|cx| {
            if let Poll::Ready(v) = fut.as_mut().poll(cx) {
                return Poll::Ready(Some(v));
            }
            *self.inner.waker.lock().unwrap_or_else(|p| p.into_inner()) = Some(cx.waker().clone());
            if self.inner.fired.load(Ordering::SeqCst) {
                return Poll::Ready(None);
            }
            Poll::Pending
        }))
    }
}

pub(crate) fn session() -> Option<Connection> {
    zbus::block_on(async {
        match crate::util::fallback_bus_address() {
            Some(a) => zbus::connection::Builder::address(a.as_str()).build().await,
            None => Connection::session().await,
        }
    })
    .ok()
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
    // the extension must see the empty name before the real one comes back
    let watch = shell.watch_dconf();
    let _ = crate::dconf::write(key, "''");
    if let Some(w) = watch {
        shell.after_dconf_change(w, &[key.to_string()], Duration::from_secs(2));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dconf_notify_paths() {
        let keys = vec!["/org/gnome/desktop/interface/clock-format".to_string()];
        assert!(notify_covers(
            "/org/gnome/desktop/interface/clock-format",
            &[],
            &keys
        ));
        assert!(notify_covers(
            "/org/gnome/desktop/",
            &["interface/clock-format".into()],
            &keys
        ));
        assert!(notify_covers("/org/gnome/", &["desktop/".into()], &keys));
        assert!(!notify_covers(
            "/org/gnome/",
            &["shell/x".into(), "desktop/y".into()],
            &keys
        ));
    }

    #[test]
    fn alarm_returns_ready_values_and_gives_up_at_the_deadline() {
        let later = Alarm::new(Instant::now() + Duration::from_secs(3600));
        assert_eq!(later.run(async { 7 }), Some(7));
        assert_eq!(later.run(async { 8 }), Some(8));
        // a future that never completes, with a deadline already passed
        let passed = Alarm::new(Instant::now());
        assert_eq!(passed.run(std::future::pending::<()>()), None);
    }
}
