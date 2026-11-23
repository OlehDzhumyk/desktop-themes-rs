// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.Ocr` (`data/dbus/io.github.OlehDzhumyk.Desk.Ocr.xml`)
//! and the recognition it serves: settings, the region size limit, the language fallback and the
//! one-at-a-time guard. The Tesseract child runs on its own thread, so a 20 s recognition never
//! holds the bus executor.

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Poll, Waker};
use std::time::Duration;

use desk_proto::ocr::PREVIEW_CHARS;
use zbus::message::Header;

use super::{
    OcrError, clean_text, filter_languages, installed_languages, png_dimensions, preview_line,
    run_tesseract,
};
use crate::trust::Gate;

/// `tesseract --list-langs` and `--version` answer at once; this only bounds a broken binary.
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// The language that is tried when none of the requested ones has data (AC-OCR-08).
const FALLBACK_LANGUAGE: &str = "eng";

/// The engine's part of `io.github.OlehDzhumyk.Desk.Ocr` (`record-in-history` is the bridge's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub languages: Vec<String>,
    pub keep_line_breaks: bool,
    pub max_megapixels: u32,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            enabled: true,
            languages: vec!["ukr".into(), "eng".into()],
            keep_line_breaks: true,
            max_megapixels: 20,
        }
    }
}

/// What `Recognize` returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recognized {
    pub text: String,
    pub preview: String,
    /// Requested language codes without installed data.
    pub missing: Vec<String>,
}

/// The errors of `Recognize`; the names are `io.github.OlehDzhumyk.Desk.Ocr.Error.<variant>`
/// (`desk_proto::ocr::ERROR_PREFIX`), so the bridge can tell them apart.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "io.github.OlehDzhumyk.Desk.Ocr.Error")]
pub enum RecognizeError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Disabled(String),
    InvalidImage(String),
    TooLarge(String),
    Busy(String),
    MissingEngine(String),
    NoLanguages(String),
    Timeout(String),
    Failed(String),
}

impl From<OcrError> for RecognizeError {
    fn from(e: OcrError) -> Self {
        let text = e.to_string();
        match e {
            OcrError::MissingEngine => {
                RecognizeError::MissingEngine(format!("{text}: install the tesseract package"))
            }
            OcrError::NoLanguages => RecognizeError::NoLanguages(text),
            OcrError::Timeout => RecognizeError::Timeout(text),
            OcrError::Failed(_) | OcrError::Io(_) => RecognizeError::Failed(text),
        }
    }
}

/// Holds the one-at-a-time flag while a recognition runs; dropping it (also while a panic
/// unwinds) frees the flag.
struct BusyGuard<'a>(&'a AtomicBool);

impl<'a> BusyGuard<'a> {
    fn take(flag: &'a AtomicBool) -> Option<BusyGuard<'a>> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| BusyGuard(flag))
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The recognition engine: where Tesseract is, where the images go, the settings in force.
pub struct Ocr {
    bin: PathBuf,
    runtime_dir: PathBuf,
    timeout: Duration,
    config: Mutex<Config>,
    busy: AtomicBool,
}

pub type Shared = Arc<Ocr>;

impl Ocr {
    /// `bin` is looked up in `PATH` when it has no slash; images go to `<runtime_dir>/ocr/`.
    pub fn new(bin: PathBuf, runtime_dir: PathBuf, timeout: Duration, config: Config) -> Ocr {
        Ocr {
            bin,
            runtime_dir,
            timeout,
            config: Mutex::new(config),
            busy: AtomicBool::new(false),
        }
    }

    fn config_lock(&self) -> MutexGuard<'_, Config> {
        self.config.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn config(&self) -> Config {
        self.config_lock().clone()
    }

    pub fn set_config(&self, config: Config) {
        *self.config_lock() = config;
    }

    /// True when the binary starts (a failing `--version` still means it exists).
    pub fn is_available(&self) -> bool {
        !matches!(
            installed_languages(&self.bin, QUERY_TIMEOUT),
            Err(OcrError::MissingEngine)
        )
    }

    /// Codes with installed data; empty without Tesseract.
    pub fn available_languages(&self) -> Vec<String> {
        installed_languages(&self.bin, QUERY_TIMEOUT).unwrap_or_default()
    }

    /// Recognise the text of a PNG region of `width` × `height` pixels.
    ///
    /// # Errors
    /// See [`RecognizeError`]; nothing is written for `Disabled`, `InvalidImage`, `TooLarge` and
    /// `Busy`, and the image file is gone when the call returns in every case (AC-OCR-04).
    pub fn recognize(
        &self,
        png: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Recognized, RecognizeError> {
        let cfg = self.config();
        if !cfg.enabled {
            return Err(RecognizeError::Disabled(
                "the text extractor is turned off".into(),
            ));
        }
        let (w, h) = png_dimensions(png)
            .ok_or_else(|| RecognizeError::InvalidImage("not a PNG image".into()))?;
        if (w, h) != (width, height) || w == 0 || h == 0 {
            return Err(RecognizeError::InvalidImage(format!(
                "the PNG image is {w}×{h}, not {width}×{height}"
            )));
        }
        let max = cfg.max_megapixels.clamp(1, 50);
        if u64::from(w) * u64::from(h) > u64::from(max) * 1_000_000 {
            return Err(RecognizeError::TooLarge(format!(
                "the region {w}×{h} is larger than {max} megapixels"
            )));
        }
        let Some(_busy) = BusyGuard::take(&self.busy) else {
            return Err(RecognizeError::Busy(
                "another recognition is running".into(),
            ));
        };

        let installed = installed_languages(&self.bin, QUERY_TIMEOUT)?;
        let (mut langs, missing) = filter_languages(&cfg.languages, &installed);
        if langs.is_empty() && installed.iter().any(|l| l == FALLBACK_LANGUAGE) {
            langs.push(FALLBACK_LANGUAGE.to_string());
        }
        if langs.is_empty() {
            return Err(RecognizeError::NoLanguages(if missing.is_empty() {
                "no Tesseract language data is installed".into()
            } else {
                format!("no data for {}", missing.join(", "))
            }));
        }
        let raw = run_tesseract(&self.bin, png, &langs, &self.runtime_dir, self.timeout)?;
        let text = clean_text(&raw, cfg.keep_line_breaks);
        let preview = preview_line(&text, PREVIEW_CHARS);
        Ok(Recognized {
            text,
            preview,
            missing,
        })
    }
}

/// Run `f` on its own thread and wait for it without blocking the executor. `None` when `f`
/// panicked (the panic stays in that thread; the caller answers with an error).
fn unblock<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> impl Future<Output = Option<T>> {
    struct Slot<T> {
        value: Option<Option<T>>,
        waker: Option<Waker>,
    }
    fn lock<T>(m: &Mutex<Slot<T>>) -> MutexGuard<'_, Slot<T>> {
        m.lock().unwrap_or_else(|p| p.into_inner())
    }
    let slot = Arc::new(Mutex::new(Slot {
        value: None,
        waker: None,
    }));
    let tx = slot.clone();
    std::thread::spawn(move || {
        let v = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok();
        let waker = {
            let mut s = lock(&tx);
            s.value = Some(v);
            s.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    });
    std::future::poll_fn(move |cx| {
        let mut s = lock(&slot);
        match s.value.take() {
            Some(v) => Poll::Ready(v),
            None => {
                s.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    })
}

pub struct OcrIface {
    pub ocr: Shared,
    /// Only GNOME Shell and desktop-themes-rs binaries may send screen pixels (ADR 0008).
    pub gate: Gate,
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Ocr")]
impl OcrIface {
    #[zbus(out_args("text", "preview", "missing_langs"))]
    async fn recognize(
        &self,
        png: Vec<u8>,
        width: u32,
        height: u32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> Result<(String, String, Vec<String>), RecognizeError> {
        self.gate
            .check(conn, Some(&hdr))
            .await
            .map_err(zbus::Error::from)?;
        let ocr = self.ocr.clone();
        let r = unblock(move || ocr.recognize(&png, width, height))
            .await
            .unwrap_or_else(|| Err(RecognizeError::Failed("internal error".into())))?;
        Ok((r.text, r.preview, r.missing))
    }

    async fn available_languages(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<Vec<String>> {
        self.gate.check(conn, Some(&hdr)).await?;
        let ocr = self.ocr.clone();
        Ok(unblock(move || ocr.available_languages())
            .await
            .unwrap_or_default())
    }

    async fn is_available(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<bool> {
        self.gate.check(conn, Some(&hdr)).await?;
        let ocr = self.ocr.clone();
        Ok(unblock(move || ocr.is_available()).await.unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use desk_proto::ocr::{ERROR_PREFIX, OcrProxy};
    use desk_proto::{OCR_PATH, OCR_XML, xml_members};
    use zbus::object_server::Interface;

    use super::super::testutil::{Sandbox, s, spawn_lock};
    use super::*;

    /// A minimal PNG header of `w` × `h` (the fake Tesseract never decodes it).
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13,
        ];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b
    }

    /// A fake Tesseract with the `langs` data installed that runs `body` to recognise; the
    /// arguments of each recognition are appended to `<dir>/args`.
    fn fake(sb: &Sandbox, langs: &[&str], body: &str) -> PathBuf {
        let list = langs.join("\\n");
        sb.script(&format!(
            "if [ \"$1\" = --list-langs ]; then printf 'List of available languages ({n}):\\n{list}\\n'; exit 0; fi\n\
             printf '%s\\n' \"$4\" >> '{args}'\n{body}",
            n = langs.len(),
            args = sb.dir.join("args").display()
        ))
    }

    fn ocr(bin: &Path, sb: &Sandbox, cfg: Config) -> Ocr {
        Ocr::new(
            bin.to_path_buf(),
            sb.runtime(),
            super::super::DEFAULT_TIMEOUT,
            cfg,
        )
    }

    fn args(sb: &Sandbox) -> String {
        std::fs::read_to_string(sb.dir.join("args")).unwrap_or_default()
    }

    #[test]
    fn recognize_returns_clean_text_preview_and_deletes_the_image() {
        let _lock = spawn_lock();
        let sb = Sandbox::new("svc-ok");
        let bin = fake(&sb, &["eng", "ukr"], "printf 'Привіт,\\nworld\\n\\n\\f'");
        let eng = ocr(&bin, &sb, Config::default());
        let r = eng.recognize(&png(40, 20), 40, 20).unwrap();
        assert_eq!(r.text, "Привіт,\nworld");
        assert_eq!(r.preview, "Привіт,");
        assert!(r.missing.is_empty());
        assert_eq!(args(&sb), "ukr+eng\n");
        assert!(sb.leftovers().is_empty());

        // keep-line-breaks off joins the lines of a paragraph
        eng.set_config(Config {
            keep_line_breaks: false,
            ..Config::default()
        });
        let r = eng.recognize(&png(40, 20), 40, 20).unwrap();
        assert_eq!(r.text, "Привіт, world");
        assert!(eng.is_available());
        assert_eq!(eng.available_languages(), s(&["eng", "ukr"]));
    }

    /// AC-OCR-02: no text gives an empty text and preview (the bridge leaves the clipboard).
    #[test]
    fn recognize_without_text_is_empty() {
        let _lock = spawn_lock();
        let sb = Sandbox::new("svc-empty");
        let bin = fake(&sb, &["eng"], "printf ' \\n\\f'");
        let r = ocr(&bin, &sb, Config::default())
            .recognize(&png(10, 10), 10, 10)
            .unwrap();
        assert_eq!((r.text.as_str(), r.preview.as_str()), ("", ""));
        assert!(sb.leftovers().is_empty());
    }

    /// AC-OCR-08: Ukrainian data missing, English still works and the missing code is named.
    #[test]
    fn recognize_runs_with_available_languages_and_names_the_missing() {
        let _lock = spawn_lock();
        let sb = Sandbox::new("svc-langs");
        let bin = fake(&sb, &["eng"], "echo hello");
        let eng = ocr(&bin, &sb, Config::default());
        let r = eng.recognize(&png(10, 10), 10, 10).unwrap();
        assert_eq!(r.text, "hello");
        assert_eq!(r.missing, s(&["ukr"]));
        // none of the requested languages: English as the fallback, still reported
        eng.set_config(Config {
            languages: s(&["ukr", "pol"]),
            ..Config::default()
        });
        let r = eng.recognize(&png(10, 10), 10, 10).unwrap();
        assert_eq!(r.missing, s(&["ukr", "pol"]));
        assert_eq!(args(&sb), "eng\neng\n");

        // no usable data at all
        let bin = fake(&sb, &["deu"], "echo never");
        let err = ocr(&bin, &sb, Config::default())
            .recognize(&png(10, 10), 10, 10)
            .unwrap_err();
        assert!(
            matches!(&err, RecognizeError::NoLanguages(m) if m.contains("ukr, eng")),
            "{err:?}"
        );
        assert!(sb.leftovers().is_empty());
    }

    #[test]
    fn recognize_refuses_regions_above_max_megapixels_before_running() {
        let _lock = spawn_lock();
        let sb = Sandbox::new("svc-large");
        let bin = fake(&sb, &["eng"], "echo never");
        let eng = ocr(
            &bin,
            &sb,
            Config {
                max_megapixels: 1,
                ..Config::default()
            },
        );
        // exactly 1 000 000 pixels pass, one row more does not
        assert!(eng.recognize(&png(1000, 1000), 1000, 1000).is_ok());
        let err = eng.recognize(&png(1000, 1001), 1000, 1001).unwrap_err();
        assert!(matches!(err, RecognizeError::TooLarge(_)), "{err:?}");
        assert_eq!(args(&sb), "eng\n");
        assert!(sb.leftovers().is_empty());
    }

    #[test]
    fn recognize_refuses_invalid_images_and_when_disabled() {
        let sb = Sandbox::new("svc-invalid");
        let eng = ocr(&sb.dir.join("none"), &sb, Config::default());
        for (bytes, w, h) in [
            (b"not a png".to_vec(), 1, 1),
            (png(10, 10), 10, 11),
            (png(0, 10), 0, 10),
        ] {
            let err = eng.recognize(&bytes, w, h).unwrap_err();
            assert!(matches!(err, RecognizeError::InvalidImage(_)), "{err:?}");
        }
        eng.set_config(Config {
            enabled: false,
            ..Config::default()
        });
        let err = eng.recognize(&png(10, 10), 10, 10).unwrap_err();
        assert!(matches!(err, RecognizeError::Disabled(_)), "{err:?}");
        assert!(!sb.runtime().exists());
    }

    /// AC-OCR-03 on the engine side: no binary, no recognition and no file.
    #[test]
    fn missing_tesseract_is_reported() {
        let sb = Sandbox::new("svc-missing");
        let eng = ocr(&sb.dir.join("no-such-tesseract"), &sb, Config::default());
        assert!(!eng.is_available());
        assert!(eng.available_languages().is_empty());
        let err = eng.recognize(&png(10, 10), 10, 10).unwrap_err();
        assert!(
            matches!(&err, RecognizeError::MissingEngine(m) if m.contains("tesseract")),
            "{err:?}"
        );
        assert!(sb.leftovers().is_empty());
    }

    /// One recognition at a time: a second call while one runs is refused, and the guard is
    /// free again after it ends. The fake blocks on FIFOs, so the test waits on events only.
    #[test]
    fn a_second_recognition_while_one_runs_is_busy() {
        let _lock = spawn_lock();
        let sb = Sandbox::new("svc-busy");
        let (started, release) = (sb.dir.join("started"), sb.dir.join("release"));
        for f in [&started, &release] {
            assert!(Command::new("mkfifo").arg(f).status().unwrap().success());
        }
        let bin = fake(
            &sb,
            &["eng"],
            &format!(
                "echo go > '{}'\ncat '{}' > /dev/null\necho done",
                started.display(),
                release.display()
            ),
        );
        let eng = Arc::new(ocr(&bin, &sb, Config::default()));
        let first = {
            let eng = eng.clone();
            std::thread::spawn(move || eng.recognize(&png(10, 10), 10, 10))
        };
        // opening the FIFO blocks until the fake runs, so the first call holds the guard now
        std::fs::read_to_string(&started).unwrap();
        let err = eng.recognize(&png(10, 10), 10, 10).unwrap_err();
        assert!(matches!(err, RecognizeError::Busy(_)), "{err:?}");
        std::fs::write(&release, "x").unwrap();
        assert_eq!(first.join().unwrap().unwrap().text, "done");
        assert!(sb.leftovers().is_empty());

        // the guard is free again: the next call runs (and blocks on the FIFOs once more)
        let second = {
            let eng = eng.clone();
            std::thread::spawn(move || eng.recognize(&png(10, 10), 10, 10))
        };
        std::fs::read_to_string(&started).unwrap();
        std::fs::write(&release, "x").unwrap();
        assert!(second.join().unwrap().is_ok());
    }

    #[test]
    fn errors_map_to_their_names() {
        use zbus::DBusError;
        let cases = [
            (
                RecognizeError::from(OcrError::MissingEngine),
                "MissingEngine",
            ),
            (RecognizeError::from(OcrError::NoLanguages), "NoLanguages"),
            (RecognizeError::from(OcrError::Timeout), "Timeout"),
            (RecognizeError::from(OcrError::Failed("x".into())), "Failed"),
            (RecognizeError::Busy(String::new()), "Busy"),
            (RecognizeError::TooLarge(String::new()), "TooLarge"),
        ];
        for (e, name) in cases {
            assert_eq!(e.name().as_str(), format!("{ERROR_PREFIX}.{name}"));
        }
        // an AccessDenied from the gate keeps its own name
        let denied = RecognizeError::from(zbus::Error::from(zbus::fdo::Error::AccessDenied(
            "no".into(),
        )));
        assert_eq!(
            denied.name().as_str(),
            "org.freedesktop.DBus.Error.AccessDenied"
        );
    }

    #[test]
    fn ocr_introspection_matches_xml() {
        let sb = Sandbox::new("svc-introspect");
        let mut xml = String::new();
        OcrIface {
            ocr: Arc::new(ocr(&sb.dir.join("none"), &sb, Config::default())),
            gate: Gate::open(),
        }
        .introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(OCR_XML), "{xml}");
    }

    #[test]
    fn api_over_a_private_connection() {
        let _lock = spawn_lock();
        let sb = Sandbox::new("svc-p2p");
        let bin = fake(&sb, &["eng"], "echo 'over the bus'");
        let eng = Arc::new(ocr(
            &bin,
            &sb,
            Config {
                max_megapixels: 1,
                ..Config::default()
            },
        ));
        zbus::block_on(async {
            let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
            let server = zbus::connection::Builder::unix_stream(a)
                .server(zbus::Guid::generate())
                .p2p()
                .serve_at(
                    OCR_PATH,
                    OcrIface {
                        ocr: eng,
                        gate: Gate::open(),
                    },
                )
                .build();
            let client = zbus::connection::Builder::unix_stream(b).p2p().build();
            let (_server, client) = futures_util::future::try_join(server, client)
                .await
                .unwrap();
            let proxy = OcrProxy::builder(&client)
                .destination("io.github.OlehDzhumyk.Desk")
                .build()
                .await
                .unwrap();
            assert!(proxy.is_available().await.unwrap());
            assert_eq!(proxy.available_languages().await.unwrap(), s(&["eng"]));
            let (text, preview, missing) = proxy.recognize(&png(20, 10), 20, 10).await.unwrap();
            assert_eq!(
                (text.as_str(), preview.as_str()),
                ("over the bus", "over the bus")
            );
            assert_eq!(missing, s(&["ukr"]));
            let err = proxy
                .recognize(&png(2000, 2000), 2000, 2000)
                .await
                .unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("{ERROR_PREFIX}.TooLarge")),
                "{err}"
            );
        });
        assert!(sb.leftovers().is_empty());
    }
}
