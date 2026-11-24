// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.Search`
//! (`data/dbus/io.github.OlehDzhumyk.Desk.Search.xml`).
//!
//! `Query` ranks an in-memory list (the first call after a change reads the desktop entries);
//! `RecordLaunch` writes one small file. Nothing runs between calls.

use std::sync::{Arc, Mutex, MutexGuard};

use zbus::fdo;
use zbus::message::Header;

use super::{Search, SearchError, now};
use crate::trust::Gate;

pub type Shared = Arc<Mutex<Search>>;

pub fn lock(m: &Shared) -> MutexGuard<'_, Search> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct SearchIface {
    pub engine: Shared,
    /// Only GNOME Shell and desktop-themes-rs binaries may search or read usage (ADR 0008).
    pub gate: Gate,
}

fn to_fdo(e: SearchError) -> fdo::Error {
    use desk_search::Error as Core;
    match e {
        SearchError::Disabled => fdo::Error::NotSupported(e.to_string()),
        SearchError::Core(Core::InvalidId(_)) => fdo::Error::InvalidArgs(e.to_string()),
        SearchError::Core(Core::Io(io)) => fdo::Error::IOError(io.to_string()),
        SearchError::Core(Core::Json(json)) => fdo::Error::Failed(json.to_string()),
    }
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Search")]
impl SearchIface {
    #[zbus(out_args("results_json"))]
    async fn query(
        &self,
        text: String,
        max_results: u32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        lock(&self.engine)
            .query_json(&text, max_results, now())
            .map_err(to_fdo)
    }

    async fn record_launch(
        &self,
        item_id: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        lock(&self.engine)
            .record_launch(&item_id, now())
            .map(drop)
            .map_err(to_fdo)
    }

    async fn clear_usage(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        lock(&self.engine).clear_usage().map_err(to_fdo)
    }
}

#[cfg(test)]
mod tests {
    use desk_proto::search::SearchProxy;
    use desk_proto::{SEARCH_PATH, SEARCH_XML, xml_members};
    use zbus::object_server::Interface;

    use super::super::Config;
    use super::super::tests::{engine_in, tmp};
    use super::*;

    fn shared(dir: &std::path::Path, config: Config) -> Shared {
        Arc::new(Mutex::new(engine_in(dir, config)))
    }

    #[test]
    fn search_introspection_matches_xml() {
        let dir = tmp("introspect");
        let mut xml = String::new();
        SearchIface {
            engine: shared(&dir, Config::default()),
            gate: Gate::open(),
        }
        .introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(SEARCH_XML), "{xml}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn connect(eng: Shared) -> (zbus::Connection, zbus::Connection) {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(a)
            .server(zbus::Guid::generate())
            .p2p()
            .serve_at(
                SEARCH_PATH,
                SearchIface {
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

    async fn proxy_on(client: &zbus::Connection) -> SearchProxy<'_> {
        SearchProxy::builder(client)
            .destination("io.github.OlehDzhumyk.Desk")
            .build()
            .await
            .unwrap()
    }

    #[test]
    fn api_over_a_private_connection() {
        let dir = tmp("p2p");
        let usage = dir.join("data/search-usage.json");
        zbus::block_on(async {
            let (_server, client) = connect(shared(&dir, Config::default())).await;
            let p = proxy_on(&client).await;

            // AC-SRCH-02 and AC-SRCH-03 through the bus
            let r: serde_json::Value =
                serde_json::from_str(&p.query("text ed", 0).await.unwrap()).unwrap();
            assert_eq!(r[0]["id"], "app:org.gnome.TextEditor.desktop");
            assert_eq!(r[0]["kind"], "app");
            assert_eq!(r[0]["activate"], "org.gnome.TextEditor.desktop");
            let r: serde_json::Value =
                serde_json::from_str(&p.query("= 12*(3+4)", 0).await.unwrap()).unwrap();
            assert_eq!(r[0]["kind"], "calculator");
            assert_eq!(r[0]["activate"], "84");

            p.record_launch("app:org.gnome.TextEditor.desktop")
                .await
                .unwrap();
            assert!(usage.exists());
            let r: serde_json::Value =
                serde_json::from_str(&p.query("", 0).await.unwrap()).unwrap();
            assert_eq!(r[0]["id"], "app:org.gnome.TextEditor.desktop");
            p.clear_usage().await.unwrap();
            assert!(!usage.exists());

            for bad in ["", "a\u{1}b"] {
                let err = p.record_launch(bad).await.unwrap_err();
                assert!(err.to_string().contains("InvalidArgs"), "{err}");
            }

            // AC-SRCH-07: nothing is written with remember-usage off
            let (_server, client) = connect(shared(
                &dir,
                Config {
                    remember_usage: false,
                    ..Config::default()
                },
            ))
            .await;
            let p = proxy_on(&client).await;
            p.record_launch("action:lock").await.unwrap();
            assert!(!usage.exists());

            let (_server, client) = connect(shared(
                &dir,
                Config {
                    enabled: false,
                    ..Config::default()
                },
            ))
            .await;
            let p = proxy_on(&client).await;
            let err = p.query("x", 0).await.unwrap_err();
            assert!(err.to_string().contains("NotSupported"), "{err}");
            p.clear_usage().await.unwrap();
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
