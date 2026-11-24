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

use crate::doctor::Issue;
use crate::error::Result;
use crate::paths::Ctx;

/// The `skin` key of `io.github.OlehDzhumyk.Desk`: how the bridge and desk-shell draw panels.
pub const SKIN_KEY: &str = "/io/github/OlehDzhumyk/Desk/skin";

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
    /// The compositor side of `skin` for `theme_id`. Returns what was done (or, with `dry_run`,
    /// what would be done). Never fails because an optional part is missing.
    fn apply_compositor_skin(
        &self,
        ctx: &Ctx,
        theme_id: &str,
        skin: &str,
        dry_run: bool,
    ) -> Result<Vec<String>>;
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

/// A Wayland desktop without a backend yet.
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
    fn apply_compositor_skin(&self, _: &Ctx, _: &str, _: &str, _: bool) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
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

    fn installed_themerc(&self, theme: &str) -> PathBuf {
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

    fn apply_compositor_skin(
        &self,
        ctx: &Ctx,
        theme_id: &str,
        skin: &str,
        dry_run: bool,
    ) -> Result<Vec<String>> {
        let e = &self.env;
        let mut done = Vec::new();
        let mut changed = false;

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
            changed = true;
        }

        if let (Some(theme), Some(src)) = (labwc_theme_of(skin), e.bundled_themerc(skin)) {
            let dest = e.installed_themerc(theme);
            let text = std::fs::read(&src)?;
            if std::fs::read(&dest).ok().as_deref() != Some(text.as_slice()) {
                if !dry_run {
                    write_atomic(&dest, &text)?;
                }
                done.push(tr!("labwc theme {name} updated", name = theme));
                changed = true;
            }
        }

        // only a live labwc of this session, never the in-memory dconf of tests
        if changed && let Some(pid) = e.live_pid.filter(|_| ctx.dconf.live()) {
            if !dry_run {
                crate::util::run("labwc", &["--reconfigure"], None)?;
            }
            done.push(tr!("labwc (pid {pid}) reconfigured", pid = pid));
        }
        Ok(done)
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
}
