// SPDX-License-Identifier: GPL-3.0-or-later
//! The intent journal of `apply`: `state.json` carries a [`Pending`] record from the first side
//! effect until the new selection is committed, and is saved before every step. So:
//!
//! * when a step fails, [`rollback`] undoes the steps begun so far and the old selection stays;
//! * when the process dies half way (crash, kill, power loss), the record is still there: the
//!   next `apply` and `themes doctor --fix` finish it with [`recover`], `themes doctor` reports it.
//!
//! Steps before [`Step::Hooks`] can be undone (dconf values are recorded before they change; GTK
//! files, flatpak overrides and token links are tracked in the state as they are made). Hooks
//! are programs with effects the manager cannot see, so once they began, recovery completes the
//! apply instead of rolling it back.

use desk_settings::tr;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::lock::Lock;
use crate::modules::Module;
use crate::paths::Ctx;
use crate::state::State;
use crate::theme::get_theme;
use crate::util::log;

/// Side-effecting steps of `apply`, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    /// Links to files generated from tokens.toml.
    Tokens,
    /// Theme keys written and reset.
    Dconf,
    /// enabled-extensions / disabled-extensions.
    Extensions,
    /// ~/.config/gtk-{4,3}.0 links and copies (look).
    Gtk,
    /// Flatpak overrides (look).
    Flatpak,
    /// apps/hooks (the point of no return).
    Hooks,
}

/// An apply that has begun and is not committed yet.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Pending {
    pub theme: String,
    /// Modules being applied.
    pub modules: Vec<Module>,
    pub started: String,
    /// Selection before the apply (rollback relinks its files).
    pub prev_modules: BTreeMap<Module, String>,
    /// Selection, managed keys and theme extensions to commit.
    pub new_modules: BTreeMap<Module, String>,
    pub new_module_keys: BTreeMap<Module, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_extensions: Option<Vec<String>>,
    /// Value of every key the apply may change, before it changed (`None`: not set).
    pub dconf_before: BTreeMap<String, Option<String>>,
    /// Steps begun, in order; the last one may be unfinished.
    pub begun: Vec<Step>,
}

impl Pending {
    pub fn began(&self, s: Step) -> bool {
        self.begun.contains(&s)
    }

    /// Recovery completes (hooks ran) instead of rolling back.
    pub fn completes(&self) -> bool {
        self.began(Step::Hooks)
    }
}

/// Records the intent before the first side effect.
pub(crate) fn begin(ctx: &Ctx, st: &mut State, p: Pending) -> Result<()> {
    st.pending = Some(p);
    st.save(ctx)
}

/// Marks `step` as begun and saves the state before its side effects happen.
pub(crate) fn step(ctx: &Ctx, st: &mut State, step: Step) -> Result<()> {
    if let Some(p) = st.pending.as_mut() {
        p.begun.push(step);
    }
    st.save(ctx)?;
    #[cfg(test)]
    if let crate::dconf::Backend::Memory(m) = &ctx.dconf
        && m.lock().unwrap().crash_at == Some(step)
    {
        panic!("simulated crash at {step:?}");
    }
    Ok(())
}

/// Commits the new selection and clears the record (one save).
pub(crate) fn commit(ctx: &Ctx, st: &mut State) -> Result<()> {
    if let Some(p) = st.pending.take() {
        finish(st, p);
    }
    st.save(ctx)
}

fn finish(st: &mut State, p: Pending) {
    for (m, keys) in p.new_module_keys {
        st.module_keys.insert(m, keys);
    }
    st.modules = p.new_modules;
    if let Some(e) = p.new_extensions {
        st.applied_extensions = e;
    }
}

/// Undoes the begun steps of the pending apply (newest first), clears the record and saves.
/// Returns what could not be undone (as warnings); the record is cleared in any case, so a
/// broken rollback is not retried forever.
pub(crate) fn rollback(ctx: &Ctx, st: &mut State) -> Vec<String> {
    let Some(p) = st.pending.take() else {
        return vec![];
    };
    let mut w = Vec::new();
    fn note(r: Result<()>, w: &mut Vec<String>) {
        if let Err(e) = r {
            w.push(e.0);
        }
    }
    let prev_look = p
        .prev_modules
        .get(&Module::Look)
        .and_then(|id| get_theme(ctx, id).ok());
    if p.began(Step::Flatpak) {
        note(crate::flatpak::remove(st, &mut w), &mut w);
        if let Some(t) = &prev_look {
            note(crate::flatpak::apply(ctx, st, t, &mut w).map(drop), &mut w);
        }
        note(st.save(ctx), &mut w);
    }
    if p.began(Step::Gtk) {
        note(crate::gtklinks::unlink_all(ctx, st, &mut w), &mut w);
        if let Some(t) = &prev_look {
            note(crate::gtklinks::link_theme(ctx, st, t).map(drop), &mut w);
        }
    }
    if p.began(Step::Dconf) || p.began(Step::Extensions) {
        // like the apply itself (GNOME Shell 51.1 workaround): theme keys first, the extension
        // list only after the shell has processed them
        let is_ext = |k: &String| k == crate::EXT_KEY || k == crate::DIS_KEY;
        let (ext, keys): (BTreeMap<_, _>, BTreeMap<_, _>) = p
            .dconf_before
            .clone()
            .into_iter()
            .partition(|(k, _)| is_ext(k));
        let shell = ctx.shell();
        let watch = shell.as_ref().and_then(crate::shell::Shell::watch_dconf);
        let changed: Vec<String> = match ctx.dconf.dump() {
            Ok(now) => keys
                .iter()
                .filter(|(k, v)| now.get(*k) != v.as_ref())
                .map(|(k, _)| k.clone())
                .collect(),
            Err(_) => keys.keys().cloned().collect(),
        };
        note(ctx.dconf.restore(&keys), &mut w);
        if let (Some(sh), Some(watch)) = (&shell, watch)
            && !changed.is_empty()
        {
            sh.after_dconf_change(watch, &changed, std::time::Duration::from_secs(3));
        }
        note(ctx.dconf.restore(&ext), &mut w);
    }
    if p.began(Step::Tokens) {
        for m in [Module::Look, Module::Apps] {
            if !p.modules.contains(&m) {
                continue;
            }
            crate::tokens::unlink_module(st, m, &mut w);
            if let Some(t) = p
                .prev_modules
                .get(&m)
                .and_then(|id| get_theme(ctx, id).ok())
            {
                note(
                    crate::tokens::link_module(ctx, st, m, &t, &mut w).map(drop),
                    &mut w,
                );
            }
        }
    }
    note(st.save(ctx), &mut w);
    w
}

/// What [`recover`] did with an interrupted apply.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Recovery {
    pub theme: String,
    pub modules: Vec<Module>,
    pub started: String,
    /// The last step begun before the interruption.
    pub step: Option<Step>,
    /// true: completed (hooks had run); false: rolled back.
    pub completed: bool,
    pub warnings: Vec<String>,
}

impl Recovery {
    pub fn message(&self) -> String {
        let mods = self
            .modules
            .iter()
            .map(|m| m.id())
            .collect::<Vec<_>>()
            .join(", ");
        if self.completed {
            tr!(
                "the interrupted apply of “{theme}” ({modules}, {started}) was completed",
                theme = self.theme,
                modules = mods,
                started = self.started
            )
        } else {
            tr!(
                "the interrupted apply of “{theme}” ({modules}, {started}) was rolled back",
                theme = self.theme,
                modules = mods,
                started = self.started
            )
        }
    }
}

/// Description of an interrupted apply for `themes doctor` (no change).
pub fn describe(p: &Pending) -> String {
    tr!(
        "an apply of “{theme}” started {started} was interrupted at the step {step}; “themes doctor --fix” finishes it",
        theme = p.theme,
        started = p.started,
        step = p
            .begun
            .last()
            .map(|s| format!("{s:?}").to_lowercase())
            .unwrap_or_else(|| "-".into())
    )
}

/// Finishes an interrupted apply recorded in `st` (the caller holds the lock): completes it
/// when its hooks had begun, else rolls it back.
pub(crate) fn recover_locked(ctx: &Ctx, st: &mut State) -> Result<Option<Recovery>> {
    let Some(p) = st.pending.clone() else {
        return Ok(None);
    };
    let completed = p.completes();
    let warnings = if completed {
        commit(ctx, st)?;
        vec![]
    } else {
        rollback(ctx, st)
    };
    let r = Recovery {
        theme: p.theme,
        modules: p.modules,
        started: p.started,
        step: p.begun.last().copied(),
        completed,
        warnings,
    };
    log(
        ctx,
        &format!(
            "recover {} step={:?} completed={}", // i18n-ok
            r.theme, r.step, r.completed
        ),
    );
    Ok(Some(r))
}

/// [`recover_locked`] under the lock.
pub fn recover(ctx: &Ctx) -> Result<Option<Recovery>> {
    let _lock = Lock::acquire(ctx)?;
    let mut st = State::load(ctx);
    recover_locked(ctx, &mut st)
}

#[cfg(test)]
mod tests {
    //! `apply` on a temporary HOME and theme root with the in-memory dconf: failures injected
    //! at each step must leave the old selection, dconf values and the user's files as they
    //! were; a simulated crash must be finished by `recover`.
    use super::*;
    use crate::apply::{Options, apply};
    use crate::dconf::{Backend, Settings, mem::Db};
    use crate::theme::tests::{tmp_root, write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    const KEY_A: &str = "/org/example/txn/color";
    const KEY_B: &str = "/org/example/txn/size";
    const KEY_APPS: &str = "/org/example/txn/term";

    struct Fx {
        root: PathBuf,
        ctx: Ctx,
        db: Arc<Mutex<Db>>,
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            let _ =
                std::fs::set_permissions(&self.ctx.gtk4, std::fs::Permissions::from_mode(0o755));
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn theme(ctx: &Ctx, id: &str, color: &str, files: &[(&str, &str)]) {
        let d = ctx.themes.join(id);
        write(&d.join("theme.conf"), &format!("NAME=\"{id}\"\n"));
        write(
            &d.join("settings.dconf"),
            &format!("[org/example/txn]\ncolor='{color}'\nsize=2\n"),
        );
        write(
            &d.join("apps.dconf"),
            &format!("[org/example/txn]\nterm='{id}'\n"),
        );
        write(&d.join("gtk4/gtk.css"), &format!("/* {id} */\n"));
        for (f, body) in files {
            write(&d.join(f), body);
        }
    }

    fn hook(ctx: &Ctx, id: &str, body: &str) {
        let p = ctx.themes.join(id).join("apps/hooks/apply");
        write(&p, &format!("#!/bin/sh\n{body}\n"));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn fx(tag: &str) -> Fx {
        let root = tmp_root(tag);
        let mut ctx = Ctx::new(root.join("home"), root.clone());
        let db = Arc::new(Mutex::new(Db::default()));
        db.lock()
            .unwrap()
            .data
            .insert(KEY_A.into(), "'user'".into());
        ctx.dconf = Backend::Memory(db.clone());
        write(
            &root.join("baseline-t/full.dconf"),
            "[org/example/txn]\ncolor='user'\n",
        );
        theme(&ctx, "a", "red", &[]);
        theme(
            &ctx,
            "b",
            "blue",
            &[("extensions.txt", "b-ext@example.org\n")],
        );
        // the user's own GTK file, which the first apply moves to a backup
        write(&ctx.gtk4.join("gtk.css"), "/* mine */\n");
        Fx { root, ctx, db }
    }

    fn data(f: &Fx) -> Settings {
        f.db.lock().unwrap().data.clone()
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    fn ok(f: &Fx, id: &str) -> crate::apply::ApplyResult {
        apply(&f.ctx, id, &[], &Options::default()).unwrap()
    }

    /// Everything observable that a failed apply must leave as it was.
    fn snapshot(f: &Fx) -> (Settings, String, State) {
        let mut st = State::load(&f.ctx);
        st.last_doctor = None;
        (data(f), read(&f.ctx.gtk4.join("gtk.css")), st)
    }

    fn same(a: &(Settings, String, State), b: &(Settings, String, State)) {
        assert_eq!(a.0, b.0, "dconf");
        assert_eq!(a.1, b.1, "gtk.css");
        assert_eq!(a.2.modules, b.2.modules, "selection");
        assert_eq!(a.2.module_keys, b.2.module_keys, "managed keys");
        assert_eq!(a.2.links, b.2.links, "links");
        assert_eq!(a.2.backups, b.2.backups, "backups");
        assert_eq!(a.2.applied_extensions, b.2.applied_extensions);
        assert!(b.2.pending.is_none(), "journal left behind");
    }

    #[test]
    fn apply_commits_and_reset_restores_user_file() {
        let f = fx("txn-ok");
        let r = ok(&f, "a");
        assert!(r.ok, "{r:?}");
        let st = State::load(&f.ctx);
        assert!(st.pending.is_none());
        assert!(Module::ALL.iter().all(|m| st.source(*m) == Some("a")));
        assert_eq!(data(&f)[KEY_A], "'red'");
        assert_eq!(read(&f.ctx.gtk4.join("gtk.css")), "/* a */\n");
        let dest = f.ctx.gtk4.join("gtk.css").to_string_lossy().into_owned();
        assert_eq!(read(Path::new(&st.backups[&dest])), "/* mine */\n");
        assert!(st.module_keys[&Module::Look].contains(&KEY_B.to_string()));
        // switching themes keeps the one backup of the user's file
        ok(&f, "b");
        let st = State::load(&f.ctx);
        assert_eq!(read(&f.ctx.gtk4.join("gtk.css")), "/* b */\n");
        assert_eq!(st.applied_extensions, vec!["b-ext@example.org"]);
        let mut w = vec![];
        let mut st = State::load(&f.ctx);
        crate::gtklinks::unlink_all(&f.ctx, &mut st, &mut w).unwrap();
        assert_eq!(read(&f.ctx.gtk4.join("gtk.css")), "/* mine */\n");
        let st = State::load(&f.ctx);
        assert!(st.links.is_empty() && st.backups.is_empty(), "{st:?}");
    }

    #[test]
    fn link_unlink_and_orphan_backups() {
        let f = fx("txn-links");
        let t = get_theme(&f.ctx, "a").unwrap();
        let mut st = State::load(&f.ctx);
        let made = crate::gtklinks::link_theme(&f.ctx, &mut st, &t).unwrap();
        assert_eq!(made.len(), 1);
        // the record is on disk at once, not only in memory
        let on_disk = State::load(&f.ctx);
        assert_eq!(on_disk.links.len(), 1);
        assert_eq!(on_disk.backups.len(), 1);
        // a backup record whose file never got moved (crash between record and move) is dropped
        st.backups
            .insert("/nonexistent/gtk.css".into(), "/nonexistent/b".into());
        let mut w = vec![];
        crate::gtklinks::unlink_all(&f.ctx, &mut st, &mut w).unwrap();
        assert!(st.backups.is_empty() && st.links.is_empty(), "{st:?}");
        assert_eq!(read(&f.ctx.gtk4.join("gtk.css")), "/* mine */\n");
        assert!(w.is_empty(), "{w:?}");
        // a link the user replaced is left alone, and the backup is not forced over it
        crate::gtklinks::link_theme(&f.ctx, &mut st, &t).unwrap();
        let dest = f.ctx.gtk4.join("gtk.css");
        std::fs::remove_file(&dest).unwrap();
        std::fs::write(&dest, "/* edited */\n").unwrap();
        crate::gtklinks::unlink_all(&f.ctx, &mut st, &mut w).unwrap();
        assert_eq!(read(&dest), "/* edited */\n");
        assert_eq!(w.len(), 2, "{w:?}");
    }

    #[test]
    fn failing_dconf_rolls_back() {
        let f = fx("txn-dconf");
        ok(&f, "a");
        let before = snapshot(&f);
        f.db.lock().unwrap().fail_key = Some(KEY_A.into());
        let e = apply(&f.ctx, "b", &[], &Options::default()).unwrap_err();
        assert!(e.0.contains("rolled back"), "{e}");
        f.db.lock().unwrap().fail_key = None;
        same(&before, &snapshot(&f));
    }

    #[test]
    fn failing_extension_write_restores_written_keys() {
        let f = fx("txn-ext");
        ok(&f, "a");
        let before = snapshot(&f);
        // the theme keys are written, then the extension list fails
        f.db.lock().unwrap().fail_key = Some(crate::EXT_KEY.into());
        assert!(apply(&f.ctx, "b", &[], &Options::default()).is_err());
        f.db.lock().unwrap().fail_key = None;
        same(&before, &snapshot(&f));
        assert_eq!(data(&f)[KEY_A], "'red'");
    }

    #[test]
    fn read_only_gtk_dir_rolls_back() {
        if unsafe { libc::geteuid() } == 0 {
            return; // root ignores the permission bits this test relies on
        }
        let f = fx("txn-ro");
        let before = snapshot(&f);
        std::fs::set_permissions(&f.ctx.gtk4, std::fs::Permissions::from_mode(0o555)).unwrap();
        let e = apply(&f.ctx, "a", &[], &Options::default()).unwrap_err();
        assert!(e.0.contains("rolled back"), "{e}");
        std::fs::set_permissions(&f.ctx.gtk4, std::fs::Permissions::from_mode(0o755)).unwrap();
        same(&before, &snapshot(&f));
        assert_eq!(data(&f)[KEY_A], "'user'");
        // and it works once the directory is writable again
        ok(&f, "a");
    }

    #[test]
    fn failing_hook_is_a_warning() {
        let f = fx("txn-hook");
        hook(&f.ctx, "a", "echo broken >&2; exit 7");
        let r = ok(&f, "a");
        assert!(r.warnings.iter().any(|w| w.contains("broken")), "{r:?}");
        let st = State::load(&f.ctx);
        assert_eq!(st.hooks_applied, None);
        assert_eq!(st.source(Module::Apps), Some("a"));
        assert!(st.pending.is_none());
        hook(&f.ctx, "b", "exit 0");
        ok(&f, "b");
        assert_eq!(State::load(&f.ctx).hooks_applied.as_deref(), Some("b"));
    }

    fn crash(f: &Fx, id: &str, at: Step) {
        f.db.lock().unwrap().crash_at = Some(at);
        let ctx = f.ctx.clone();
        let id = id.to_string();
        let r = std::panic::catch_unwind(move || apply(&ctx, &id, &[], &Options::default()));
        assert!(r.is_err(), "no simulated crash");
        f.db.lock().unwrap().crash_at = None;
    }

    #[test]
    fn crash_mid_apply_is_rolled_back_by_recover() {
        let f = fx("txn-crash");
        ok(&f, "a");
        let before = snapshot(&f);
        crash(&f, "b", Step::Flatpak);
        // dconf and GTK files are half way to b; the journal says so
        let st = State::load(&f.ctx);
        let p = st.pending.clone().expect("journal");
        assert_eq!(p.begun.last(), Some(&Step::Flatpak));
        assert_eq!(data(&f)[KEY_A], "'blue'");
        assert!(describe(&p).contains("flatpak"));
        let r = recover(&f.ctx).unwrap().unwrap();
        assert!(!r.completed && r.warnings.is_empty(), "{r:?}");
        same(&before, &snapshot(&f));
        assert_eq!(recover(&f.ctx).unwrap(), None);
    }

    #[test]
    fn crash_after_hooks_is_completed() {
        let f = fx("txn-crash-hooks");
        ok(&f, "a");
        crash(&f, "b", Step::Hooks);
        let r = recover(&f.ctx).unwrap().unwrap();
        assert!(r.completed, "{r:?}");
        let st = State::load(&f.ctx);
        assert!(Module::ALL.iter().all(|m| st.source(*m) == Some("b")));
        assert!(st.module_keys[&Module::Apps].contains(&KEY_APPS.to_string()));
        assert_eq!(st.applied_extensions, vec!["b-ext@example.org"]);
    }

    #[test]
    fn next_apply_recovers_first() {
        let f = fx("txn-crash-next");
        ok(&f, "a");
        crash(&f, "b", Step::Dconf);
        let r = ok(&f, "b");
        assert!(
            r.warnings.iter().any(|w| w.contains("rolled back")),
            "{r:?}"
        );
        let st = State::load(&f.ctx);
        assert!(st.pending.is_none() && st.source(Module::Look) == Some("b"));
        // a dry run reports an interrupted apply and changes nothing
        crash(&f, "a", Step::Tokens);
        let opts = Options {
            dry_run: true,
            ..Default::default()
        };
        let r = apply(&f.ctx, "a", &[], &opts).unwrap();
        assert!(
            r.warnings.iter().any(|w| w.contains("interrupted")),
            "{r:?}"
        );
        assert!(State::load(&f.ctx).pending.is_some());
    }

    #[test]
    fn experimental_theme_needs_confirmation() {
        let f = fx("txn-exp");
        theme(
            &f.ctx,
            "x",
            "green",
            &[(
                "theme.conf",
                "NAME=\"X\"\nEXPERIMENTAL=true\nWARNING=\"may crash\"\n",
            )],
        );
        let before = snapshot(&f);
        let e = apply(&f.ctx, "x", &[Module::Behavior], &Options::default()).unwrap_err();
        assert!(e.0.contains("may crash"), "{e}");
        same(&before, &snapshot(&f));
        let dry = Options {
            dry_run: true,
            ..Default::default()
        };
        assert!(apply(&f.ctx, "x", &[], &dry).is_ok());
        let yes = Options {
            confirmed: true,
            ..Default::default()
        };
        assert!(apply(&f.ctx, "x", &[], &yes).unwrap().ok);
    }
}
