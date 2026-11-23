// SPDX-License-Identifier: GPL-3.0-or-later
//! App > System page: inline doctor check/fix, baseline info, and "Reset all".

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use desk_settings::tr;
use gtk::{gio, glib};
use themes_core::Ctx;
use themes_core::doctor::{DoctorOptions, DoctorReport, doctor};
use themes_core::state::State;

/// Build the System preferences page: inline doctor, baseline info, and Reset all.
pub fn build_page(ctx: &Ctx, toasts: &adw::ToastOverlay) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .description(tr!(
            "System health check, baseline snapshot information, and full reset to defaults."
        ))
        .build();

    let (doctor_group, issues_group) = build_doctor_groups(ctx, toasts);
    page.add(&doctor_group);
    page.add(&issues_group);
    page.add(&build_baseline_group(ctx));
    page.add(&build_reset_group(ctx, toasts));
    page
}

fn build_doctor_groups(
    ctx: &Ctx,
    toasts: &adw::ToastOverlay,
) -> (adw::PreferencesGroup, adw::PreferencesGroup) {
    let group = adw::PreferencesGroup::builder()
        .title(tr!("System check"))
        .description(tr!(
            "Check GNOME Shell extensions, themes, icons, sounds and GSettings schemas."
        ))
        .build();
    let status_row = adw::ActionRow::builder()
        .title(tr!("Status"))
        .subtitle(tr!("Not checked yet"))
        .build();
    let check_row = adw::ButtonRow::builder().title(tr!("Check now")).build();
    let fix_row = adw::ButtonRow::builder().title(tr!("Fix issues")).build();
    fix_row.set_sensitive(false);

    group.add(&status_row);
    group.add(&check_row);
    group.add(&fix_row);

    let issues_group = adw::PreferencesGroup::builder()
        .title(tr!("Check results"))
        .visible(false)
        .build();
    let issue_rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));

    let run_check = {
        let ctx = ctx.clone();
        let toasts = toasts.clone();
        let status_row = status_row.clone();
        let check_row = check_row.clone();
        let fix_row = fix_row.clone();
        let issues_group = issues_group.clone();
        let issue_rows = issue_rows.clone();
        Rc::new(move |fix: bool| {
            check_row.set_sensitive(false);
            fix_row.set_sensitive(false);
            status_row.set_subtitle(&if fix {
                tr!("Fixing…")
            } else {
                tr!("Checking the system…")
            });
            let ctx = ctx.clone();
            let toasts = toasts.clone();
            let status_row = status_row.clone();
            let check_row = check_row.clone();
            let fix_row = fix_row.clone();
            let issues_group = issues_group.clone();
            let issue_rows = issue_rows.clone();
            glib::spawn_future_local(async move {
                let res = gio::spawn_blocking(move || {
                    doctor(
                        &ctx,
                        &DoctorOptions {
                            fix,
                            wait_shell: Duration::ZERO,
                            notify: false,
                        },
                    )
                })
                .await;
                check_row.set_sensitive(true);
                match res {
                    Ok(Ok(rep)) => {
                        render_report(&rep, fix, &status_row, &fix_row, &issues_group, &issue_rows);
                        if fix {
                            let _ = status_row.activate_action("win.reload", None);
                        }
                    }
                    Ok(Err(e)) => {
                        let msg = tr!("Check failed: {error}", error = e);
                        status_row.set_subtitle(&msg);
                        toasts.add_toast(adw::Toast::new(&msg));
                    }
                    Err(_) => {
                        let msg = tr!("Check failed");
                        status_row.set_subtitle(&msg);
                        toasts.add_toast(adw::Toast::new(&msg));
                    }
                }
            });
        })
    };

    {
        let run = run_check.clone();
        check_row.connect_activated(move |_| run(false));
    }
    {
        let run = run_check;
        fix_row.connect_activated(move |_| run(true));
    }

    (group, issues_group)
}

fn render_report(
    rep: &DoctorReport,
    fixed_run: bool,
    status_row: &adw::ActionRow,
    fix_row: &adw::ButtonRow,
    issues_group: &adw::PreferencesGroup,
    issue_rows: &Rc<RefCell<Vec<adw::ActionRow>>>,
) {
    let summary = tr!(
        "GNOME Shell {version} ({state}), extensions checked: {count}.",
        version = rep.gnome_version.clone().unwrap_or_else(|| "?".into()),
        state = if rep.shell_running {
            tr!("running")
        } else {
            tr!("not running")
        },
        count = rep.extensions_checked
    );
    let problems = rep.problems();
    let status_text = if problems == 0 {
        format!("{summary} {}", tr!("No problems found."))
    } else {
        format!(
            "{summary} {}",
            tr!("Problems found: {count}", count = problems)
        )
    };
    status_row.set_subtitle(&status_text);

    for old in issue_rows.borrow_mut().drain(..) {
        issues_group.remove(&old);
    }

    let mut rows = issue_rows.borrow_mut();
    for issue in &rep.issues {
        let icon = match issue.severity {
            "error" => "dialog-error-symbolic",
            "warning" => "dialog-warning-symbolic",
            _ => "dialog-information-symbolic",
        };
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&issue.message).as_str())
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(icon));
        issues_group.add(&row);
        rows.push(row);
    }
    for f in &rep.fixed {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&tr!("Fixed: {what}", what = f)).as_str())
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("emblem-ok-symbolic"));
        issues_group.add(&row);
        rows.push(row);
    }
    for u in &rep.restored {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&tr!("Turned on again: {uuid}", uuid = u)).as_str())
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("emblem-ok-symbolic"));
        issues_group.add(&row);
        rows.push(row);
    }

    issues_group.set_visible(!rows.is_empty());
    let has_fixable = !fixed_run && rep.issues.iter().any(|i| i.fixable);
    fix_row.set_sensitive(has_fixable);
}

fn build_baseline_group(ctx: &Ctx) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(tr!("Baseline"))
        .build();
    let baseline_sub = match ctx.baseline_dir() {
        Ok(p) => ctx.rel(&p).display().to_string(),
        Err(_) => tr!("Not created yet (created on the first theme apply)"),
    };
    group.add(
        &adw::ActionRow::builder()
            .title(tr!("Baseline snapshot"))
            .subtitle(glib::markup_escape_text(&baseline_sub).as_str())
            .build(),
    );
    let themes_dir = ctx.rel(&ctx.themes).display().to_string();
    group.add(
        &adw::ActionRow::builder()
            .title(tr!("Themes directory"))
            .subtitle(glib::markup_escape_text(&themes_dir).as_str())
            .build(),
    );
    let managed = State::load(ctx).applied_keys.len().to_string();
    group.add(
        &adw::ActionRow::builder()
            .title(tr!("Managed keys"))
            .subtitle(managed)
            .build(),
    );
    group
}

fn build_reset_group(ctx: &Ctx, toasts: &adw::ToastOverlay) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(tr!("Reset"))
        .description(tr!(
            "Restore the standard Fedora look and reset all feature settings to their defaults."
        ))
        .build();
    let reset_row = adw::ButtonRow::builder().title(tr!("Reset all")).build();
    reset_row.add_css_class("destructive-action");

    let ctx = ctx.clone();
    let toasts = toasts.clone();
    reset_row.connect_activated(move |row| {
        let dlg = adw::AlertDialog::builder()
            .heading(tr!("Reset everything to defaults?"))
            .body(tr!(
                "This restores the standard Fedora theme and resets all Desktop feature settings \
                 and keyboard shortcut overrides to their defaults."
            ))
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dlg.add_response("cancel", &tr!("Cancel"));
        dlg.add_response("reset", &tr!("Reset all"));
        dlg.set_response_appearance("reset", adw::ResponseAppearance::Destructive);

        let ctx = ctx.clone();
        let toasts = toasts.clone();
        let row_ref = row.clone();
        dlg.connect_response(None, move |_, resp| {
            if resp != "reset" {
                return;
            }
            super::features::reset_all_settings();
            for sc in desk_settings::SHORTCUTS {
                let _ = themes_core::shortcuts::reset(&ctx, sc.id);
            }
            let _ = row_ref.activate_action("win.reset", None);
            toasts.add_toast(adw::Toast::new(&tr!("All settings reset to defaults")));
        });
        dlg.present(Some(row));
    });

    group.add(&reset_row);
    group
}
