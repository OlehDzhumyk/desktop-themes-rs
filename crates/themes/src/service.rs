// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus-служба io.github.OlehDzhumyk.Themes (активується шиною, виходить після простою).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use themes_core::modules::Module;
use themes_core::state::State;
use themes_core::theme::all_themes;
use themes_core::{Ctx, DBUS_NAME, DBUS_PATH};
use zbus::object_server::SignalEmitter;

const IDLE_EXIT: u64 = 90;

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone)]
struct Activity {
    last: Arc<AtomicU64>,
    busy: Arc<AtomicBool>,
}

impl Activity {
    fn touch(&self) {
        self.last.store(now_s(), Ordering::SeqCst);
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

/// Виконати синхронну роботу бібліотеки в окремому потоці (не блокуючи виконавця zbus напряму).
fn off_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f).join().expect("потік служби впав")
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Themes")]
impl Service {
    /// (id, назва, опис, модулі, які тема містить)
    fn list(&self) -> Vec<(String, String, String, Vec<String>)> {
        self.act.touch();
        all_themes(&self.ctx)
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
    }

    /// модуль → id теми
    fn current(&self) -> HashMap<String, String> {
        self.act.touch();
        current_map(&self.ctx)
    }

    /// Застосувати тему (modules порожній = усі модулі). Повертає (relogin, повідомлення, попередження).
    #[zbus(out_args("relogin", "message", "warnings"))]
    async fn apply(
        &self,
        theme: String,
        modules: Vec<String>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<(bool, String, Vec<String>)> {
        self.act.touch();
        let mods = Module::parse_list(&modules.join(","))
            .map_err(|e| zbus::fdo::Error::InvalidArgs(e.0))?;
        let ctx = self.ctx.clone();
        self.act.busy.store(true, Ordering::SeqCst);
        let res = off_thread(move || themes_core::apply::apply(&ctx, &theme, &mods, false));
        self.act.busy.store(false, Ordering::SeqCst);
        self.act.touch();
        let res = res.map_err(|e| zbus::fdo::Error::Failed(e.0))?;
        let _ = Service::changed(&emitter, current_map(&self.ctx)).await;
        // автовідкат через ~5 с (в окремому потоці; сигнал Changed, якщо щось вимкнено)
        if res.shell_running && res.extensions_changed {
            let ctx = self.ctx.clone();
            let act = self.act.clone();
            act.busy.store(true, Ordering::SeqCst);
            let conn = emitter.connection().clone();
            std::thread::spawn(move || {
                if let Ok(bad) = themes_core::apply::verify_live(&ctx, Duration::from_secs(5)) {
                    if !bad.is_empty() {
                        let list = bad
                            .iter()
                            .map(|b| b.0.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let _ = themes_core::shell::notify(
                            "Теми: розширення вимкнено",
                            &format!(
                                "Після застосування теми розширення перейшли в стан помилки й були вимкнені: {list}"
                            ),
                            "dialog-warning-symbolic",
                        );
                        emit_changed(&conn, &ctx);
                    }
                }
                act.busy.store(false, Ordering::SeqCst);
                act.touch();
            });
        }
        let mut msg = format!("Тему «{}» застосовано", res.name);
        if res.relogin {
            msg.push_str(&format!(
                ". Потрібен перевхід: {}",
                res.relogin_reasons.join("; ")
            ));
        }
        Ok((res.relogin, msg, res.warnings))
    }

    /// Перевірка системи. Повертає (усе гаразд, повідомлення, JSON-звіт).
    #[zbus(out_args("ok", "messages", "report"))]
    async fn doctor(&self, fix: bool) -> zbus::fdo::Result<(bool, Vec<String>, String)> {
        self.act.touch();
        let ctx = self.ctx.clone();
        self.act.busy.store(true, Ordering::SeqCst);
        let rep = off_thread(move || {
            themes_core::doctor::doctor(
                &ctx,
                &themes_core::doctor::DoctorOptions {
                    fix,
                    wait_shell: Duration::ZERO,
                    notify: fix,
                },
            )
        });
        self.act.busy.store(false, Ordering::SeqCst);
        self.act.touch();
        let rep = rep.map_err(|e| zbus::fdo::Error::Failed(e.0))?;
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

fn emit_changed(conn: &zbus::Connection, ctx: &Ctx) {
    let map = current_map(ctx);
    let conn = conn.clone();
    let _ = std::thread::spawn(move || {
        zbus::block_on(async {
            if let Ok(iface) = conn
                .object_server()
                .interface::<_, Service>(DBUS_PATH)
                .await
            {
                let _ = Service::changed(iface.signal_emitter(), map).await;
            }
        })
    })
    .join();
}

fn state_mtime(ctx: &Ctx) -> Option<SystemTime> {
    std::fs::metadata(&ctx.state)
        .and_then(|m| m.modified())
        .ok()
}

pub fn run() -> i32 {
    themes_core::util::ensure_bus();
    let ctx = Ctx::from_env();
    let act = Activity {
        last: Arc::new(AtomicU64::new(now_s())),
        busy: Arc::new(AtomicBool::new(false)),
    };
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
            eprintln!("themes --dbus-service: не вдалося зайняти {DBUS_NAME}: {e}");
            return 1;
        }
    };
    // стежимо за state.json, щоб сповіщати про застосування з CLI/GUI
    let mut seen = state_mtime(&ctx);
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let m = state_mtime(&ctx);
        if m != seen {
            seen = m;
            emit_changed(&conn, &ctx);
        }
        let idle = now_s().saturating_sub(act.last.load(Ordering::SeqCst));
        if idle >= IDLE_EXIT && !act.busy.load(Ordering::SeqCst) {
            break;
        }
    }
    0
}
