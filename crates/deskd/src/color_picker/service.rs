// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.ColorPicker`
//! (`data/dbus/io.github.OlehDzhumyk.Desk.ColorPicker.xml`).

use std::sync::{Arc, Mutex, MutexGuard};

use desk_proto::COLOR_PICKER_PATH;
use desk_proto::color_picker::ColorPick;
use theme_tokens::color::Rgba;
use theme_tokens::picker;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use super::{ColorPicker, parse_format};
use crate::trust::Gate;

pub type Shared = Arc<Mutex<ColorPicker>>;

pub fn lock(m: &Shared) -> MutexGuard<'_, ColorPicker> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct ColorPickerIface {
    pub engine: Shared,
    /// Only GNOME Shell and desktop-themes-rs binaries may read or change the history (ADR 0008).
    pub gate: Gate,
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.ColorPicker")]
impl ColorPickerIface {
    #[zbus(out_args("formatted", "hex", "name"))]
    async fn record(
        &self,
        r: u8,
        g: u8,
        b: u8,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<(String, String, String)> {
        self.gate.check(conn, Some(&hdr)).await?;
        let (out, remembered) = {
            let mut eng = lock(&self.engine);
            let out = eng.record(Rgba::rgb(r, g, b));
            (out, eng.config().history_size > 0)
        };
        if remembered {
            let _ = Self::changed(&em).await;
        }
        Ok(out)
    }

    async fn list(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<Vec<ColorPick>> {
        self.gate.check(conn, Some(&hdr)).await?;
        Ok(lock(&self.engine).list())
    }

    async fn remove(
        &self,
        hex: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<bool> {
        self.gate.check(conn, Some(&hdr)).await?;
        let removed = lock(&self.engine).remove(&hex);
        if removed {
            let _ = Self::changed(&em).await;
        }
        Ok(removed)
    }

    async fn clear(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        if lock(&self.engine).clear() {
            let _ = Self::changed(&em).await;
        }
        Ok(())
    }

    async fn format(
        &self,
        r: u8,
        g: u8,
        b: u8,
        format: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        let current = lock(&self.engine).config().format;
        let fmt = parse_format(&format, current).ok_or_else(|| {
            zbus::fdo::Error::InvalidArgs(format!("unknown colour format: {format}"))
        })?;
        Ok(picker::format_color(Rgba::rgb(r, g, b), fmt))
    }

    async fn contrast(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<f64> {
        self.gate.check(conn, Some(&hdr)).await?;
        Ok(lock(&self.engine).contrast())
    }

    /// The history changed, or the format or history size did.
    #[zbus(signal)]
    pub async fn changed(em: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Emit `Changed()` if the ColorPicker object is registered.
pub async fn emit(conn: &zbus::Connection) {
    if let Ok(iface) = conn
        .object_server()
        .interface::<_, ColorPickerIface>(COLOR_PICKER_PATH)
        .await
    {
        let _ = ColorPickerIface::changed(iface.signal_emitter()).await;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use desk_proto::color_picker::ColorPickerProxy;
    use desk_proto::{COLOR_PICKER_XML, xml_members};
    use futures_util::StreamExt;
    use zbus::object_server::Interface;

    use super::super::Config;
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deskd-colorsvc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("colors.json")
    }

    fn engine(p: &std::path::Path) -> Shared {
        Arc::new(Mutex::new(ColorPicker::new(
            p.to_path_buf(),
            Config::default(),
        )))
    }

    #[test]
    fn color_picker_introspection_matches_xml() {
        let p = temp_path("introspect");
        let mut xml = String::new();
        ColorPickerIface {
            engine: engine(&p),
            gate: Gate::open(),
        }
        .introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(COLOR_PICKER_XML), "{xml}");
        assert!(xml.contains("a(sssx)"), "{xml}");
    }

    #[test]
    fn api_over_a_private_connection() {
        let p = temp_path("p2p");
        zbus::block_on(async {
            let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
            let server = zbus::connection::Builder::unix_stream(a)
                .server(zbus::Guid::generate())
                .p2p()
                .serve_at(
                    COLOR_PICKER_PATH,
                    ColorPickerIface {
                        engine: engine(&p),
                        gate: Gate::open(),
                    },
                )
                .build();
            let client = zbus::connection::Builder::unix_stream(b).p2p().build();
            let (_server, client) = futures_util::future::try_join(server, client)
                .await
                .unwrap();
            let proxy = ColorPickerProxy::builder(&client)
                .destination("io.github.OlehDzhumyk.Desk")
                .build()
                .await
                .unwrap();
            let mut changed = proxy.receive_changed().await.unwrap();

            let (text, hex, _) = proxy.record(0x33, 0x66, 0x99).await.unwrap();
            assert_eq!((text.as_str(), hex.as_str()), ("#336699", "#336699"));
            assert!(changed.next().await.is_some());
            proxy.record(255, 255, 255).await.unwrap();
            assert!(changed.next().await.is_some());

            let list = proxy.list().await.unwrap();
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].hex, "#FFFFFF");
            assert_eq!(list[0].name, "white");
            assert!(proxy.contrast().await.unwrap() > 1.0);

            assert_eq!(
                proxy.format(0x33, 0x66, 0x99, "rgb").await.unwrap(),
                "rgb(51, 102, 153)"
            );
            assert_eq!(proxy.format(0, 0, 0, "").await.unwrap(), "#000000");
            let err = proxy.format(0, 0, 0, "cmyk").await.unwrap_err();
            assert!(err.to_string().contains("InvalidArgs"), "{err}");

            assert!(proxy.remove("#ffffff").await.unwrap());
            assert!(changed.next().await.is_some());
            assert!(!proxy.remove("#ffffff").await.unwrap());
            proxy.clear().await.unwrap();
            assert!(changed.next().await.is_some());
            assert!(proxy.list().await.unwrap().is_empty());
            assert_eq!(proxy.contrast().await.unwrap(), 0.0);
        });
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
