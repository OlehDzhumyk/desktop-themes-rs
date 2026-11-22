// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.Mixer` (data/dbus/io.github.OlehDzhumyk.Desk.Mixer.xml).
//!
//! Requests return as soon as they are handed to the PipeWire thread; the result shows up in the
//! next `Changed` signal, as the server confirms it.

use std::sync::Arc;
use std::time::Instant;

use desk_proto::{MIXER_PATH, MIXER_VERSION, MixerApp, MixerDevice, MixerStream};
use zbus::object_server::SignalEmitter;

use super::model::{Class, MixError, dirty};
use super::{Mixer, lock};

pub struct MixerIface {
    pub mixer: Arc<Mixer>,
}

fn err(e: MixError) -> zbus::fdo::Error {
    match e {
        MixError::Unavailable => zbus::fdo::Error::Failed(e.to_string()),
        _ => zbus::fdo::Error::InvalidArgs(e.to_string()),
    }
}

impl MixerIface {
    fn run(
        &self,
        plan: impl FnOnce(&mut super::model::Graph) -> Result<(Vec<super::model::Op>, bool), MixError>,
    ) -> zbus::fdo::Result<()> {
        self.mixer.request(plan).map_err(err)
    }
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Mixer")]
impl MixerIface {
    fn list_devices(&self, kind: String) -> zbus::fdo::Result<Vec<MixerDevice>> {
        self.mixer.wanted();
        lock(&self.mixer.graph).devices(&kind).map_err(err)
    }

    fn list_streams(&self) -> Vec<MixerStream> {
        lock(&self.mixer.graph).streams()
    }

    fn list_apps(&self, include_silent: bool) -> Vec<MixerApp> {
        self.mixer.wanted();
        lock(&self.mixer.graph).apps(Instant::now(), include_silent)
    }

    fn set_volume(&self, id: u32, volume: f64) -> zbus::fdo::Result<()> {
        self.run(|g| g.set_volume(id, volume))
    }

    fn set_mute(&self, id: u32, mute: bool) -> zbus::fdo::Result<()> {
        self.run(|g| g.set_mute(id, mute))
    }

    fn set_app_volume(&self, app: String, volume: f64) -> zbus::fdo::Result<()> {
        self.run(|g| g.set_app_volume(&app, volume))
    }

    fn set_app_mute(&self, app: String, mute: bool) -> zbus::fdo::Result<()> {
        self.run(|g| g.set_app_mute(&app, mute))
    }

    fn set_default(&self, kind: String, id: u32) -> zbus::fdo::Result<()> {
        self.run(|g| Ok((g.set_default(&kind, id)?, false)))
    }

    fn move_stream(&self, stream: u32, device: u32) -> zbus::fdo::Result<()> {
        self.run(|g| Ok((g.move_stream(stream, device)?, false)))
    }

    fn move_app(&self, app: String, device: u32) -> zbus::fdo::Result<()> {
        self.run(|g| Ok((g.move_app(&app, device)?, false)))
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        MIXER_VERSION
    }

    #[zbus(property)]
    fn available(&self) -> bool {
        lock(&self.mixer.graph).connected
    }

    #[zbus(property)]
    fn default_output(&self) -> u32 {
        lock(&self.mixer.graph).default_id(Class::Sink).unwrap_or(0)
    }

    #[zbus(property)]
    fn default_input(&self) -> u32 {
        lock(&self.mixer.graph)
            .default_id(Class::Source)
            .unwrap_or(0)
    }

    #[zbus(signal)]
    pub async fn changed(em: &SignalEmitter<'_>, what: Vec<String>) -> zbus::Result<()>;
}

/// Send `Changed` (and the property changes it implies) for `flags`.
pub async fn emit(conn: &zbus::Connection, flags: u8) {
    let Ok(iface) = conn
        .object_server()
        .interface::<_, MixerIface>(MIXER_PATH)
        .await
    else {
        return;
    };
    let em = iface.signal_emitter();
    let _ = MixerIface::changed(em, dirty::names(flags)).await;
    let g = iface.get().await;
    if flags & dirty::AVAILABLE != 0 {
        let _ = g.available_changed(em).await;
    }
    if flags & (dirty::DEFAULTS | dirty::AVAILABLE) != 0 {
        let _ = g.default_output_changed(em).await;
        let _ = g.default_input_changed(em).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use desk_proto::mixer::MixerProxy;
    use desk_proto::{MIXER_XML, xml_members};
    use futures_util::StreamExt;
    use zbus::object_server::Interface;

    use super::super::model::{Config, Graph, Op, Props};
    use super::super::notify::Notifier;
    use super::super::testing::Echo;
    use super::*;

    fn props(kv: &[(&str, &str)]) -> Props {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// A mixer over a mocked graph: two sinks, mpv playing on the first.
    fn mixer() -> (Arc<Mixer>, Arc<Mutex<Vec<Op>>>) {
        let mut g = Graph::new(Config::default(), Box::new(|_| None));
        g.connected = true;
        for (id, name) in [(30, "speakers"), (31, "headphones")] {
            g.add_node(
                id,
                props(&[
                    ("media.class", "Audio/Sink"),
                    ("node.name", name),
                    ("node.description", name),
                ]),
            );
            g.node_params(id, Some(vec![1.0, 1.0]), Some(false));
        }
        g.add_node(
            60,
            props(&[
                ("media.class", "Stream/Output/Audio"),
                ("application.name", "mpv"),
            ]),
        );
        g.node_params(60, Some(vec![1.0, 1.0]), Some(false));
        g.node_state(60, true, Instant::now());
        g.metadata(
            0,
            Some("default.audio.sink"),
            Some(r#"{"name":"speakers"}"#),
        );
        let graph = Arc::new(Mutex::new(g));
        let log = Arc::new(Mutex::new(Vec::new()));
        let m = Arc::new(Mixer::new(graph.clone(), Notifier::inert(), None));
        m.set_backend(Some(Box::new(Echo {
            graph,
            log: log.clone(),
        })));
        (m, log)
    }

    #[test]
    fn introspection_matches_xml() {
        let (m, _) = mixer();
        let mut xml = String::new();
        MixerIface { mixer: m }.introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(MIXER_XML), "{xml}");
        for sig in ["a(ussssdbb)", "a(ussssdbbu)", "a(ssssdbbuau)"] {
            assert!(xml.contains(sig), "{sig}");
        }
    }

    /// The API over a private peer-to-peer D-Bus connection (no session bus involved): lists,
    /// requests, errors and the Changed signal.
    #[test]
    fn api_over_a_private_connection() {
        zbus::block_on(async {
            let (m, log) = mixer();
            let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
            let guid = zbus::Guid::generate();
            let server = zbus::connection::Builder::unix_stream(a)
                .server(guid)
                .p2p()
                .serve_at(MIXER_PATH, MixerIface { mixer: m.clone() })
                .build();
            let client = zbus::connection::Builder::unix_stream(b).p2p().build();
            let (server, client) = futures_util::future::try_join(server, client)
                .await
                .unwrap();

            let p = MixerProxy::builder(&client)
                .destination("io.github.OlehDzhumyk.Desk")
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
                .unwrap();
            assert_eq!(p.version().await.unwrap(), 1);
            assert!(p.available().await.unwrap());
            assert_eq!(p.default_output().await.unwrap(), 30);
            let outs = p.list_devices("output").await.unwrap();
            assert_eq!(outs.len(), 2);
            assert!(p.list_devices("speakers").await.is_err());
            let apps = p.list_apps(false).await.unwrap();
            assert_eq!(apps.len(), 1);
            assert_eq!(apps[0].key, "mpv");

            p.set_app_volume("mpv", 0.3).await.unwrap();
            let s = p.list_streams().await.unwrap();
            assert!((s[0].volume - 0.3).abs() < 1e-6, "{s:?}");
            p.set_app_mute("mpv", true).await.unwrap();
            assert!(p.list_apps(false).await.unwrap()[0].mute);
            let e = p.set_volume(99, 0.5).await.unwrap_err();
            assert!(e.to_string().contains("InvalidArgs"), "{e}");

            let mut changed = p.receive_changed().await.unwrap();
            p.set_default("output", 31).await.unwrap();
            assert_eq!(p.list_devices("output").await.unwrap()[0].id, 31);
            assert!(p.list_devices("output").await.unwrap()[0].is_default);
            emit(&server, dirty::DEFAULTS).await;
            let sig = changed.next().await.unwrap();
            assert_eq!(sig.args().unwrap().what, vec!["defaults".to_string()]);
            assert_eq!(p.default_output().await.unwrap(), 31);

            p.move_app("mpv", 30).await.unwrap();
            assert!(log.lock().unwrap().iter().any(|op| matches!(
                op,
                Op::Metadata { subject: 60, key, value: Some(v), .. } if key == "target.node" && v == "30"
            )));

            // disconnected: requests fail with Failed, lists are empty
            m.set_backend(None);
            lock(&m.graph).forget_server();
            let e = p.set_app_volume("mpv", 0.5).await.unwrap_err();
            assert!(e.to_string().contains("Failed"), "{e}");
            assert!(p.list_apps(true).await.unwrap().is_empty());
        });
    }
}
