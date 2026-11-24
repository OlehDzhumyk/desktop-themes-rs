// SPDX-License-Identifier: GPL-3.0-or-later
//! The one-shot commands, bound to keys in the compositor: `--capture-region` (Win+Shift+S),
//! `--ocr-region` (Win+Shift+T) and `--pick-color` (Win+Shift+C). Each selects a region or a
//! point with `slurp`, captures it with `grim`, hands the pixels to deskd (`Capture`, `Ocr`,
//! `ColorPicker`, with the checks of ADR 0008), copies the result with `wl-copy`, says what
//! happened in the display of the running shell (`show-osd`, nothing when no shell runs) and
//! exits. They start no GTK and no window, so they answer at once.
//!
//! A cancelled selection (`Esc`) exits quietly with success.

use desk_proto as proto;
use desk_settings::tr;
use gtk::glib::{self, ExitCode};
use gtk::prelude::*;

use crate::deskd::{self, CallError};
use crate::region::{self, ToolError};
use crate::{launch, osd};

/// A one-shot command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    CaptureRegion,
    OcrRegion,
    PickColor,
}

/// How long deskd may take (ms): OCR runs Tesseract for up to 20 s.
const SAVE_TIMEOUT_MS: i32 = 15_000;
const OCR_TIMEOUT_MS: i32 = 30_000;
const COLOR_TIMEOUT_MS: i32 = 5_000;

/// What `Ocr.Recognize` refused, by its D-Bus error name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcrFailure {
    Disabled,
    InvalidImage,
    TooLarge,
    Busy,
    MissingEngine,
    NoLanguages,
    Timeout,
    Failed,
    /// deskd is not running (or refused): there is no local fallback for OCR.
    Unavailable,
}

/// The [`OcrFailure`] of a failed `Recognize`.
pub fn ocr_failure(e: &CallError) -> OcrFailure {
    let CallError::Remote {
        name: Some(name), ..
    } = e
    else {
        return match e {
            CallError::Unavailable => OcrFailure::Unavailable,
            CallError::Remote { .. } => OcrFailure::Failed,
        };
    };
    match name.strip_prefix("io.github.OlehDzhumyk.Desk.Ocr.Error.") {
        Some("Disabled") => OcrFailure::Disabled,
        Some("InvalidImage") => OcrFailure::InvalidImage,
        Some("TooLarge") => OcrFailure::TooLarge,
        Some("Busy") => OcrFailure::Busy,
        Some("MissingEngine") => OcrFailure::MissingEngine,
        Some("NoLanguages") => OcrFailure::NoLanguages,
        Some("Timeout") => OcrFailure::Timeout,
        _ if name == "org.freedesktop.DBus.Error.AccessDenied" => OcrFailure::Unavailable,
        _ => OcrFailure::Failed,
    }
}

fn ocr_failure_text(f: OcrFailure) -> String {
    match f {
        OcrFailure::Disabled => tr!("Screen text recognition is turned off"),
        OcrFailure::InvalidImage => tr!("The capture is not a valid image"),
        OcrFailure::TooLarge => tr!("The selected region is too large"),
        OcrFailure::Busy => tr!("Text recognition is busy, try again"),
        OcrFailure::MissingEngine => tr!("Text recognition needs Tesseract (not installed)"),
        OcrFailure::NoLanguages => tr!("No text recognition language is installed"),
        OcrFailure::Timeout => tr!("Text recognition took too long"),
        OcrFailure::Failed => tr!("Text recognition failed"),
        OcrFailure::Unavailable => tr!("The desktop service is not running"),
    }
}

/// `Capture.SaveScreenshot` failed in a way the local save can take over: deskd is missing or
/// the screenshot feature is turned off (`NotSupported`). A refused image is not saved anyway.
pub fn save_locally_after(e: &CallError) -> bool {
    match e {
        CallError::Unavailable => true,
        CallError::Remote { name, .. } => matches!(
            name.as_deref(),
            Some(
                "org.freedesktop.DBus.Error.NotSupported"
                    | "org.freedesktop.DBus.Error.AccessDenied"
            )
        ),
    }
}

fn tool_text(e: &ToolError) -> String {
    match e {
        ToolError::Missing(tool) => tr!("{tool} is not installed", tool = tool),
        ToolError::Failed(tool) => tr!("{tool} failed", tool = tool),
        ToolError::TooLarge => tr!("The selected region is too large"),
    }
}

/// Show `text` in the display of the running shell (best effort: no shell, no display).
fn toast(icon: &str, text: &str) {
    eprintln!("desk-shell: {text}");
    crate::remote_activate(&[(crate::SHOW_OSD, Some(osd::encode(icon, text).to_variant()))]);
}

fn fail(text: &str) -> ExitCode {
    toast("dialog-warning-symbolic", text);
    ExitCode::FAILURE
}

/// Select and capture (`point`: one pixel as PPM, else a PNG); `Ok(None)`: cancelled.
fn grab(point: bool) -> Result<Option<Vec<u8>>, ToolError> {
    let Some(region) = region::select(point)? else {
        return Ok(None);
    };
    region::capture(&region, point).map(Some)
}

/// Run `task` and exit.
pub fn run(task: Task) -> ExitCode {
    match task {
        Task::CaptureRegion => capture_region(),
        Task::OcrRegion => ocr_region(),
        Task::PickColor => pick_color(),
    }
}

/// The local save of a screenshot (deskd missing or the feature off).
fn save_locally(png: &[u8]) -> std::io::Result<std::path::PathBuf> {
    let home = glib::home_dir();
    let special = glib::user_special_dir(glib::UserDirectory::Pictures);
    let env = std::env::var("XDG_PICTURES_DIR").ok();
    let dir = region::screenshot_dir(env.as_deref(), special.as_deref(), &home);
    let stamp = glib::DateTime::now_local()
        .and_then(|t| t.format("%Y-%m-%d %H-%M-%S"))
        .map_or_else(|_| "capture".to_owned(), |s| s.to_string());
    region::save_new(&dir, &stamp, png)
}

fn capture_region() -> ExitCode {
    let png = match grab(false) {
        Ok(Some(png)) => png,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => return fail(&tool_text(&e)),
    };
    let saved = deskd::call_blocking(
        proto::CAPTURE_PATH,
        proto::CAPTURE_IFACE,
        "SaveScreenshot",
        Some((png.as_slice(),).to_variant()),
        SAVE_TIMEOUT_MS,
    );
    let path = match saved.map(|v| deskd::single_string(&v)) {
        Ok(Some(path)) => Some(path),
        Ok(None) => None,
        Err(e) if save_locally_after(&e) => match save_locally(&png) {
            Ok(p) => Some(p.to_string_lossy().into_owned()),
            Err(e) => {
                eprintln!("desk-shell: cannot save the screenshot: {e}");
                None
            }
        },
        Err(_) => None,
    };
    // the image goes on the clipboard even when it could not be saved
    let copied = region::copy(&png, Some("image/png"));
    let Some(path) = path else {
        return fail(&tr!("The screenshot could not be saved"));
    };
    toast(
        "camera-photo-symbolic",
        &if copied.is_ok() {
            tr!("Screenshot saved and copied")
        } else {
            tr!("Screenshot saved")
        },
    );
    launch::spawn_detached(&[&launch::themes_binary(), "capture-edit", &path]);
    ExitCode::SUCCESS
}

fn ocr_region() -> ExitCode {
    let png = match grab(false) {
        Ok(Some(png)) => png,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => return fail(&tool_text(&e)),
    };
    let Some((width, height)) = region::png_size(&png) else {
        return fail(&ocr_failure_text(OcrFailure::InvalidImage));
    };
    let answer = deskd::call_blocking(
        proto::OCR_PATH,
        proto::OCR_IFACE,
        "Recognize",
        Some((png.as_slice(), width, height).to_variant()),
        OCR_TIMEOUT_MS,
    );
    let found = match answer {
        Ok(v) => deskd::ocr_text(&v),
        Err(e) => return fail(&ocr_failure_text(ocr_failure(&e))),
    };
    let Some(found) = found.filter(|f| !f.text.trim().is_empty()) else {
        toast("dialog-information-symbolic", &tr!("No text found"));
        return ExitCode::SUCCESS;
    };
    if let Err(e) = region::copy(found.text.as_bytes(), None) {
        return fail(&tool_text(&e));
    }
    let preview = deskd::preview(&found.preview, 48);
    toast(
        "edit-copy-symbolic",
        &tr!("Text copied: {preview}", preview = preview),
    );
    ExitCode::SUCCESS
}

fn pick_color() -> ExitCode {
    let ppm = match grab(true) {
        Ok(Some(ppm)) => ppm,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => return fail(&tool_text(&e)),
    };
    let (r, g, b) = match region::ppm_pixel(&ppm) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("desk-shell: grim gave no pixel: {e:?}");
            return fail(&tool_text(&ToolError::Failed("grim")));
        }
    };
    let recorded = deskd::call_blocking(
        proto::COLOR_PICKER_PATH,
        proto::COLOR_PICKER_IFACE,
        "Record",
        Some((r, g, b).to_variant()),
        COLOR_TIMEOUT_MS,
    )
    .ok()
    .and_then(|v| deskd::color_record(&v));
    // without deskd the colour is still copied, as #RRGGBB
    let (text, name) = recorded.map_or_else(
        || (region::hex(r, g, b), String::new()),
        |c| (c.formatted, c.name),
    );
    if let Err(e) = region::copy(text.as_bytes(), None) {
        return fail(&tool_text(&e));
    }
    let message = if name.is_empty() {
        tr!("Copied {color}", color = text)
    } else {
        tr!("Copied {color} ({name})", color = text, name = name)
    };
    toast("color-select-symbolic", &message);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str) -> CallError {
        CallError::Remote {
            name: Some(name.into()),
            message: "m".into(),
        }
    }

    #[test]
    fn ocr_errors_map_by_their_d_bus_name() {
        let prefix = "io.github.OlehDzhumyk.Desk.Ocr.Error.";
        for (suffix, want) in [
            ("Disabled", OcrFailure::Disabled),
            ("InvalidImage", OcrFailure::InvalidImage),
            ("TooLarge", OcrFailure::TooLarge),
            ("Busy", OcrFailure::Busy),
            ("MissingEngine", OcrFailure::MissingEngine),
            ("NoLanguages", OcrFailure::NoLanguages),
            ("Timeout", OcrFailure::Timeout),
            ("Failed", OcrFailure::Failed),
            ("Newer", OcrFailure::Failed),
        ] {
            assert_eq!(ocr_failure(&remote(&format!("{prefix}{suffix}"))), want);
        }
        assert_eq!(
            ocr_failure(&remote("org.freedesktop.DBus.Error.AccessDenied")),
            OcrFailure::Unavailable
        );
        assert_eq!(
            ocr_failure(&CallError::Unavailable),
            OcrFailure::Unavailable
        );
        assert_eq!(
            ocr_failure(&CallError::Remote {
                name: None,
                message: "x".into()
            }),
            OcrFailure::Failed
        );
    }

    #[test]
    fn screenshots_are_saved_locally_only_without_deskd() {
        assert!(save_locally_after(&CallError::Unavailable));
        assert!(save_locally_after(&remote(
            "org.freedesktop.DBus.Error.NotSupported"
        )));
        assert!(save_locally_after(&remote(
            "org.freedesktop.DBus.Error.AccessDenied"
        )));
        // a refused image or a full disk is not written by this process either
        assert!(!save_locally_after(&remote(
            "org.freedesktop.DBus.Error.InvalidArgs"
        )));
        assert!(!save_locally_after(&remote(
            "org.freedesktop.DBus.Error.IOError"
        )));
    }
}
