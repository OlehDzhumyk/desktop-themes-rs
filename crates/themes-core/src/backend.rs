// SPDX-License-Identifier: GPL-3.0-or-later
//! The desktop backend boundary ([ADR 0011](../../../docs/adr/0011-wayland-agnostic-rust-core.md)).
//!
//! Plan, managed keys, baseline, rollback and `themes doctor` stay above this boundary. A backend
//! adds what only its desktop has: its own diagnostics and the compositor side of a skin.
//!
//! * [`GnomeBackend`]: GNOME Shell. Its checks (extensions, journal, crashes) stay in
//!   [`crate::doctor`] and run when [`DesktopBackend::supports_gnome_extensions`] is true, so the
//!   GNOME behavior is the same as before this module. The skin goes through dconf.
//! * [`LabwcBackend`]: the "Desk (Windows 11 / labwc)" session
//!   ([ADR 0012](../../../docs/adr/0012-wayland-layer-shell-desk-shell.md)).
//! * [`OtherBackend`]: any other Wayland desktop. Nothing is supported yet; the doctor says so
//!   and skips the GNOME Shell checks.

use desk_settings::tr;
use std::path::{Path, PathBuf};
use theme_tokens::generate::ExtraTarget;

use crate::doctor::Issue;
use crate::error::Result;
use crate::paths::Ctx;
use crate::theme::Theme;

/// The `skin` key of `io.github.OlehDzhumyk.Desk`: how the bridge and desk-shell draw panels.
pub const SKIN_KEY: &str = "/io/github/OlehDzhumyk/Desk/skin";
/// The labwc theme referenced by `data/labwc/windows11/rc.xml`.
pub const DEFAULT_LABWC_THEME: &str = "desk-win11";
/// The skin whose bundled `themerc` backs [`DEFAULT_LABWC_THEME`].
const DEFAULT_LABWC_SKIN: &str = "windows11";

/// Which desktop the session runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopKind {
    Gnome,
    Labwc,
    WaylandOther,
}

impl DesktopKind {
    pub fn name(self) -> &'static str {
        match self {
            DesktopKind::Gnome => "gnome",
            DesktopKind::Labwc => "labwc",
            DesktopKind::WaylandOther => "wayland-other",
        }
    }
}

/// The desktop from `XDG_CURRENT_DESKTOP` (a `:`-separated list, case does not matter) and
/// `WAYLAND_DISPLAY`.
///
/// * A `GNOME` entry (also `ubuntu:GNOME`, `GNOME-Classic`) is GNOME; a `labwc` entry, or our
///   session name `desk-win11`, is labwc. GNOME wins when both are given.
/// * No desktop name is GNOME: the headless GNOME harness, login units and SSH run without it,
///   and GNOME was the only target before this boundary.
/// * Any other name is `WaylandOther` on Wayland. Without a Wayland display (an X11 desktop) the
///   GNOME path stays, as before.
pub fn detect_from_env(
    xdg_current_desktop: Option<&str>,
    wayland_display: Option<&str>,
) -> DesktopKind {
    let names: Vec<String> = xdg_current_desktop
        .unwrap_or_default()
        .split(':')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() || names.iter().any(|n| n.starts_with("gnome")) {
        return DesktopKind::Gnome;
    }
    if names.iter().any(|n| n == "labwc" || n == "desk-win11") {
        return DesktopKind::Labwc;
    }
    if wayland_display.is_some_and(|w| !w.trim().is_empty()) {
        DesktopKind::WaylandOther
    } else {
        DesktopKind::Gnome
    }
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// The desktop of this process: `XDG_CURRENT_DESKTOP`, else `XDG_SESSION_DESKTOP`, else
/// `DESKTOP_SESSION`.
pub fn detect_kind() -> DesktopKind {
    let desktop = env_nonempty("XDG_CURRENT_DESKTOP")
        .or_else(|| env_nonempty("XDG_SESSION_DESKTOP"))
        .or_else(|| env_nonempty("DESKTOP_SESSION"));
    detect_from_env(
        desktop.as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
    )
}

/// The backend of this process's desktop.
pub fn detect() -> Box<dyn DesktopBackend> {
    match detect_kind() {
        DesktopKind::Gnome => Box::new(GnomeBackend),
        DesktopKind::Labwc => Box::new(LabwcBackend::from_env()),
        DesktopKind::WaylandOther => Box::new(OtherBackend),
    }
}

/// The backend of `ctx`: detected from the environment on a live session, [`GnomeBackend`] with
/// the in-memory test dconf so unit tests never pick up the host desktop.
pub(crate) fn default_backend(ctx: &Ctx) -> Box<dyn DesktopBackend> {
    if ctx.dconf.live() {
        detect()
    } else {
        Box::new(GnomeBackend)
    }
}

/// What differs between desktops. Kept small on purpose (ADR 0011): it grows when a backend
/// needs it, not ahead of time.
pub trait DesktopBackend {
    fn kind(&self) -> DesktopKind;
    /// A short name for people and logs.
    fn name(&self) -> &'static str;
    /// Checks of this desktop only, added to the shared `themes doctor` checks.
    fn diagnose(&self, ctx: &Ctx) -> Vec<Issue>;
    /// GNOME Shell extensions exist here: the doctor checks and fixes them.
    fn supports_gnome_extensions(&self) -> bool;
    /// The compositor side of `skin` for `theme_id` (the resolved Look module theme). Returns what
    /// was done (or, with `dry_run`, what would be done). Never fails because an optional part is
    /// missing.
    fn apply_compositor_skin(
        &self,
        ctx: &Ctx,
        theme_id: &str,
        skin: &str,
        dry_run: bool,
    ) -> Result<Vec<String>>;
    /// Writes or removes token-driven compositor files (`themerc`, `desk-shell.css`) for the
    /// resolved Look theme without modifying dconf or reconfiguring a live compositor yet.
    /// Returns `(messages, needs_reconfigure)`. Used inside `apply`'s transaction so a later step
    /// failure can roll the files back.
    fn sync_compositor_files(
        &self,
        ctx: &Ctx,
        theme_id: &str,
        skin: &str,
        dry_run: bool,
    ) -> Result<(Vec<String>, bool)> {
        let _ = (ctx, theme_id, skin, dry_run);
        Ok((Vec::new(), false))
    }
    /// Reconfigures a live compositor after committed changes.
    fn reconfigure_compositor(&self, ctx: &Ctx, dry_run: bool) -> Result<Vec<String>> {
        let _ = (ctx, dry_run);
        Ok(Vec::new())
    }
    /// Restores compositor files to `prev_theme_id` (or removes generated files and restores
    /// bundled defaults when `prev_theme_id` is `None` or has no tokens), reconfiguring a live
    /// compositor when changed.
    fn reset_compositor(
        &self,
        ctx: &Ctx,
        prev_theme_id: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<String>> {
        let _ = (ctx, prev_theme_id, dry_run);
        Ok(Vec::new())
    }
}

/// GNOME Shell: the existing checks in [`crate::doctor`]; dconf carries the skin.
#[derive(Debug, Default)]
pub struct GnomeBackend;

impl DesktopBackend for GnomeBackend {
    fn kind(&self) -> DesktopKind {
        DesktopKind::Gnome
    }
    fn name(&self) -> &'static str {
        "GNOME"
    }
    fn diagnose(&self, _ctx: &Ctx) -> Vec<Issue> {
        Vec::new()
    }
    fn supports_gnome_extensions(&self) -> bool {
        true
    }
    fn apply_compositor_skin(&self, _: &Ctx, _: &str, _: &str, _: bool) -> Result<Vec<String>> {
        // the theme's behavior.dconf writes the skin key and the bridge follows it
        Ok(Vec::new())
    }
}

/// A Wayland desktop without a compositor-specific backend yet (`desk-shell` still runs on any
/// `wlr-layer-shell` compositor, so its generated stylesheet is kept in sync).
#[derive(Debug, Default)]
pub struct OtherBackend;

impl DesktopBackend for OtherBackend {
    fn kind(&self) -> DesktopKind {
        DesktopKind::WaylandOther
    }
    fn name(&self) -> &'static str {
        "Wayland"
    }
    fn diagnose(&self, _ctx: &Ctx) -> Vec<Issue> {
        vec![issue(
            "info",
            "desktop",
            tr!("this desktop has no backend yet: GNOME Shell checks are skipped"),
        )]
    }
    fn supports_gnome_extensions(&self) -> bool {
        false
    }
    fn sync_compositor_files(
        &self,
        ctx: &Ctx,
        theme_id: &str,
        _skin: &str,
        dry_run: bool,
    ) -> Result<(Vec<String>, bool)> {
        let look_theme = resolve_look_theme(ctx, theme_id);
        let mut done = Vec::new();
        sync_desk_shell_css(ctx, look_theme.as_ref(), dry_run, &mut done)?;
        Ok((done, false))
    }
    fn apply_compositor_skin(
        &self,
        ctx: &Ctx,
        theme_id: &str,
        skin: &str,
        dry_run: bool,
    ) -> Result<Vec<String>> {
        self.sync_compositor_files(ctx, theme_id, skin, dry_run)
            .map(|(done, _)| done)
    }
    fn reset_compositor(
        &self,
        ctx: &Ctx,
        prev_theme_id: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<String>> {
        self.apply_compositor_skin(ctx, prev_theme_id.unwrap_or(""), "", dry_run)
    }
}

fn resolve_look_theme(ctx: &Ctx, theme_id: &str) -> Option<Theme> {
    (!theme_id.is_empty())
        .then(|| crate::theme::get_theme(ctx, theme_id).ok())
        .flatten()
}

/// Writes `$XDG_CONFIG_HOME/desk-shell/desk-shell.css` when `look_theme` produces tokens (along
/// its `INHERITS` chain), or removes any stale generated file when it does not.
fn sync_desk_shell_css(
    ctx: &Ctx,
    look_theme: Option<&Theme>,
    dry_run: bool,
    done: &mut Vec<String>,
) -> Result<bool> {
    let dest = ctx.desk_shell_css();
    let rendered = match look_theme {
        Some(t) => crate::tokens::render_extra(t, ExtraTarget::DeskShellCss)?,
        None => None,
    };
    if let Some(file) = rendered {
        let bytes = file.contents.as_bytes();
        if std::fs::read(&dest).ok().as_deref() != Some(bytes) {
            if !dry_run {
                write_atomic(&dest, bytes)?;
            }
            done.push(tr!(
                "desk-shell stylesheet updated ({path})",
                path = dest.display()
            ));
            return Ok(true);
        }
        return Ok(false);
    }
    if crate::util::lexists(&dest) && !dest.is_dir() {
        if !dry_run {
            std::fs::remove_file(&dest)?;
        }
        done.push(tr!(
            "desk-shell stylesheet removed ({path})",
            path = dest.display()
        ));
        return Ok(true);
    }
    Ok(false)
}

fn issue(severity: &'static str, kind: &'static str, message: String) -> Issue {
    Issue {
        severity,
        kind,
        message,
        uuid: None,
        fixable: false,
    }
}

/// Where the labwc backend looks. [`LabwcEnv::from_env`] reads the process; tests build one
/// over temporary directories, so they never see the host.
#[derive(Clone, Debug)]
pub struct LabwcEnv {
    /// Program directories (`PATH`, then `~/.local/bin`).
    pub bin_dirs: Vec<PathBuf>,
    /// Library directories searched for `libgtk4-layer-shell.so.0`.
    pub lib_dirs: Vec<PathBuf>,
    /// The session config directory (`DESK_LABWC_DIR`, else `$XDG_CONFIG_HOME/desk-labwc`).
    pub config_dir: PathBuf,
    /// `$XDG_DATA_HOME` (themes are installed under it).
    pub data_home: PathBuf,
    /// Bundled labwc configs, one directory per skin (`<dir>/windows11/themerc`).
    pub bundled_dirs: Vec<PathBuf>,
    /// `LABWC_PID` of a labwc that still runs; `None` outside a live labwc session.
    pub live_pid: Option<u32>,
}

impl LabwcEnv {
    pub fn from_env() -> LabwcEnv {
        let home = std::env::var_os("HOME").map_or_else(|| "/".into(), PathBuf::from);
        let local_bin = home.join(".local/bin");
        let mut bin_dirs: Vec<PathBuf> =
            std::env::var_os("PATH").map_or_default(|p| std::env::split_paths(&p).collect());
        if !bin_dirs.contains(&local_bin) {
            bin_dirs.push(local_bin);
        }
        let config_home =
            env_nonempty("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
        let data_home =
            env_nonempty("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from);
        let config_dir = env_nonempty("DESK_LABWC_DIR")
            .map_or_else(|| config_home.join("desk-labwc"), PathBuf::from);
        // the same places as data/labwc/desk-win11-session, minus the script's own directory
        let mut bundled_dirs: Vec<PathBuf> = env_nonempty("DESK_LABWC_SRC")
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .into_iter()
            .collect();
        bundled_dirs.extend([
            data_home.join("desktop-themes/labwc"),
            PathBuf::from("/usr/local/share/desktop-themes/labwc"),
            PathBuf::from("/usr/share/desktop-themes/labwc"),
        ]);
        let live_pid = env_nonempty("LABWC_PID")
            .and_then(|p| p.trim().parse::<u32>().ok())
            .filter(|pid| process_is_labwc(Path::new("/proc"), *pid));
        LabwcEnv {
            bin_dirs,
            lib_dirs: [
                "/usr/lib64",
                "/usr/lib",
                "/usr/local/lib64",
                "/usr/local/lib",
                "/usr/lib/x86_64-linux-gnu",
                "/usr/lib/aarch64-linux-gnu",
            ]
            .map(PathBuf::from)
            .to_vec(),
            config_dir,
            data_home,
            bundled_dirs,
            live_pid,
        }
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.bin_dirs
            .iter()
            .map(|d| d.join(name))
            .find(|p| crate::util::is_executable(p))
    }

    fn has_library(&self, name: &str) -> bool {
        self.lib_dirs.iter().any(|d| d.join(name).exists())
    }

    pub fn installed_themerc(&self, theme: &str) -> PathBuf {
        self.data_home
            .join("themes")
            .join(theme)
            .join("openbox-3/themerc")
    }

    fn bundled_themerc(&self, skin: &str) -> Option<PathBuf> {
        self.bundled_dirs
            .iter()
            .map(|d| d.join(skin).join("themerc"))
            .find(|p| p.is_file())
    }
}

/// `<proc>/<pid>/comm` is `labwc`: `LABWC_PID` may be inherited from a session that ended.
fn process_is_labwc(proc_root: &Path, pid: u32) -> bool {
    std::fs::read_to_string(proc_root.join(pid.to_string()).join("comm"))
        .is_ok_and(|c| c.trim() == "labwc")
}

/// The labwc theme (openbox-3 `themerc`) of a skin; `None` when the skin has none yet.
pub fn labwc_theme_of(skin: &str) -> Option<&'static str> {
    match skin {
        "windows11" => Some("desk-win11"),
        _ => None,
    }
}

/// The "Desk (Windows 11 / labwc)" session: labwc + desk-shell + deskd (ADR 0012).
#[derive(Clone, Debug)]
pub struct LabwcBackend {
    pub env: LabwcEnv,
}

impl LabwcBackend {
    pub fn from_env() -> LabwcBackend {
        LabwcBackend {
            env: LabwcEnv::from_env(),
        }
    }

    pub fn new(env: LabwcEnv) -> LabwcBackend {
        LabwcBackend { env }
    }
}

impl DesktopBackend for LabwcBackend {
    fn kind(&self) -> DesktopKind {
        DesktopKind::Labwc
    }
    fn name(&self) -> &'static str {
        "labwc"
    }

    fn diagnose(&self, _ctx: &Ctx) -> Vec<Issue> {
        let e = &self.env;
        let mut out = Vec::new();
        if e.find_program("labwc").is_none() {
            out.push(issue(
                "error",
                "labwc",
                tr!("labwc is not installed (rpm-ostree install labwc)"),
            ));
        } else if e.live_pid.is_none() {
            out.push(issue(
                "info",
                "labwc",
                tr!("labwc is not running in this session; run the check inside the labwc session"),
            ));
        }
        if e.find_program("desk-shell").is_none() {
            out.push(issue(
                "error",
                "desk-shell",
                tr!("desk-shell is not installed: no taskbar and Start menu (run data/install.sh)"),
            ));
        }
        if !e.has_library("libgtk4-layer-shell.so.0") {
            out.push(issue(
                "warning",
                "layer-shell",
                tr!(
                    "libgtk4-layer-shell.so.0 is not found: desk-shell draws normal windows instead of a taskbar (install gtk4-layer-shell)"
                ),
            ));
        }
        if e.find_program("deskd").is_none() {
            out.push(issue(
                "warning",
                "deskd",
                tr!("deskd is not installed: no clipboard history or search (run data/install.sh)"),
            ));
        }
        if e.find_program("swaylock").is_none() {
            out.push(issue(
                "warning",
                "lock-screen",
                tr!("swaylock is not installed: the screen cannot be locked (install swaylock)"),
            ));
        }
        if e.find_program("swaybg").is_none() {
            out.push(issue(
                "info",
                "wallpaper",
                tr!("swaybg is not installed: the desktop has no wallpaper (install swaybg)"),
            ));
        }
        if !e.config_dir.join("rc.xml").is_file() {
            out.push(issue(
                "warning",
                "labwc-config",
                tr!(
                    "the labwc session config {path} is missing (log in with the “Desk (Windows 11 / labwc)” session)",
                    path = e.config_dir.join("rc.xml").display()
                ),
            ));
        }
        out
    }

    fn supports_gnome_extensions(&self) -> bool {
        false
    }

    fn sync_compositor_files(
        &self,
        ctx: &Ctx,
        theme_id: &str,
        skin: &str,
        dry_run: bool,
    ) -> Result<(Vec<String>, bool)> {
        let e = &self.env;
        let look_theme = resolve_look_theme(ctx, theme_id);
        let mut done = Vec::new();
        let mut reconfigure = false;

        let labwc_theme = labwc_theme_of(skin).unwrap_or(DEFAULT_LABWC_THEME);
        let dest = e.installed_themerc(labwc_theme);
        let rendered = match &look_theme {
            Some(t) => crate::tokens::render_extra(t, ExtraTarget::LabwcTheme)?,
            None => None,
        };
        if let Some(file) = rendered {
            let bytes = file.contents.as_bytes();
            if std::fs::read(&dest).ok().as_deref() != Some(bytes) {
                if !dry_run {
                    write_atomic(&dest, bytes)?;
                }
                done.push(tr!("labwc theme {name} updated", name = labwc_theme));
                reconfigure = true;
            }
        } else if let Some(src) = (!skin.is_empty())
            .then(|| e.bundled_themerc(skin))
            .flatten()
            .or_else(|| e.bundled_themerc(DEFAULT_LABWC_SKIN))
        {
            let text = std::fs::read(&src)?;
            if std::fs::read(&dest).ok().as_deref() != Some(text.as_slice()) {
                if !dry_run {
                    write_atomic(&dest, &text)?;
                }
                done.push(tr!("labwc theme {name} updated", name = labwc_theme));
                reconfigure = true;
            }
        }

        sync_desk_shell_css(ctx, look_theme.as_ref(), dry_run, &mut done)?;
        Ok((done, reconfigure))
    }

    fn reconfigure_compositor(&self, ctx: &Ctx, dry_run: bool) -> Result<Vec<String>> {
        // only a live labwc of this session, never the in-memory dconf of tests
        if let Some(pid) = self.env.live_pid.filter(|_| ctx.dconf.live()) {
            if !dry_run {
                crate::util::run("labwc", &["--reconfigure"], None)?;
            }
            return Ok(vec![tr!("labwc (pid {pid}) reconfigured", pid = pid)]);
        }
        Ok(Vec::new())
    }

    fn apply_compositor_skin(
        &self,
        ctx: &Ctx,
        theme_id: &str,
        skin: &str,
        dry_run: bool,
    ) -> Result<Vec<String>> {
        let mut done = Vec::new();
        let mut reconfigure = false;

        if !skin.is_empty() {
            let want = format!("'{skin}'");
            if ctx.dconf.dump()?.get(SKIN_KEY) != Some(&want) {
                if !dry_run {
                    ctx.dconf.write(SKIN_KEY, &want)?;
                }
                done.push(tr!(
                    "skin → {skin} (theme {theme})",
                    skin = skin,
                    theme = theme_id
                ));
                reconfigure = true;
            }
        }

        let (file_done, files_reconfigure) =
            self.sync_compositor_files(ctx, theme_id, skin, dry_run)?;
        done.extend(file_done);
        reconfigure |= files_reconfigure;

        if reconfigure {
            done.extend(self.reconfigure_compositor(ctx, dry_run)?);
        }
        Ok(done)
    }

    fn reset_compositor(
        &self,
        ctx: &Ctx,
        prev_theme_id: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<String>> {
        self.apply_compositor_skin(ctx, prev_theme_id.unwrap_or(""), "", dry_run)
    }
}

fn write_atomic(p: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = crate::gtklinks::tmp_name(p);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dconf::{Backend, mem::Db};
    use std::sync::{Arc, Mutex};

    #[test]
    fn detect_from_env_cases() {
        use DesktopKind::*;
        let wl = Some("wayland-0");
        let cases: &[(Option<&str>, Option<&str>, DesktopKind)] = &[
            (Some("GNOME"), wl, Gnome),
            (Some("fedora:GNOME"), wl, Gnome),
            (Some("ubuntu:GNOME"), None, Gnome),
            (Some("GNOME-Classic:GNOME"), wl, Gnome),
            (Some("labwc:wlroots"), wl, Labwc),
            (Some("LABWC"), None, Labwc),
            (Some("desk-win11"), wl, Labwc),
            (Some("niri"), wl, WaylandOther),
            (Some("wlroots"), wl, WaylandOther),
            (Some("KDE"), wl, WaylandOther),
            // an X11 desktop keeps the old GNOME path
            (Some("XFCE"), None, Gnome),
            (Some("niri"), Some(" "), Gnome),
            // unset or empty: GNOME (headless harness, login units, SSH)
            (None, None, Gnome),
            (None, wl, Gnome),
            (Some(""), wl, Gnome),
            (Some(" : "), wl, Gnome),
        ];
        for (xdg, way, want) in cases {
            assert_eq!(detect_from_env(*xdg, *way), *want, "{xdg:?} {way:?}");
        }
    }

    #[test]
    fn only_gnome_has_extensions() {
        assert!(GnomeBackend.supports_gnome_extensions());
        assert!(!OtherBackend.supports_gnome_extensions());
        let (_, b) = fixture("ext", &[]);
        assert!(!b.supports_gnome_extensions());
        assert_eq!(b.kind(), DesktopKind::Labwc);
    }

    /// A labwc environment over a temporary directory: programs in `bin`, libraries in `lib`.
    fn fixture(tag: &str, programs: &[&str]) -> (PathBuf, LabwcBackend) {
        let root = crate::theme::tests::tmp_root(&format!("backend-{tag}"));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        for p in programs {
            add_program(&bin, p);
        }
        let env = LabwcEnv {
            bin_dirs: vec![bin],
            lib_dirs: vec![root.join("lib")],
            config_dir: root.join("config/desk-labwc"),
            data_home: root.join("data"),
            bundled_dirs: vec![root.join("bundled")],
            live_pid: None,
        };
        (root, LabwcBackend::new(env))
    }

    fn add_program(bin: &Path, name: &str) {
        use std::os::unix::fs::PermissionsExt;
        let p = bin.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn mem_ctx(root: &Path) -> Ctx {
        let mut ctx = Ctx::new(
            root.join("home"),
            root.join("home/.local/share/desktop-themes"),
        );
        ctx.dconf = Backend::Memory(Arc::new(Mutex::new(Db::default())));
        ctx
    }

    fn kinds(issues: &[Issue]) -> Vec<(&'static str, &'static str)> {
        issues.iter().map(|i| (i.kind, i.severity)).collect()
    }

    #[test]
    fn labwc_diagnose_empty_system() {
        let (root, b) = fixture("empty", &[]);
        let got = kinds(&b.diagnose(&mem_ctx(&root)));
        assert_eq!(
            got,
            vec![
                ("labwc", "error"),
                ("desk-shell", "error"),
                ("layer-shell", "warning"),
                ("deskd", "warning"),
                ("lock-screen", "warning"),
                ("wallpaper", "info"),
                ("labwc-config", "warning"),
            ]
        );
        // never a GNOME Shell issue in a labwc session
        assert!(
            !got.iter()
                .any(|(k, _)| ["extension", "permanent"].contains(k))
        );
    }

    #[test]
    fn labwc_diagnose_complete_system() {
        let all = ["labwc", "desk-shell", "deskd", "swaylock", "swaybg"];
        let (root, mut b) = fixture("full", &all);
        std::fs::write(root.join("lib/libgtk4-layer-shell.so.0"), "").unwrap();
        crate::theme::tests::write(&b.env.config_dir.join("rc.xml"), "<labwc_config/>");
        b.env.live_pid = Some(1);
        assert_eq!(kinds(&b.diagnose(&mem_ctx(&root))), vec![]);
        // not running: one info, still nothing to fix
        b.env.live_pid = None;
        assert_eq!(kinds(&b.diagnose(&mem_ctx(&root))), vec![("labwc", "info")]);
    }

    #[test]
    fn labwc_diagnose_finds_programs_in_any_bin_dir() {
        let (root, mut b) = fixture("bins", &["labwc"]);
        let local_bin = root.join("home/.local/bin");
        std::fs::create_dir_all(&local_bin).unwrap();
        add_program(&local_bin, "desk-shell");
        // a file that is not executable is not a program
        std::fs::write(local_bin.join("swaylock"), "").unwrap();
        b.env.bin_dirs.push(local_bin);
        let got = kinds(&b.diagnose(&mem_ctx(&root)));
        assert!(!got.iter().any(|(k, _)| *k == "desk-shell"), "{got:?}");
        assert!(got.contains(&("lock-screen", "warning")), "{got:?}");
    }

    #[test]
    fn process_is_labwc_reads_comm() {
        let root = crate::theme::tests::tmp_root("backend-proc");
        crate::theme::tests::write(&root.join("42/comm"), "labwc\n");
        crate::theme::tests::write(&root.join("43/comm"), "bash\n");
        assert!(process_is_labwc(&root, 42));
        assert!(!process_is_labwc(&root, 43));
        assert!(!process_is_labwc(&root, 44));
    }

    #[test]
    fn labwc_apply_skin_writes_key_and_theme_once() {
        let (root, b) = fixture("apply", &[]);
        let ctx = mem_ctx(&root);
        crate::theme::tests::write(&root.join("bundled/windows11/themerc"), "border.width: 1\n");
        let themerc = b.env.installed_themerc("desk-win11");

        // dry run: reports, changes nothing
        let planned = b
            .apply_compositor_skin(&ctx, "windows11", "windows11", true)
            .unwrap();
        assert_eq!(planned.len(), 2, "{planned:?}");
        assert!(!ctx.dconf.dump().unwrap().contains_key(SKIN_KEY));
        assert!(!themerc.exists());

        let done = b
            .apply_compositor_skin(&ctx, "windows11", "windows11", false)
            .unwrap();
        assert_eq!(done.len(), 2, "{done:?}");
        assert_eq!(
            ctx.dconf.dump().unwrap().get(SKIN_KEY).map(String::as_str),
            Some("'windows11'")
        );
        assert_eq!(
            std::fs::read_to_string(&themerc).unwrap(),
            "border.width: 1\n"
        );
        // idempotent; no live labwc and in-memory dconf: never a reconfigure
        assert!(
            b.apply_compositor_skin(&ctx, "windows11", "windows11", false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn labwc_apply_skin_without_labwc_theme() {
        let (root, mut b) = fixture("apply-macos", &[]);
        // a live pid is ignored with the in-memory dconf
        b.env.live_pid = Some(1);
        let ctx = mem_ctx(&root);
        let done = b
            .apply_compositor_skin(&ctx, "macos", "macos", false)
            .unwrap();
        assert_eq!(done.len(), 1, "{done:?}");
        assert!(!root.join("data/themes").exists());
        assert!(
            GnomeBackend
                .apply_compositor_skin(&ctx, "macos", "macos", false)
                .unwrap()
                .is_empty()
        );
    }

    const LAV_TOKENS: &str = r##"
schema = 1
[colors.light]
accent = "#6d4fc2"
window_bg = "#f7f5fc"
window_fg = "#221c35"
[colors.dark]
accent = "#8f74e6"
window_bg = "#1d1a27"
window_fg = "#ebe7f5"
headerbar_bg = "#262232"
headerbar_fg = "#ebe7f5"
[radii]
button = 8
popover = 12
"##;

    const WIN_TOKENS: &str = r##"
schema = 1
[colors]
accent = "blue"
[colors.dark]
headerbar_bg = "#202020"
headerbar_fg = "#ffffff"
[radii]
button = 4
"##;

    const BUNDLED_THEMERC: &str = "# bundled default\nborder.width: 1\n";

    fn apply_fx(tag: &str) -> (PathBuf, Ctx, Arc<Mutex<Db>>, LabwcBackend) {
        let (root, b) = fixture(tag, &[]);
        let mut ctx = Ctx::new(root.join("home"), root.clone());
        let db = Arc::new(Mutex::new(Db::default()));
        ctx.dconf = Backend::Memory(db.clone());
        crate::theme::tests::write(&root.join("baseline-t/full.dconf"), "");
        crate::theme::tests::write(&root.join("bundled/windows11/themerc"), BUNDLED_THEMERC);
        // Builtin fedora theme (no tokens.toml, sets skin='default')
        crate::theme::tests::write(&ctx.themes.join("fedora/theme.conf"), "NAME=Fedora\n");
        crate::theme::tests::write(
            &ctx.themes.join("fedora/desk.dconf"),
            "[io/github/OlehDzhumyk/Desk]\nskin='default'\n",
        );
        // Non-token theme
        crate::theme::tests::write(&ctx.themes.join("plain/theme.conf"), "NAME=Plain\n");
        crate::theme::tests::write(
            &ctx.themes.join("plain/settings.dconf"),
            "[org/example/test]\nval='plain'\n",
        );
        // Token theme without desk.dconf (like lavender)
        crate::theme::tests::write(&ctx.themes.join("lav/theme.conf"), "NAME=Lav\n");
        crate::theme::tests::write(&ctx.themes.join("lav/tokens.toml"), LAV_TOKENS);
        crate::theme::tests::write(
            &ctx.themes.join("lav/settings.dconf"),
            "[org/example/test]\nval='lav'\n",
        );
        // Token theme with skin='windows11' and behavior module
        crate::theme::tests::write(&ctx.themes.join("win/theme.conf"), "NAME=Win\n");
        crate::theme::tests::write(&ctx.themes.join("win/tokens.toml"), WIN_TOKENS);
        crate::theme::tests::write(
            &ctx.themes.join("win/desk.dconf"),
            "[io/github/OlehDzhumyk/Desk]\nskin='windows11'\n",
        );
        crate::theme::tests::write(
            &ctx.themes.join("win/behavior.dconf"),
            "[org/example/behavior]\nflag=true\n",
        );
        // Child theme inheriting from lav and overriding button radius & dark accent
        crate::theme::tests::write(
            &ctx.themes.join("lav-child/theme.conf"),
            "NAME=\"Lav Child\"\nINHERITS=lav\n",
        );
        crate::theme::tests::write(
            &ctx.themes.join("lav-child/tokens.toml"),
            "[colors.dark]\naccent = \"#aa3355\"\n[radii]\nbutton = 10\n",
        );
        (root, ctx, db, b)
    }

    #[test]
    fn labwc_apply_tokens_inherits_and_dry_run() {
        use crate::apply::{Options, apply_on};
        let (root, ctx, _, b) = apply_fx("tokens-apply");
        let themerc = b.env.installed_themerc(DEFAULT_LABWC_THEME);
        let css = ctx.desk_shell_css();
        assert!(
            css.starts_with(&root),
            "must stay inside temp root: {css:?}"
        );

        // dry run writes neither file and reports planned updates
        let dry = Options {
            dry_run: true,
            ..Default::default()
        };
        let planned = apply_on(&ctx, "lav", &[], &dry, &b).unwrap();
        assert_eq!(planned.compositor.len(), 2, "{:?}", planned.compositor);
        assert!(!themerc.exists());
        assert!(!css.exists());

        // real apply generates both files matching render_extra
        let res = apply_on(&ctx, "lav", &[], &Options::default(), &b).unwrap();
        assert_eq!(res.compositor.len(), 2, "{:?}", res.compositor);
        let lav_theme = crate::theme::get_theme(&ctx, "lav").unwrap();
        let want_rc = crate::tokens::render_extra(&lav_theme, ExtraTarget::LabwcTheme)
            .unwrap()
            .unwrap()
            .contents;
        let want_css = crate::tokens::render_extra(&lav_theme, ExtraTarget::DeskShellCss)
            .unwrap()
            .unwrap()
            .contents;
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), want_rc);
        assert_eq!(std::fs::read_to_string(&css).unwrap(), want_css);
        assert!(want_rc.contains("window.active.title.bg.color: #262232"));
        assert!(want_css.contains("--ds-accent: #8f74e6;"));

        // idempotent re-apply
        let again = apply_on(&ctx, "lav", &[], &Options::default(), &b).unwrap();
        assert!(again.compositor.is_empty(), "{:?}", again.compositor);

        // INHERITS merges parent tokens with child overrides in both outputs
        apply_on(&ctx, "lav-child", &[], &Options::default(), &b).unwrap();
        let child_rc = std::fs::read_to_string(&themerc).unwrap();
        let child_css = std::fs::read_to_string(&css).unwrap();
        assert!(
            child_rc.contains("window.active.title.bg.color: #262232"),
            "{child_rc}"
        );
        assert!(
            child_rc.contains("window.button.hover.bg.corner-radius: 10"),
            "{child_rc}"
        );
        assert!(
            child_rc.contains("snapping.overlay.edge.border.color: #aa3355"),
            "{child_rc}"
        );
        assert!(child_css.contains("--ds-accent: #aa3355;"), "{child_css}");
        assert!(child_css.contains("--ds-radius: 10px;"), "{child_css}");
        assert!(
            child_css.contains("--ds-flyout-radius: 12px;"),
            "{child_css}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn labwc_non_token_theme_and_reset_restore_bundled_and_clear_css() {
        use crate::apply::{Options, apply_on};
        let (root, ctx, _, b) = apply_fx("tokens-reset");
        let themerc = b.env.installed_themerc(DEFAULT_LABWC_THEME);
        let css = ctx.desk_shell_css();

        apply_on(&ctx, "lav", &[], &Options::default(), &b).unwrap();
        let lav_rc = std::fs::read_to_string(&themerc).unwrap();
        let lav_css = std::fs::read_to_string(&css).unwrap();
        assert!(css.is_file());

        // dry run of a non-token theme leaves generated files untouched
        let dry = Options {
            dry_run: true,
            ..Default::default()
        };
        let planned = apply_on(&ctx, "plain", &[], &dry, &b).unwrap();
        assert_eq!(planned.compositor.len(), 2, "{:?}", planned.compositor);
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), lav_rc);
        assert_eq!(std::fs::read_to_string(&css).unwrap(), lav_css);

        // real apply of non-token theme restores bundled themerc and removes desk-shell.css
        let res = apply_on(&ctx, "plain", &[], &Options::default(), &b).unwrap();
        assert_eq!(res.compositor.len(), 2, "{:?}", res.compositor);
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), BUNDLED_THEMERC);
        assert!(!css.exists());

        // re-apply token theme, then reset (apply fedora)
        apply_on(&ctx, "lav", &[], &Options::default(), &b).unwrap();
        assert!(css.is_file());
        assert_ne!(std::fs::read_to_string(&themerc).unwrap(), BUNDLED_THEMERC);

        apply_on(&ctx, crate::BUILTIN_THEME, &[], &Options::default(), &b).unwrap();
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), BUNDLED_THEMERC);
        assert!(!css.exists());

        // idempotent second reset
        let again = apply_on(&ctx, crate::BUILTIN_THEME, &[], &Options::default(), &b).unwrap();
        assert!(again.compositor.is_empty(), "{:?}", again.compositor);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn module_mixing_follows_look_module_tokens() {
        use crate::apply::{Options, apply_on};
        use crate::modules::Module;
        let (root, ctx, _, b) = apply_fx("tokens-mix");
        let themerc = b.env.installed_themerc(DEFAULT_LABWC_THEME);
        let css = ctx.desk_shell_css();

        // apply look from lav, then behavior from win -> outputs stay lav's
        apply_on(&ctx, "lav", &[Module::Look], &Options::default(), &b).unwrap();
        let lav_rc = std::fs::read_to_string(&themerc).unwrap();
        let lav_css = std::fs::read_to_string(&css).unwrap();

        let mix = apply_on(&ctx, "win", &[Module::Behavior], &Options::default(), &b).unwrap();
        assert!(mix.compositor.is_empty(), "{:?}", mix.compositor);
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), lav_rc);
        assert_eq!(std::fs::read_to_string(&css).unwrap(), lav_css);

        // switching only behavior to fedora (reset --only behavior) keeps lav's look files
        apply_on(
            &ctx,
            crate::BUILTIN_THEME,
            &[Module::Behavior],
            &Options::default(),
            &b,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), lav_rc);
        assert_eq!(std::fs::read_to_string(&css).unwrap(), lav_css);

        // switching look to win updates both outputs to win's tokens
        apply_on(&ctx, "win", &[Module::Look], &Options::default(), &b).unwrap();
        let win_rc = std::fs::read_to_string(&themerc).unwrap();
        let win_css = std::fs::read_to_string(&css).unwrap();
        assert!(
            win_rc.contains("window.active.title.bg.color: #202020"),
            "{win_rc}"
        );
        assert!(win_css.contains("--ds-radius: 4px;"), "{win_css}");

        // switching only look to plain clears desk-shell.css and restores bundled themerc
        apply_on(&ctx, "plain", &[Module::Look], &Options::default(), &b).unwrap();
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), BUNDLED_THEMERC);
        assert!(!css.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn other_backend_syncs_desk_shell_css_only() {
        use crate::apply::{Options, apply_on};
        let (root, ctx, _, _) = apply_fx("other-backend");
        let css = ctx.desk_shell_css();

        // dry run writes nothing
        let dry = Options {
            dry_run: true,
            ..Default::default()
        };
        let planned = apply_on(&ctx, "lav", &[], &dry, &OtherBackend).unwrap();
        assert_eq!(planned.compositor.len(), 1, "{:?}", planned.compositor);
        assert!(!css.exists());

        // real apply writes desk-shell.css only (no labwc themerc)
        apply_on(&ctx, "lav", &[], &Options::default(), &OtherBackend).unwrap();
        let content = std::fs::read_to_string(&css).unwrap();
        assert!(content.contains("--ds-accent: #8f74e6;"), "{content}");
        assert!(!root.join("data/themes").exists());

        // reset removes desk-shell.css cleanly
        apply_on(
            &ctx,
            crate::BUILTIN_THEME,
            &[],
            &Options::default(),
            &OtherBackend,
        )
        .unwrap();
        assert!(!css.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn failed_apply_rolls_back_compositor_files() {
        use crate::apply::{Options, apply_on};
        let (root, ctx, db, b) = apply_fx("tokens-rollback");
        let themerc = b.env.installed_themerc(DEFAULT_LABWC_THEME);
        let css = ctx.desk_shell_css();

        // 1. Fresh state with bundled themerc: failed apply of lav rolls back to bundled + no css
        crate::theme::tests::write(&themerc, BUNDLED_THEMERC);
        db.lock().unwrap().fail_key = Some("/org/example/test/val".into());
        let err = apply_on(&ctx, "lav", &[], &Options::default(), &b).unwrap_err();
        assert!(err.0.contains("rolled back"), "{err}");
        db.lock().unwrap().fail_key = None;
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), BUNDLED_THEMERC);
        assert!(!css.exists());

        // 2. Active token theme lav -> failed apply of win restores lav's themerc and css
        apply_on(&ctx, "lav", &[], &Options::default(), &b).unwrap();
        let lav_rc = std::fs::read_to_string(&themerc).unwrap();
        let lav_css = std::fs::read_to_string(&css).unwrap();

        db.lock().unwrap().fail_key = Some(SKIN_KEY.into());
        let err = apply_on(&ctx, "win", &[], &Options::default(), &b).unwrap_err();
        assert!(err.0.contains("rolled back"), "{err}");
        db.lock().unwrap().fail_key = None;
        assert_eq!(std::fs::read_to_string(&themerc).unwrap(), lav_rc);
        assert_eq!(std::fs::read_to_string(&css).unwrap(), lav_css);

        // 3. OtherBackend rollback from lav -> failed plain restores lav's desk-shell.css
        db.lock().unwrap().fail_key = Some("/org/example/test/val".into());
        let err = apply_on(&ctx, "plain", &[], &Options::default(), &OtherBackend).unwrap_err();
        assert!(err.0.contains("rolled back"), "{err}");
        db.lock().unwrap().fail_key = None;
        assert_eq!(std::fs::read_to_string(&css).unwrap(), lav_css);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gnome_backend_leaves_compositor_and_json_unchanged() {
        use crate::apply::{Options, apply_on};
        let (root, ctx, _, _) = apply_fx("gnome-unchanged");
        let res = apply_on(&ctx, "lav", &[], &Options::default(), &GnomeBackend).unwrap();
        assert!(res.compositor.is_empty());
        assert!(!ctx.desk_shell_css().exists());
        let json = serde_json::to_value(&res).unwrap();
        assert!(json.get("compositor").is_none(), "{json}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
