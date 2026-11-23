// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.Appearance`
//! (`data/dbus/io.github.OlehDzhumyk.Desk.Appearance.xml`).

use desk_proto::APPEARANCE_PATH;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use super::solar::Scheme;
use super::{Shared, lock, wallpaper};
use crate::schedule::Scheduler;
use crate::trust::Gate;

pub struct AppearanceIface {
    pub eng: Shared,
    pub scheduler: Scheduler,
    /// The live "accent from wallpaper" mode.
    pub wallpaper: wallpaper::Shared,
    /// Only GNOME Shell and desktop-themes-rs binaries may read or change appearance state (ADR 0008).
    pub gate: Gate,
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Appearance")]
impl AppearanceIface {
    #[zbus(out_args(
        "mode",
        "scheme",
        "next_transition",
        "location_label",
        "polar",
        "overridden"
    ))]
    async fn status(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<(String, String, String, String, bool, bool)> {
        self.gate.check(conn, Some(&hdr)).await?;
        let st = lock(&self.eng).status();
        Ok((
            st.mode,
            st.scheme,
            st.next_transition,
            st.location_label,
            st.polar,
            st.overridden,
        ))
    }

    async fn set_scheme(
        &self,
        scheme: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let parsed = Scheme::parse(&scheme).ok_or_else(|| {
            zbus::fdo::Error::InvalidArgs(format!(
                "invalid scheme '{scheme}' (expected 'light', 'dark', or 'default')"
            ))
        })?;
        let changed = lock(&self.eng).set_scheme(parsed);
        self.scheduler.trigger();
        if changed {
            let _ = Self::changed(&em).await;
        }
        Ok(())
    }

    async fn recompute(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        let (_, changed) = lock(&self.eng).recompute();
        self.scheduler.trigger();
        if changed {
            let _ = Self::changed(&em).await;
        }
        Ok(())
    }

    #[zbus(out_args("enabled", "seed", "accent", "monochrome", "uri", "error", "palette"))]
    async fn wallpaper_accent(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<(bool, String, String, bool, String, String, String)> {
        self.gate.check(conn, Some(&hdr)).await?;
        let st = wallpaper::lock(&self.wallpaper).status();
        Ok((
            st.enabled,
            st.seed,
            st.accent,
            st.monochrome,
            st.uri,
            st.error,
            st.palette,
        ))
    }

    #[zbus(signal)]
    pub async fn changed(em: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Emit `Changed()` on the session bus if the Appearance object is registered.
pub async fn emit_changed(conn: &zbus::Connection) {
    if let Ok(iface) = conn
        .object_server()
        .interface::<_, AppearanceIface>(APPEARANCE_PATH)
        .await
    {
        let _ = AppearanceIface::changed(iface.signal_emitter()).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use desk_proto::appearance::AppearanceProxy;
    use desk_proto::{APPEARANCE_XML, xml_members};
    use futures_util::StreamExt;
    use zbus::object_server::Interface;

    use super::super::solar::{AppearanceMode, LocalNow};
    use super::super::{AppearanceEngine, Config};
    use super::*;

    fn test_wallpaper() -> wallpaper::Shared {
        let path = std::env::temp_dir().join(format!(
            "deskd-appearance-iface-{}/wallpaper-accent.json",
            std::process::id()
        ));
        Arc::new(Mutex::new(wallpaper::WallpaperEngine::new(
            wallpaper::Config::default(),
            path,
        )))
    }

    fn test_engine() -> Shared {
        let eng = AppearanceEngine::new(
            Config {
                mode: AppearanceMode::AutoTime,
                dark_from: "20:00".into(),
                light_from: "07:00".into(),
                location: "Kyiv".into(),
                twilight_offset: 0,
                override_until_next: true,
            },
            Scheme::Light,
            |_| {},
            || LocalNow {
                year: 2026,
                month: 6,
                day: 21,
                hour: 21,
                minute: 0,
                second: 0,
                tz_offset_min: 180,
            },
            || Some("Europe/Kyiv".into()),
        );
        Arc::new(Mutex::new(eng))
    }

    #[test]
    fn appearance_introspection_matches_xml() {
        let mut xml = String::new();
        AppearanceIface {
            eng: test_engine(),
            scheduler: Scheduler::inert(),
            wallpaper: test_wallpaper(),
            gate: Gate::open(),
        }
        .introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(APPEARANCE_XML), "{xml}");
    }

    #[test]
    fn api_over_a_private_connection() {
        zbus::block_on(async {
            let eng = test_engine();
            lock(&eng).tick();
            let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
            let guid = zbus::Guid::generate();
            let server = zbus::connection::Builder::unix_stream(a)
                .server(guid)
                .p2p()
                .serve_at(
                    APPEARANCE_PATH,
                    AppearanceIface {
                        eng: eng.clone(),
                        scheduler: Scheduler::inert(),
                        wallpaper: test_wallpaper(),
                        gate: Gate::open(),
                    },
                )
                .build();
            let client = zbus::connection::Builder::unix_stream(b).p2p().build();
            let (_server, client) = futures_util::future::try_join(server, client)
                .await
                .unwrap();

            let p = AppearanceProxy::builder(&client)
                .destination("io.github.OlehDzhumyk.Desk")
                .build()
                .await
                .unwrap();

            let (mode, scheme, next, loc, polar, overridden) = p.status().await.unwrap();
            assert_eq!(mode, "auto-time");
            assert_eq!(scheme, "dark");
            assert_eq!(next, "07:00");
            assert_eq!(loc, "Kyiv");
            assert!(!polar);
            assert!(!overridden);

            let mut changed = p.receive_changed().await.unwrap();
            p.set_scheme("light").await.unwrap();
            assert!(changed.next().await.is_some());

            let (_, scheme, _, _, _, overridden) = p.status().await.unwrap();
            assert_eq!(scheme, "light");
            assert!(overridden);

            let err = p.set_scheme("invalid").await.unwrap_err();
            assert!(err.to_string().contains("InvalidArgs"), "{err}");

            p.recompute().await.unwrap();

            let (enabled, _, accent, _, _, error, palette) = p.wallpaper_accent().await.unwrap();
            assert!(!enabled);
            assert_eq!(
                (accent.as_str(), error.as_str(), palette.as_str()),
                ("", "", "")
            );
        });
    }
}
