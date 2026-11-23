// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus-служба io.github.OlehDzhumyk.Themes (активується шиною, виходить після простою).
//!
//! The interface is `data/dbus/io.github.OlehDzhumyk.Themes.xml` (the contract test below keeps
//! the two equal; the bridge reads the same file). Library calls block (dconf, files, the shell),
//! so they run on a blocking pool (`zruntime::unblock`) and the zbus executor keeps serving other
//! calls. The process exits when it has been idle for [`IDLE_EXIT`] with no operation running, or
//! at once when the bus connection closes. A file monitor on `state.json` emits `Changed` for
//! applies made by the CLI or the window.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use desk_settings::tr;
use gtk::gio::prelude::*;
use gtk::{gio, glib};
use themes_core::modules::Module;
use themes_core::state::State;
use themes_core::theme::{all_themes, get_theme};
use themes_core::{Ctx, DBUS_NAME, DBUS_PATH};
use zbus::object_server::SignalEmitter;

const IDLE_EXIT: u64 = 90;

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// When the service was last used and how many operations run now (applies, the automatic
/// rollback check, doctor); it exits only when idle and none runs.
#[derive(Clone, Default)]
struct Activity {
    last: Arc<AtomicU64>,
    running: Arc<AtomicUsize>,
    /// The module map last announced with `Changed` (the file monitor announces only changes).
    announced: Arc<Mutex<Option<HashMap<String, String>>>>,
}

/// One running operation; dropping it ends the operation (also on an error or a panic).
struct Busy(Activity);

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::SeqCst);
        self.0.touch();
    }
}

impl Activity {
    fn touch(&self) {
        self.last.store(now_s(), Ordering::SeqCst);
    }
    fn busy(&self) -> Busy {
        self.running.fetch_add(1, Ordering::SeqCst);
        self.touch();
        Busy(self.clone())
    }
    fn idle_for(&self) -> u64 {
        now_s().saturating_sub(self.last.load(Ordering::SeqCst))
    }
    /// Remembers `map` as announced; true when it differs from the last one.
    fn announce(&self, map: &HashMap<String, String>) -> bool {
        let mut a = self.announced.lock().unwrap_or_else(|p| p.into_inner());
        if a.as_ref() == Some(map) {
            return false;
        }
        *a = Some(map.clone());
        true
    }
}

struct Service {
    ctx: Ctx,
    act: Activity,
}

pub fn current_map(ctx: &Ctx) -> HashMap<String, String> {
    let st = State::load(ctx);
    Module::ALL
        .iter()
        .map(|m| (m.id().to_string(), st.source(*m).unwrap_or("").to_string()))
        .collect()
}

/// Runs blocking library work on the blocking pool and awaits it without blocking the zbus
/// executor. A panic of the work becomes a D-Bus error instead of taking the service down.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> zbus::fdo::Result<T> {
    zruntime::unblock(move || std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)))
        .await
        .map_err(|_| zbus::fdo::Error::Failed(tr!("internal error of the theme service")))
}

impl Service {
    async fn apply_impl(
        &self,
        theme: String,
        modules: Vec<String>,
        confirmed: bool,
        emitter: &SignalEmitter<'_>,
    ) -> zbus::fdo::Result<(bool, String, Vec<String>)> {
        let busy = self.act.busy();
        let mods = Module::parse_list(&modules.join(","))
            .map_err(|e| zbus::fdo::Error::InvalidArgs(e.0))?;
        let ctx = self.ctx.clone();
        let res = blocking(move || {
            // ADR 0006: the plain method never applies an experimental theme; the library
            // refuses it as well, this only gives callers a distinct error
            if !confirmed
                && let Ok(t) = get_theme(&ctx, &theme)
                && t.experimental()
            {
                return Err(zbus::fdo::Error::AccessDenied(
                    themes_core::apply::experimental_refusal(&t),
                ));
            }
            let opts = themes_core::apply::Options {
                confirmed,
                ..Default::default()
            };
            themes_core::apply::apply(&ctx, &theme, &mods, &opts)
                .map_err(|e| zbus::fdo::Error::Failed(e.0))
        })
        .await??;
        // the state file monitor may have announced it already
        let map = current_map(&self.ctx);
        if self.act.announce(&map) {
            let _ = Service::changed(emitter, map).await;
        }
        // автовідкат через ~5 с, поза викликом (сигнал Changed, якщо щось вимкнено)
        if res.shell_running && res.extensions_changed {
            let ctx = self.ctx.clone();
            let check = self.act.busy();
            let conn = emitter.connection().clone();
            let act = self.act.clone();
            std::thread::spawn(move || {
                let _check = check;
                if let Ok(bad) = themes_core::apply::verify_live(&ctx, Duration::from_secs(5))
                    && !bad.is_empty()
                {
                    let list = bad
                        .iter()
                        .map(|b| b.0.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let _ = themes_core::shell::notify(
                        &tr!("Themes: extensions turned off"),
                        &tr!(
                            "After the theme was applied these extensions went into an error state and were turned off: {list}",
                            list = list
                        ),
                        "dialog-warning-symbolic",
                    );
                    emit_changed(&conn, &ctx, &act, true);
                }
            });
        }
        drop(busy);
        let mut msg = tr!("Theme “{name}” applied", name = res.name);
        if res.relogin {
            msg.push_str(". ");
            msg.push_str(&tr!(
                "You need to log in again: {reasons}",
                reasons = res.relogin_reasons.join("; ")
            ));
        }
        Ok((res.relogin, msg, res.warnings))
    }
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Themes")]
impl Service {
    /// (id, назва, опис, модулі, які тема містить)
    async fn list(&self) -> zbus::fdo::Result<Vec<(String, String, String, Vec<String>)>> {
        self.act.touch();
        let ctx = self.ctx.clone();
        blocking(move || {
            all_themes(&ctx)
                .iter()
                .map(|t| {
                    (
                        t.id.clone(),
                        t.name(),
                        t.description(),
                        t.modules().iter().map(|m| m.id().to_string()).collect(),
                    )
                })
                .collect()
        })
        .await
    }

    /// Експериментальні теми: id → попередження (мовою служби). `Apply` такої теми відмовляє;
    /// клієнт питає користувача й викликає `ApplyConfirmed`.
    async fn experimental(&self) -> zbus::fdo::Result<HashMap<String, String>> {
        self.act.touch();
        let ctx = self.ctx.clone();
        blocking(move || {
            all_themes(&ctx)
                .iter()
                .filter(|t| t.experimental())
                .map(|t| (t.id.clone(), t.warning()))
                .collect()
        })
        .await
    }

    /// модуль → id теми
    async fn current(&self) -> zbus::fdo::Result<HashMap<String, String>> {
        self.act.touch();
        let ctx = self.ctx.clone();
        blocking(move || current_map(&ctx)).await
    }

    /// Застосувати тему (modules порожній = усі модулі). Повертає (relogin, повідомлення,
    /// попередження). Експериментальну тему відхиляє (AccessDenied): див. `ApplyConfirmed`.
    #[zbus(out_args("relogin", "message", "warnings"))]
    async fn apply(
        &self,
        theme: String,
        modules: Vec<String>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<(bool, String, Vec<String>)> {
        self.apply_impl(theme, modules, false, &emitter).await
    }

    /// Як `Apply`, але користувач явно підтвердив експериментальну тему (ADR 0006).
    #[zbus(out_args("relogin", "message", "warnings"))]
    async fn apply_confirmed(
        &self,
        theme: String,
        modules: Vec<String>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<(bool, String, Vec<String>)> {
        self.apply_impl(theme, modules, true, &emitter).await
    }

    /// Перевірка системи. Повертає (усе гаразд, повідомлення, JSON-звіт).
    #[zbus(out_args("ok", "messages", "report"))]
    async fn doctor(&self, fix: bool) -> zbus::fdo::Result<(bool, Vec<String>, String)> {
        let _busy = self.act.busy();
        let ctx = self.ctx.clone();
        let rep = blocking(move || {
            themes_core::doctor::doctor(
                &ctx,
                &themes_core::doctor::DoctorOptions {
                    fix,
                    wait_shell: Duration::ZERO,
                    notify: fix,
                },
            )
        })
        .await?
        .map_err(|e| zbus::fdo::Error::Failed(e.0))?;
        let msgs = rep
            .issues
            .iter()
            .map(|i| format!("{}: {}", i.severity, i.message))
            .chain(rep.fixed.iter().cloned())
            .collect();
        Ok((
            rep.ok,
            msgs,
            serde_json::to_string(&rep).unwrap_or_default(),
        ))
    }

    #[zbus(signal)]
    async fn changed(
        emitter: &SignalEmitter<'_>,
        modules: HashMap<String, String>,
    ) -> zbus::Result<()>;
}

/// Emits `Changed` with the current map (blocking; for threads and the main loop). `always`:
/// also when the map equals the last one announced.
fn emit_changed(conn: &zbus::Connection, ctx: &Ctx, act: &Activity, always: bool) {
    let map = current_map(ctx);
    if !act.announce(&map) && !always {
        return;
    }
    zbus::block_on(async {
        if let Ok(iface) = conn
            .object_server()
            .interface::<_, Service>(DBUS_PATH)
            .await
        {
            let _ = Service::changed(iface.signal_emitter(), map).await;
        }
    });
}

pub fn run() -> i32 {
    let ctx = Ctx::from_env();
    let act = Activity::default();
    act.touch();
    act.announce(&current_map(&ctx));
    let svc = Service {
        ctx: ctx.clone(),
        act: act.clone(),
    };
    let conn = match zbus::block_on(async {
        zbus::connection::Builder::session()
            .name(DBUS_NAME)
            .serve_at(DBUS_PATH, svc)
            .build()
            .await
    }) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "themes --dbus-service: {}",
                tr!("cannot own {name}: {error}", name = DBUS_NAME, error = e)
            );
            return 1;
        }
    };
    let ml = glib::MainLoop::new(None, false);
    // the bus went away (session ended, bus daemon restarted): nothing can reach us any more
    {
        let ml = ml.clone();
        let conn = conn.clone();
        std::thread::spawn(move || {
            zbus::block_on(conn.closed());
            ml.quit();
        });
    }
    // applies by the CLI or the window change state.json: announce them
    let monitor = gio::File::for_path(&ctx.state)
        .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE);
    if let Ok(m) = &monitor {
        let (conn, ctx, act) = (conn.clone(), ctx.clone(), act.clone());
        m.connect_changed(move |_, _, _, _| emit_changed(&conn, &ctx, &act, false));
    }
    {
        let (ml, act) = (ml.clone(), act.clone());
        glib::timeout_add_seconds(10, move || {
            if act.idle_for() >= IDLE_EXIT && act.running.load(Ordering::SeqCst) == 0 {
                ml.quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }
    ml.run();
    drop(monitor);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::object_server::Interface;

    /// The interface definition shared with the bridge.
    const THEMES_XML: &str = include_str!("../../../data/dbus/io.github.OlehDzhumyk.Themes.xml");

    /// Method/signal names with their argument signatures, in order.
    fn xml_members(xml: &str) -> Vec<String> {
        let mut out = Vec::new();
        for part in xml.split('<').skip(1) {
            let tag = part.split('>').next().unwrap_or("");
            let name = |attr: &str| {
                tag.split(&format!("{attr}=\""))
                    .nth(1)
                    .and_then(|s| s.split('"').next())
                    .unwrap_or("")
                    .to_string()
            };
            if tag.starts_with("method ") || tag.starts_with("signal ") {
                out.push(format!(
                    "{} {}",
                    tag.split(' ').next().unwrap_or(""),
                    name("name")
                ));
            } else if tag.starts_with("arg ") {
                let dir = name("direction");
                out.push(format!(
                    "  {} {}",
                    if dir.is_empty() { "out" } else { &dir },
                    name("type")
                ));
            }
        }
        out
    }

    #[test]
    fn introspection_matches_xml() {
        let svc = Service {
            ctx: Ctx::new("/nonexistent".into(), "/nonexistent".into()),
            act: Activity::default(),
        };
        let mut xml = String::new();
        svc.introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(THEMES_XML), "{xml}");
        assert!(THEMES_XML.contains("ApplyConfirmed"));
    }

    #[test]
    fn busy_counts_operations() {
        let act = Activity::default();
        let a = act.busy();
        let b = act.busy();
        drop(a);
        assert_eq!(act.running.load(Ordering::SeqCst), 1);
        drop(b);
        assert_eq!(act.running.load(Ordering::SeqCst), 0);
        let m = HashMap::from([("look".to_string(), "a".to_string())]);
        assert!(act.announce(&m) && !act.announce(&m));
    }
}
