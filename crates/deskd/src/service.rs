// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus-інтерфейси deskd: `io.github.OlehDzhumyk.Desk` і `io.github.OlehDzhumyk.Desk.Clipboard`.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use desk_proto::ClipItem;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use crate::clipboard::{Engine, Pushed};
use crate::persist::Saver;
use crate::trust::Gate;

pub type Shared = Arc<Mutex<Engine>>;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub(crate) fn lock(e: &Shared) -> MutexGuard<'_, Engine> {
    e.lock().unwrap_or_else(|p| p.into_inner())
}

/// Own [`desk_proto::BUS_NAME`]. Not replaceable (ADR 0008): only with `replace` (the development
/// mode `deskd --replace`) the name is taken from a `--replace` deskd and left replaceable.
pub async fn own_name(conn: &zbus::Connection, replace: bool) -> zbus::Result<()> {
    use zbus::fdo::{RequestNameFlags as F, RequestNameReply};
    let mut flags = F::DO_NOT_QUEUE;
    if replace {
        flags = F::DO_NOT_QUEUE | F::ALLOW_REPLACEMENT | F::REPLACE_EXISTING;
    }
    match conn
        .request_name_with_flags(desk_proto::BUS_NAME, flags)
        .await?
    {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(()),
        _ => Err(zbus::Error::NameTaken),
    }
}

fn not_found(id: u64) -> zbus::fdo::Error {
    zbus::fdo::Error::InvalidArgs(format!("no item {id}"))
}

pub struct Desk {
    pub features: Vec<String>,
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk")]
impl Desk {
    #[zbus(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    #[zbus(property)]
    fn features(&self) -> Vec<String> {
        self.features.clone()
    }
}

pub struct ClipboardIface {
    pub eng: Shared,
    /// Writes the history after a quiet period (and on shutdown).
    pub saver: Saver,
    /// Записати приватний режим у GSettings (у тестах — нічого).
    pub set_private: fn(bool),
    /// Content and changes only for the shell and our binaries (ADR 0008).
    pub gate: Gate,
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Clipboard")]
impl ClipboardIface {
    #[allow(clippy::too_many_arguments)] // the bus arguments plus header, connection, emitter
    async fn push(
        &self,
        mime: String,
        data: Vec<u8>,
        offered: Vec<String>,
        source_app: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<u64> {
        self.gate.check(conn, Some(&hdr)).await?;
        let r = {
            let mut eng = lock(&self.eng);
            eng.expire(now());
            let r = eng.push(&mime, &data, &offered, &source_app, now());
            if !matches!(r, Pushed::Skipped(_)) {
                self.saver.touch();
            }
            r
        };
        match r {
            Pushed::Added(id) | Pushed::Bumped(id) => {
                let _ = Self::changed(&em).await;
                let _ = self.count_changed(&em).await;
                Ok(id)
            }
            Pushed::Skipped(_) => Ok(0),
        }
    }

    async fn list(
        &self,
        query: String,
        limit: u32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<Vec<ClipItem>> {
        self.gate.check(conn, Some(&hdr)).await?;
        let mut eng = lock(&self.eng);
        eng.expire(now());
        Ok(eng.list(&query, limit))
    }

    #[zbus(out_args("mime", "data"))]
    async fn get(
        &self,
        id: u64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<(String, Vec<u8>)> {
        self.gate.check(conn, Some(&hdr)).await?;
        let mut eng = lock(&self.eng);
        eng.expire(now());
        eng.get(id).ok_or_else(|| not_found(id))
    }

    #[zbus(out_args("mime", "data"))]
    async fn activate(
        &self,
        id: u64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<(String, Vec<u8>)> {
        self.gate.check(conn, Some(&hdr)).await?;
        let r = {
            let mut eng = lock(&self.eng);
            eng.expire(now());
            let r = eng.activate(id, now());
            self.saver.touch();
            r
        };
        let r = r.ok_or_else(|| not_found(id))?;
        let _ = Self::changed(&em).await;
        Ok(r)
    }

    async fn pin(
        &self,
        id: u64,
        pinned: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let found = {
            let mut eng = lock(&self.eng);
            let found = eng.find(id).is_some();
            if eng.pin(id, pinned) {
                self.saver.touch();
            }
            found
        };
        if !found {
            return Err(not_found(id));
        }
        let _ = Self::changed(&em).await;
        Ok(())
    }

    async fn delete(
        &self,
        id: u64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let changed = {
            let mut eng = lock(&self.eng);
            let c = eng.delete(id);
            if c {
                self.saver.touch();
            }
            c
        };
        if changed {
            let _ = Self::changed(&em).await;
            let _ = self.count_changed(&em).await;
        }
        Ok(())
    }

    async fn clear(
        &self,
        keep_pinned: bool,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        {
            let mut eng = lock(&self.eng);
            eng.clear(keep_pinned);
            self.saver.touch();
        }
        let _ = Self::changed(&em).await;
        let _ = self.count_changed(&em).await;
        Ok(())
    }

    #[zbus(property)]
    fn private(&self) -> bool {
        lock(&self.eng).cfg.private
    }

    /// Turning private mode off from a sandbox would re-enable recording: gated as well.
    #[zbus(property)]
    async fn set_private(
        &self,
        value: bool,
        #[zbus(header)] hdr: Option<Header<'_>>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, hdr.as_ref()).await?;
        lock(&self.eng).cfg.private = value;
        (self.set_private)(value);
        Ok(())
    }

    #[zbus(property)]
    fn enabled(&self) -> bool {
        lock(&self.eng).cfg.enabled
    }

    #[zbus(property)]
    fn count(&self) -> u32 {
        u32::try_from(lock(&self.eng).len()).unwrap_or(u32::MAX)
    }

    #[zbus(signal)]
    pub async fn changed(em: &SignalEmitter<'_>) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use desk_proto::{CLIPBOARD_XML, DESK_XML, xml_members};
    use zbus::object_server::Interface;

    use super::*;

    /// A private `dbus-daemon` for this test only (killed on drop). `None` when not installed.
    struct Bus(std::process::Child, String);

    impl Bus {
        fn start(dir: &std::path::Path) -> Option<Bus> {
            use std::io::BufRead;
            let mut child = std::process::Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
                .arg(format!("--address=unix:path={}", dir.join("bus").display()))
                .env_clear()
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()?;
            let mut line = String::new();
            std::io::BufReader::new(child.stdout.take()?)
                .read_line(&mut line)
                .ok()?;
            Some(Bus(child, line.trim().to_string()))
        }

        async fn connect(&self) -> zbus::Connection {
            zbus::connection::Builder::address(self.1.as_str())
                .build()
                .await
                .unwrap()
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// On a real (private) bus: a foreign client is refused content and changes, the owner of
    /// org.gnome.Shell is served (and loses that when it releases the name), metadata stays
    /// open, and the bus name cannot be taken over.
    #[test]
    fn untrusted_clients_are_refused() {
        let dir = std::env::temp_dir().join(format!("deskd-trust-bus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let Some(bus) = Bus::start(&dir) else {
            eprintln!("dbus-daemon not installed: skipped");
            return;
        };
        zbus::block_on(async {
            let mut eng = Engine::new(Default::default(), dir.join("data"), dir.join("run"));
            eng.push("text/plain", b"a secret note", &[], "", 1);
            let server = zbus::connection::Builder::address(bus.1.as_str())
                .serve_at(
                    desk_proto::CLIPBOARD_PATH,
                    ClipboardIface {
                        eng: Arc::new(Mutex::new(eng)),
                        saver: Saver::noop(),
                        set_private: |_| {},
                        // the test binary is deskd's caller here: not one of the trusted binaries
                        gate: Gate::new(Arc::new(crate::trust::Trust::with_binaries(vec![]))),
                    },
                )
                .build()
                .await
                .unwrap();
            own_name(&server, false).await.unwrap();

            let client = bus.connect().await;
            let p = desk_proto::ClipboardProxy::builder(&client)
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
                .unwrap();
            let refused = |r: zbus::Result<()>| {
                let e = r.unwrap_err().to_string();
                assert!(e.contains("AccessDenied"), "{e}");
            };
            refused(p.list("", 0).await.map(drop));
            refused(p.get(1).await.map(drop));
            refused(p.activate(1).await.map(drop));
            refused(p.push("text/plain", b"injected", &[], "").await.map(drop));
            refused(p.delete(1).await);
            refused(p.clear(false).await);
            refused(p.set_private(true).await);
            assert_eq!(p.count().await.unwrap(), 1, "metadata stays open");

            // the stand-in shell: owning org.gnome.Shell makes the same client trusted
            client.request_name(crate::trust::SHELL_NAME).await.unwrap();
            let items = p.list("", 0).await.unwrap();
            assert_eq!(items[0].text, "a secret note");
            client.release_name(crate::trust::SHELL_NAME).await.unwrap();
            refused(p.list("", 0).await.map(drop));

            // S-2: nobody takes the name over, not even with REPLACE_EXISTING
            let thief = bus.connect().await;
            use zbus::fdo::RequestNameFlags as F;
            let r = thief
                .request_name_with_flags(
                    desk_proto::BUS_NAME,
                    F::REPLACE_EXISTING | F::DO_NOT_QUEUE,
                )
                .await;
            assert!(matches!(r, Err(zbus::Error::NameTaken)), "{r:?}");
            let dbus = zbus::fdo::DBusProxy::new(&client).await.unwrap();
            let owner = dbus
                .get_name_owner(desk_proto::BUS_NAME.try_into().unwrap())
                .await
                .unwrap();
            assert_eq!(
                Some(owner.as_ref()),
                server.unique_name().map(|n| n.as_ref())
            );
        });
        drop(bus);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Члени, які генерує `#[zbus::interface]`, збігаються з data/dbus/*.xml (його читає міст).
    #[test]
    fn introspection_matches_xml() {
        let mut xml = String::new();
        let eng = Engine::new(
            Default::default(),
            "/nonexistent".into(),
            "/nonexistent".into(),
        );
        let iface = ClipboardIface {
            eng: Arc::new(Mutex::new(eng)),
            saver: Saver::noop(),
            set_private: |_| {},
            gate: Gate::open(),
        };
        iface.introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(CLIPBOARD_XML), "{xml}");
        assert!(xml.contains("a(tssssbxt)"));
        let mut xml = String::new();
        Desk { features: vec![] }.introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(DESK_XML));
    }
}
