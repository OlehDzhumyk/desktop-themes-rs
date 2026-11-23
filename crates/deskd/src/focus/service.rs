// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.Focus`
//! (`data/dbus/io.github.OlehDzhumyk.Desk.Focus.xml`).

use desk_proto::FOCUS_PATH;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use super::model::Mode;
use super::{Shared, lock};
use crate::schedule::Scheduler;
use crate::trust::Gate;

pub struct FocusIface {
    pub eng: Shared,
    pub scheduler: Scheduler,
    /// Only GNOME Shell and desktop-themes-rs binaries may read or change focus state (ADR 0008).
    pub gate: Gate,
}

type StatusTuple = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    i64,
    Vec<String>,
);

fn invalid(e: String) -> zbus::fdo::Error {
    zbus::fdo::Error::InvalidArgs(e)
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Focus")]
impl FocusIface {
    #[zbus(out_args(
        "mode",
        "name",
        "icon",
        "source",
        "until",
        "break_until",
        "next_mode",
        "next_start",
        "allowed_apps"
    ))]
    async fn status(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<StatusTuple> {
        self.gate.check(conn, Some(&hdr)).await?;
        let s = lock(&self.eng).status();
        Ok((
            s.mode,
            s.name,
            s.icon,
            s.source,
            s.until,
            s.break_until,
            s.next_mode,
            s.next_start,
            s.allowed_apps,
        ))
    }

    async fn list_modes(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        Ok(lock(&self.eng).modes_json())
    }

    async fn start(
        &self,
        mode: String,
        until: i64,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let r = lock(&self.eng).start(&mode, (until > 0).then_some(until));
        self.finish(r, &em).await
    }

    async fn start_session(
        &self,
        minutes: u32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let r = lock(&self.eng).start_session(minutes);
        self.finish(r, &em).await
    }

    async fn stop(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let changed = lock(&self.eng).stop();
        self.finish(Ok(changed), &em).await
    }

    async fn save_mode(
        &self,
        mode: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let m: Mode =
            serde_json::from_str(&mode).map_err(|e| invalid(format!("invalid mode JSON: {e}")))?;
        let r = lock(&self.eng).save_mode(m);
        self.finish(r.map(|_| true), &em).await
    }

    async fn delete_mode(
        &self,
        id: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let r = lock(&self.eng).delete_mode(&id);
        self.finish(r.map(|_| true), &em).await
    }

    async fn recompute(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let (_, changed) = lock(&self.eng).tick();
        self.finish(Ok(changed), &em).await
    }

    /// State, modes or schedule changed.
    #[zbus(signal)]
    pub async fn changed(em: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// A focus session ran out (the bridge shows a quiet notification).
    #[zbus(signal)]
    pub async fn session_ended(em: &SignalEmitter<'_>) -> zbus::Result<()>;
}

impl FocusIface {
    async fn finish(
        &self,
        r: Result<bool, String>,
        em: &SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let changed = r.map_err(invalid)?;
        self.scheduler.trigger();
        if changed {
            let _ = Self::changed(em).await;
        }
        Ok(())
    }
}

/// Emit `Changed()` (and `SessionEnded()` when `session_ended`) if the Focus object is registered.
pub async fn emit(conn: &zbus::Connection, session_ended: bool) {
    if let Ok(iface) = conn
        .object_server()
        .interface::<_, FocusIface>(FOCUS_PATH)
        .await
    {
        let _ = FocusIface::changed(iface.signal_emitter()).await;
        if session_ended {
            let _ = FocusIface::session_ended(iface.signal_emitter()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use desk_proto::focus::FocusProxy;
    use desk_proto::{FOCUS_XML, xml_members};
    use futures_util::StreamExt;
    use zbus::object_server::Interface;

    use super::super::model::Store;
    use super::super::model::tests::at;
    use super::super::{Config, Effects, FocusEngine};
    use super::*;

    struct Nothing;
    impl Effects for Nothing {
        fn set_show_banners(&mut self, _: bool) {}
        fn set_event_sounds(&mut self, _: bool) {}
        fn set_active_mode(&mut self, _: &str) {}
        fn persist(&mut self, _: &Store) {}
    }

    fn test_engine() -> Shared {
        Arc::new(Mutex::new(FocusEngine::new(
            Config::default(),
            None,
            true,
            true,
            String::new(),
            Nothing,
            || at(0, 12, 0),
        )))
    }

    #[test]
    fn focus_introspection_matches_xml() {
        let mut xml = String::new();
        FocusIface {
            eng: test_engine(),
            scheduler: Scheduler::inert(),
            gate: Gate::open(),
        }
        .introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(FOCUS_XML), "{xml}");
    }

    #[test]
    fn api_over_a_private_connection() {
        zbus::block_on(async {
            let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
            let guid = zbus::Guid::generate();
            let server = zbus::connection::Builder::unix_stream(a)
                .server(guid)
                .p2p()
                .serve_at(
                    FOCUS_PATH,
                    FocusIface {
                        eng: test_engine(),
                        scheduler: Scheduler::inert(),
                        gate: Gate::open(),
                    },
                )
                .build();
            let client = zbus::connection::Builder::unix_stream(b).p2p().build();
            let (_server, client) = futures_util::future::try_join(server, client)
                .await
                .unwrap();
            let p = FocusProxy::builder(&client)
                .destination("io.github.OlehDzhumyk.Desk")
                .build()
                .await
                .unwrap();

            assert_eq!(p.status().await.unwrap().0, "");
            let mut changed = p.receive_changed().await.unwrap();
            p.start("work", 0).await.unwrap();
            assert!(changed.next().await.is_some());
            let st = p.status().await.unwrap();
            assert_eq!((st.0.as_str(), st.3.as_str(), st.4), ("work", "manual", 0));

            let err = p.start("nope", 0).await.unwrap_err();
            assert!(err.to_string().contains("InvalidArgs"), "{err}");
            let err = p.save_mode("{").await.unwrap_err();
            assert!(err.to_string().contains("InvalidArgs"), "{err}");

            p.save_mode(r#"{"id":"gym","name":"Gym"}"#).await.unwrap();
            assert!(p.list_modes().await.unwrap().contains("\"gym\""));
            p.delete_mode("gym").await.unwrap();
            p.stop().await.unwrap();
            assert_eq!(p.status().await.unwrap().0, "");
            p.start_session(15).await.unwrap();
            assert_eq!(p.status().await.unwrap().3, "session");
            p.recompute().await.unwrap();
        });
    }
}
