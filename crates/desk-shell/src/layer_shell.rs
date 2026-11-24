// SPDX-License-Identifier: GPL-3.0-or-later
//! A minimal, run-time binding of `libgtk4-layer-shell.so.0` (ADR 0012).
//!
//! The library is opened with `dlopen`, so the build needs no `-devel` package and the binary
//! starts on systems without it. When the library is absent, the compositor has no
//! `wlr-layer-shell` (GNOME), or `--windowed` is given, [`place`] does nothing and the surface
//! stays a normal GTK 4 window: developers can preview the shell anywhere.
//!
//! gtk4-layer-shell must come before `libwayland-client` in the symbol lookup order, so
//! [`ensure_preloaded`] re-executes the program once with the library in `LD_PRELOAD`; then
//! [`api`] finds the already loaded copy. Children get the original value back ([`child_env`]).

use std::ffi::{CStr, CString, OsString, c_char, c_int, c_void};
use std::sync::OnceLock;

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;

/// The soname that is opened.
pub const LIBRARY: &str = "libgtk4-layer-shell.so.0";
/// Set by [`ensure_preloaded`] for the re-executed process: the original `LD_PRELOAD`.
const ENV_ORIG_PRELOAD: &str = "DESK_SHELL_ORIG_LD_PRELOAD";

/// `GtkLayerShellLayer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Top = 2,
    Overlay = 3,
}

/// `GtkLayerShellEdge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left = 0,
    Right = 1,
    Top = 2,
    Bottom = 3,
}

/// `GtkLayerShellKeyboardMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardMode {
    None = 0,
    OnDemand = 2,
}

/// Where and how a surface is placed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub namespace: &'static str,
    pub layer: Layer,
    pub anchors: &'static [Edge],
    /// `(edge, pixels)`.
    pub margins: &'static [(Edge, i32)],
    pub exclusive_zone: bool,
    pub keyboard: KeyboardMode,
}

/// Why the library cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// `dlopen` failed: the library is not installed.
    NotFound(String),
    /// The library lacks a symbol (too old or not gtk4-layer-shell).
    MissingSymbol(&'static str),
}

type GtkWindowPtr = *mut c_void;

/// The resolved functions.
#[derive(Clone, Copy, Debug)]
pub struct Api {
    is_supported: unsafe extern "C" fn() -> c_int,
    init_for_window: unsafe extern "C" fn(GtkWindowPtr),
    set_layer: unsafe extern "C" fn(GtkWindowPtr, c_int),
    set_anchor: unsafe extern "C" fn(GtkWindowPtr, c_int, c_int),
    auto_exclusive_zone_enable: unsafe extern "C" fn(GtkWindowPtr),
    set_margin: unsafe extern "C" fn(GtkWindowPtr, c_int, c_int),
    set_keyboard_mode: unsafe extern "C" fn(GtkWindowPtr, c_int),
    set_namespace: unsafe extern "C" fn(GtkWindowPtr, *const c_char),
}

/// Resolve `name` in `handle` as a function pointer of type `F`.
///
/// # Safety
/// `F` must be the C signature of the symbol.
unsafe fn symbol<F: Copy>(handle: *mut c_void, name: &'static CStr) -> Result<F, LoadError> {
    // SAFETY: the handle comes from dlopen and stays open for the process lifetime.
    let ptr = unsafe { libc::dlsym(handle, name.as_ptr()) };
    if ptr.is_null() {
        return Err(LoadError::MissingSymbol(name.to_str().unwrap_or("?")));
    }
    // SAFETY: a function pointer has the size of a data pointer on the supported platforms, and
    // the caller promises the signature.
    Ok(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&ptr) })
}

impl Api {
    /// Open `library` (`RTLD_LAZY | RTLD_LOCAL`, never closed) and resolve every function.
    pub fn load(library: &str) -> Result<Api, LoadError> {
        let name = CString::new(library).map_err(|_| LoadError::NotFound(library.to_owned()))?;
        // SAFETY: a valid C string; the flags are plain dlopen flags.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(LoadError::NotFound(library.to_owned()));
        }
        // SAFETY: the signatures below are those of gtk4-layer-shell 1.x (gtk-layer-shell.h).
        unsafe {
            Ok(Api {
                is_supported: symbol(handle, c"gtk_layer_is_supported")?,
                init_for_window: symbol(handle, c"gtk_layer_init_for_window")?,
                set_layer: symbol(handle, c"gtk_layer_set_layer")?,
                set_anchor: symbol(handle, c"gtk_layer_set_anchor")?,
                auto_exclusive_zone_enable: symbol(
                    handle,
                    c"gtk_layer_auto_exclusive_zone_enable",
                )?,
                set_margin: symbol(handle, c"gtk_layer_set_margin")?,
                set_keyboard_mode: symbol(handle, c"gtk_layer_set_keyboard_mode")?,
                set_namespace: symbol(handle, c"gtk_layer_set_namespace")?,
            })
        }
    }

    /// The compositor supports `wlr-layer-shell`. Call after GTK is initialized.
    pub fn is_supported(&self) -> bool {
        // SAFETY: no arguments; GTK is initialized by the caller.
        unsafe { (self.is_supported)() != 0 }
    }

    /// Turn `window` (not yet realized) into a layer surface placed by `p`.
    pub fn apply(&self, window: &gtk::Window, p: &Placement) {
        let ptr: *mut gtk::ffi::GtkWindow = window.to_glib_none().0;
        let w = ptr.cast::<c_void>();
        let ns = CString::new(p.namespace).unwrap_or_default();
        // SAFETY: `w` is a live GtkWindow for the duration of the calls; the library copies the
        // namespace string.
        unsafe {
            (self.init_for_window)(w);
            (self.set_namespace)(w, ns.as_ptr());
            (self.set_layer)(w, p.layer as c_int);
            for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
                (self.set_anchor)(w, edge as c_int, c_int::from(p.anchors.contains(&edge)));
            }
            for (edge, px) in p.margins {
                (self.set_margin)(w, *edge as c_int, *px);
            }
            if p.exclusive_zone {
                (self.auto_exclusive_zone_enable)(w);
            }
            (self.set_keyboard_mode)(w, p.keyboard as c_int);
        }
    }
}

static WINDOWED: OnceLock<bool> = OnceLock::new();
static API: OnceLock<Option<Api>> = OnceLock::new();

/// Force normal windows (`--windowed`). Call before the first [`place`].
pub fn force_windowed() {
    let _ = WINDOWED.set(true);
}

/// The library, when it loads and the compositor supports the protocol (needs GTK initialized).
pub fn api() -> Option<&'static Api> {
    if WINDOWED.get().copied().unwrap_or(false) {
        return None;
    }
    API.get_or_init(|| match Api::load(LIBRARY) {
        Ok(api) if api.is_supported() => Some(api),
        Ok(_) => {
            eprintln!("desk-shell: the compositor has no wlr-layer-shell; using normal windows");
            None
        }
        Err(e) => {
            eprintln!("desk-shell: {e:?}; using normal windows");
            None
        }
    })
    .as_ref()
}

/// Place `window` as a layer surface. Returns `false` (normal window) in the fallback.
pub fn place(window: &impl IsA<gtk::Window>, p: &Placement) -> bool {
    match api() {
        Some(api) => {
            api.apply(window.upcast_ref(), p);
            true
        }
        None => false,
    }
}

/// `LD_PRELOAD` with the library added, or `None` when it is already there.
pub fn preload_value(current: &str) -> Option<String> {
    if current.split([':', ' ']).any(|p| p.ends_with(LIBRARY)) {
        return None;
    }
    Some(if current.is_empty() {
        LIBRARY.to_owned()
    } else {
        format!("{LIBRARY}:{current}")
    })
}

/// Re-execute once with the library in `LD_PRELOAD` (see the module docs). Does nothing when
/// the library is absent, already preloaded, or this is the re-executed process. Call first
/// thing in `main`, before any thread starts.
pub fn ensure_preloaded() {
    use std::os::unix::process::CommandExt;
    if std::env::var_os(ENV_ORIG_PRELOAD).is_some() || Api::load(LIBRARY).is_err() {
        return;
    }
    let current = std::env::var("LD_PRELOAD").unwrap_or_default();
    let Some(value) = preload_value(&current) else {
        return;
    };
    let mut args = std::env::args_os();
    let (Some(argv0), Ok(exe)) = (args.next(), std::env::current_exe()) else {
        return;
    };
    let err = std::process::Command::new(exe)
        .arg0(argv0)
        .args(args)
        .env("LD_PRELOAD", value)
        .env(ENV_ORIG_PRELOAD, current)
        .exec();
    eprintln!("desk-shell: cannot preload {LIBRARY}: {err}");
}

/// What a launched application gets instead of the shell's own environment: `(name, value)`,
/// `None` = unset. Empty unless [`ensure_preloaded`] changed `LD_PRELOAD` (`orig` is then the
/// value the user started the shell with).
pub fn child_env(orig: Option<OsString>) -> Vec<(&'static str, Option<OsString>)> {
    match orig {
        None => Vec::new(),
        Some(o) => vec![
            ("LD_PRELOAD", Some(o).filter(|v| !v.is_empty())),
            (ENV_ORIG_PRELOAD, None),
        ],
    }
}

/// [`child_env`] of this process.
pub fn child_env_now() -> Vec<(&'static str, Option<OsString>)> {
    child_env(std::env::var_os(ENV_ORIG_PRELOAD))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_library_is_an_error_not_a_panic() {
        assert_eq!(
            Api::load("libdesk-shell-does-not-exist.so.0").unwrap_err(),
            LoadError::NotFound("libdesk-shell-does-not-exist.so.0".into())
        );
        assert!(matches!(
            Api::load("bad\0name"),
            Err(LoadError::NotFound(_))
        ));
    }

    #[test]
    fn a_library_without_the_symbols_is_rejected() {
        // the C library is always there and has none of the gtk_layer_* functions
        assert_eq!(
            Api::load("libc.so.6").unwrap_err(),
            LoadError::MissingSymbol("gtk_layer_is_supported")
        );
    }

    #[test]
    fn windowed_mode_never_loads_the_library() {
        force_windowed();
        assert!(api().is_none());
    }

    #[test]
    fn preload_is_added_once() {
        assert_eq!(preload_value("").as_deref(), Some(LIBRARY));
        assert_eq!(
            preload_value("/usr/lib/libfoo.so").as_deref(),
            Some("libgtk4-layer-shell.so.0:/usr/lib/libfoo.so")
        );
        assert_eq!(preload_value(LIBRARY), None);
        assert_eq!(
            preload_value("/usr/lib64/libgtk4-layer-shell.so.0 /x.so"),
            None
        );
    }

    #[test]
    fn children_get_the_original_preload_back() {
        assert!(child_env(None).is_empty());
        assert_eq!(
            child_env(Some(OsString::new())),
            [("LD_PRELOAD", None), (ENV_ORIG_PRELOAD, None)]
        );
        assert_eq!(
            child_env(Some("/x.so".into())),
            [
                ("LD_PRELOAD", Some("/x.so".into())),
                (ENV_ORIG_PRELOAD, None)
            ]
        );
    }
}
