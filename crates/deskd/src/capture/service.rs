// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.Capture`
//! (`data/dbus/io.github.OlehDzhumyk.Desk.Capture.xml`).
//!
//! A call does its work synchronously: saving is a check and one write (milliseconds for a
//! 1080p capture); annotating decodes and encodes once and runs only when the user saves in the
//! editor.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use zbus::fdo;
use zbus::message::Header;

use super::{Capture, CaptureError, local_stamp};
use crate::trust::Gate;

pub type Shared = Arc<Mutex<Capture>>;

pub fn lock(m: &Shared) -> MutexGuard<'_, Capture> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct CaptureIface {
    pub engine: Shared,
    /// Only GNOME Shell and desktop-themes-rs binaries may write files through deskd (ADR 0008).
    pub gate: Gate,
}

fn to_fdo(e: CaptureError) -> fdo::Error {
    use desk_capture::Error as Core;
    match e {
        CaptureError::Disabled => fdo::Error::NotSupported(e.to_string()),
        CaptureError::Core(Core::Io(io)) => fdo::Error::IOError(io.to_string()),
        CaptureError::BadPath(_) | CaptureError::Core(_) => fdo::Error::InvalidArgs(e.to_string()),
    }
}

fn path_text(p: PathBuf) -> fdo::Result<String> {
    p.into_os_string()
        .into_string()
        .map_err(|_| fdo::Error::IOError("the saved path is not valid UTF-8".into()))
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Capture")]
impl CaptureIface {
    #[zbus(out_args("path"))]
    async fn save_screenshot(
        &self,
        png_bytes: Vec<u8>,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        let stamp = local_stamp();
        let path = lock(&self.engine)
            .save(&png_bytes, &stamp)
            .map_err(to_fdo)?;
        path_text(path)
    }

    #[zbus(out_args("saved_path"))]
    async fn apply_annotations(
        &self,
        path: String,
        annotations_json: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        let saved = lock(&self.engine)
            .annotate(&path, &annotations_json)
            .map_err(to_fdo)?;
        path_text(saved)
    }
}

#[cfg(test)]
mod tests {
    use desk_capture::{Image, codec};
    use desk_proto::capture::CaptureProxy;
    use desk_proto::{CAPTURE_PATH, CAPTURE_XML, xml_members};
    use zbus::object_server::Interface;

    use super::super::tests::tmp;
    use super::super::{Config, Dirs};
    use super::*;

    fn engine(home: &std::path::Path, enabled: bool) -> Shared {
        Arc::new(Mutex::new(Capture::new(
            Config {
                enabled,
                save_folder: String::new(),
            },
            Dirs {
                home: home.to_path_buf(),
                pictures: None,
            },
        )))
    }

    /// CRC-32 (IEEE) for hand-made PNG chunks.
    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    /// A small valid PNG whose IHDR is changed to claim `w`×`h` (CRC fixed).
    fn claiming(w: u32, h: u32) -> Vec<u8> {
        let mut png = codec::encode(&Image::filled(1, 1, [0, 0, 0, 255])).unwrap();
        png[16..20].copy_from_slice(&w.to_be_bytes());
        png[20..24].copy_from_slice(&h.to_be_bytes());
        let crc = crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        png
    }

    #[test]
    fn capture_introspection_matches_xml() {
        let home = tmp("introspect");
        let mut xml = String::new();
        CaptureIface {
            engine: engine(&home, true),
            gate: Gate::open(),
        }
        .introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(CAPTURE_XML), "{xml}");
        assert!(xml.contains("type=\"ay\""), "{xml}");
        let _ = std::fs::remove_dir_all(&home);
    }

    async fn connect(eng: Shared) -> (zbus::Connection, zbus::Connection) {
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = zbus::connection::Builder::unix_stream(a)
            .server(zbus::Guid::generate())
            .p2p()
            .serve_at(
                CAPTURE_PATH,
                CaptureIface {
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

    async fn proxy_on(client: &zbus::Connection) -> CaptureProxy<'_> {
        CaptureProxy::builder(client)
            .destination("io.github.OlehDzhumyk.Desk")
            .build()
            .await
            .unwrap()
    }

    #[test]
    fn save_then_redact_over_a_private_connection() {
        let home = tmp("p2p");
        zbus::block_on(async {
            let (_server, client) = connect(engine(&home, true)).await;
            let proxy = proxy_on(&client).await;

            let mut orig = Image::filled(16, 8, [250, 250, 250, 255]);
            for x in 0..16 {
                orig.set_pixel(x, 3, [0, 0, 0, 255]); // a line of "text"
            }
            let png = codec::encode(&orig).unwrap();
            let path = proxy.save_screenshot(&png).await.unwrap();
            let p = std::path::Path::new(&path);
            assert_eq!(p.parent().unwrap(), home.join("Pictures/Screenshots"));
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                name.starts_with("Screenshot ") && name.ends_with(".png"),
                "{name}"
            );

            let json = r#"[{"kind": "redact", "x": 0, "y": 0, "width": 8, "height": 8,
                            "style": {"solid": [20, 20, 20, 255]}},
                           {"kind": "redact", "x": 8, "y": 0, "width": 8, "height": 8,
                            "style": {"pixelate": 8}}]"#;
            assert_eq!(proxy.apply_annotations(&path, json).await.unwrap(), path);
            let out = codec::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!((out.width, out.height), (16, 8));
            for y in 0..8 {
                for x in 0..16 {
                    assert_ne!(out.pixel(x, y), [0, 0, 0, 255], "({x},{y}) kept the text");
                }
            }
            assert_eq!(out.pixel(2, 3), [20, 20, 20, 255]);

            let err = proxy
                .apply_annotations(&path, "[{\"kind\": 1}]")
                .await
                .unwrap_err();
            assert!(err.to_string().contains("InvalidArgs"), "{err}");
        });
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn refusals_over_a_private_connection() {
        let home = tmp("refuse");
        zbus::block_on(async {
            let (_server, client) = connect(engine(&home, true)).await;
            let proxy = proxy_on(&client).await;
            // 20000 × 20000 = 400 MP: refused from the header, nothing decoded or written
            let err = proxy
                .save_screenshot(&claiming(20_000, 20_000))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("InvalidArgs"), "{err}");
            assert!(err.to_string().contains("too large"), "{err}");
            let err = proxy.save_screenshot(b"not a png").await.unwrap_err();
            assert!(err.to_string().contains("InvalidArgs"), "{err}");
            assert!(!home.join("Pictures").exists());

            let (_server, client) = connect(engine(&home, false)).await;
            let proxy = proxy_on(&client).await;
            let png = codec::encode(&Image::filled(1, 1, [0, 0, 0, 255])).unwrap();
            let err = proxy.save_screenshot(&png).await.unwrap_err();
            assert!(err.to_string().contains("NotSupported"), "{err}");
            assert!(!home.join("Pictures").exists());
        });
        let _ = std::fs::remove_dir_all(&home);
    }
}
