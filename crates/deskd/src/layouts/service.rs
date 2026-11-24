// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.Layouts`
//! (`data/dbus/io.github.OlehDzhumyk.Desk.Layouts.xml`).
//!
//! Every call is a lookup or a computation of a few rectangles; `SetActiveLayout` writes one small
//! file. Nothing runs between calls.

use std::sync::{Arc, Mutex, MutexGuard};

use desk_layouts::Rect;
use desk_proto::LAYOUTS_PATH;
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use super::{Layouts, LayoutsError};
use crate::trust::Gate;

pub type Shared = Arc<Mutex<Layouts>>;

pub fn lock(m: &Shared) -> MutexGuard<'_, Layouts> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct LayoutsIface {
    pub engine: Shared,
    /// Only GNOME Shell and desktop-themes-rs binaries may read or change layouts (ADR 0008).
    pub gate: Gate,
}

fn to_fdo(e: LayoutsError) -> fdo::Error {
    use desk_layouts::Error as Core;
    match e {
        LayoutsError::Disabled => fdo::Error::NotSupported(e.to_string()),
        LayoutsError::Core(Core::Io(io)) => fdo::Error::IOError(io.to_string()),
        LayoutsError::Core(Core::Json(json)) => fdo::Error::Failed(json.to_string()),
        LayoutsError::BadArgs(_)
        | LayoutsError::Core(Core::NotFound(_) | Core::InvalidLayout(_)) => {
            fdo::Error::InvalidArgs(e.to_string())
        }
    }
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Layouts")]
impl LayoutsIface {
    #[zbus(out_args("json"))]
    async fn list_layouts(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        lock(&self.engine).list_json().map_err(to_fdo)
    }

    #[zbus(out_args("layout_id"))]
    async fn active_layout(
        &self,
        monitor_key: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        lock(&self.engine).active(&monitor_key).map_err(to_fdo)
    }

    async fn set_active_layout(
        &self,
        monitor_key: String,
        layout_id: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let changed = lock(&self.engine)
            .set_active(&monitor_key, &layout_id)
            .map_err(to_fdo)?;
        if changed {
            let _ = Self::changed(&em).await;
        }
        Ok(())
    }

    #[zbus(out_args("zones_json"))]
    #[allow(
        clippy::too_many_arguments,
        reason = "the D-Bus method takes a rectangle"
    )]
    async fn resolve_zones(
        &self,
        layout_id: String,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        lock(&self.engine)
            .resolve_json(&layout_id, Rect::new(x, y, w, h))
            .map_err(to_fdo)
    }

    /// Layouts, an active layout or a setting that changes the zones changed.
    #[zbus(signal)]
    pub async fn changed(em: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Emit `Changed()` if the Layouts object is registered.
pub async fn emit(conn: &zbus::Connection) {
    if let Ok(iface) = conn
        .object_server()
        .interface::<_, LayoutsIface>(LAYOUTS_PATH)
        .await
    {
        let _ = LayoutsIface::changed(iface.signal_emitter()).await;
    }
}

#[cfg(test)]
mod tests {
    use desk_proto::layouts::LayoutsProxy;
    use desk_proto::{LAYOUTS_XML, xml_members};
    use futures_util::StreamExt;
    use zbus::object_server::Interface;

    use super::super::Config;
    use super::super::tests::{engine_in, tmp};
    use super::*;

    fn shared(dir: &std::path::Path, enabled: bool) -> Shared {
        Arc::new(Mutex::new(engine_in(
            dir,
            Config {
                enabled,
                ..Config::default()
            },
        )))
    }

    #[test]
    fn layouts_introspection_matches_xml() {
        let dir = tmp("introspect");
        let mut xml = String::new();
        LayoutsIface {
            engine: shared(&dir, true),
            gate: Gate::open(),
        }
        .introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(LAYOUTS_XML), "{xml}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn connect(eng: Shared) -> (zbus::Connection, zbus::Connection) {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(a)
            .server(zbus::Guid::generate())
            .p2p()
            .serve_at(
                LAYOUTS_PATH,
                LayoutsIface {
                    engine: eng,
                    gate: Gate::open(),
                },
            )
            .build();
        let client = zbus::connection::Builder::unix_stream(b).p2p().build();
        futures_util::future::try_join(server, client)
            .await
            .unwrap()
    }

    async fn proxy_on(client: &zbus::Connection) -> LayoutsProxy<'_> {
        LayoutsProxy::builder(client)
            .destination("io.github.OlehDzhumyk.Desk")
            .build()
            .await
            .unwrap()
    }

    #[test]
    fn api_over_a_private_connection() {
        let dir = tmp("p2p");
        zbus::block_on(async {
            let (server, client) = connect(shared(&dir, true)).await;
            let p = proxy_on(&client).await;

            let list: serde_json::Value =
                serde_json::from_str(&p.list_layouts().await.unwrap()).unwrap();
            let ids: Vec<&str> = list
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["id"].as_str().unwrap())
                .collect();
            assert_eq!(
                ids,
                [
                    "halves",
                    "thirds",
                    "two-thirds",
                    "quarters",
                    "focus",
                    "stack"
                ]
            );

            assert_eq!(p.active_layout("DP-1").await.unwrap(), "halves");
            let mut changed = p.receive_changed().await.unwrap();
            p.set_active_layout("DP-1", "stack").await.unwrap();
            assert!(changed.next().await.is_some());
            assert_eq!(p.active_layout("DP-1").await.unwrap(), "stack");

            // AC-WIN-01: the left half of a 1920x1080 work area under a 27 px top bar
            let zones: serde_json::Value =
                serde_json::from_str(&p.resolve_zones("halves", 0, 27, 1920, 1053).await.unwrap())
                    .unwrap();
            assert_eq!(
                zones[0],
                serde_json::json!({"x": 8, "y": 36, "w": 950, "h": 1036})
            );

            // the settings watcher's path: emit() reaches the client
            emit(&server).await;
            assert!(changed.next().await.is_some());

            for err in [
                p.set_active_layout("DP-1", "nope").await.unwrap_err(),
                p.active_layout("").await.unwrap_err(),
                p.resolve_zones("halves", 0, 0, 0, 0).await.unwrap_err(),
            ] {
                assert!(err.to_string().contains("InvalidArgs"), "{err}");
            }

            let (_server, client) = connect(shared(&dir, false)).await;
            let p = proxy_on(&client).await;
            let err = p.list_layouts().await.unwrap_err();
            assert!(err.to_string().contains("NotSupported"), "{err}");
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
