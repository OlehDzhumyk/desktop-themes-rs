// SPDX-License-Identifier: GPL-3.0-or-later
//! `themes focus status|list|start|session|stop|set|delete`: focus modes of deskd
//! (`io.github.OlehDzhumyk.Desk.Focus`). deskd owns the state; this is a thin client.

use clap::Subcommand;
use desk_proto::focus::{FocusProxy, Status};
use desk_settings::{n_, tr};
use gtk::glib;
use serde_json::{Value, json};
use themes_core::Error;

// Help texts (gettext ids; cli.rs translates them at start-up).
const F01: &str = n_("Show the focus state and the next scheduled start");
const F02: &str = n_("List the focus modes");
const F03: &str = n_("Start a focus mode (until you end it, or for a time)");
const F04: &str = n_("Mode id, see `themes focus list`");
const F05: &str = n_("For how long, e.g. 30m, 2h, 1h30m");
const F06: &str = n_("Until a time of day (HH:MM, today or tomorrow)");
const F07: &str = n_("Start a focus session: do not disturb for some minutes, then a break");
const F08: &str = n_("Minutes (default: the session-length setting)");
const F09: &str = n_("End the active focus mode");
const F10: &str = n_("Add or change a focus mode");
const F11: &str = n_("Delete a custom focus mode");
const F12: &str = n_("Display name");
const F13: &str = n_("Hide notification banners (true or false)");
const F14: &str = n_("Mute event sounds (true or false)");
const F15: &str = n_("App id whose banners still show (repeat; replaces the list)");
const F16: &str = n_("Schedule days, e.g. mon,tue,wed,thu,fri");
const F17: &str = n_("Schedule start (HH:MM)");
const F18: &str = n_("Schedule end (HH:MM; earlier than the start means the next morning)");
const F19: &str = n_("Remove the schedule");
const F20: &str = n_("Allow no apps (clear the list)");

#[derive(Subcommand)]
pub enum FocusCmd {
    #[command(about = F01)]
    Status,
    #[command(about = F02)]
    List,
    #[command(about = F03)]
    Start {
        #[arg(value_name = "ID", help = F04)]
        id: String,
        #[arg(long = "for", value_name = "DURATION", help = F05, conflicts_with = "until")]
        duration: Option<String>,
        #[arg(long, value_name = "HH:MM", help = F06)]
        until: Option<String>,
    },
    #[command(about = F07)]
    Session {
        #[arg(value_name = "MINUTES", help = F08)]
        minutes: Option<u32>,
    },
    #[command(about = F09)]
    Stop,
    #[command(about = F10)]
    Set {
        #[arg(value_name = "ID", help = F04)]
        id: String,
        #[arg(long, help = F12)]
        name: Option<String>,
        #[arg(long, value_name = "BOOL", help = F13)]
        block_banners: Option<bool>,
        #[arg(long, value_name = "BOOL", help = F14)]
        mute_sounds: Option<bool>,
        #[arg(long = "allow", value_name = "APP", help = F15)]
        allow: Vec<String>,
        #[arg(long, help = F20, conflicts_with = "allow")]
        allow_none: bool,
        #[arg(long, value_name = "DAYS", help = F16, requires_all = ["from", "to"])]
        days: Option<String>,
        #[arg(long, value_name = "HH:MM", help = F17, requires = "days")]
        from: Option<String>,
        #[arg(long, value_name = "HH:MM", help = F18, requires = "days")]
        to: Option<String>,
        #[arg(long, help = F19, conflicts_with = "days")]
        no_schedule: bool,
    },
    #[command(about = F11)]
    Delete {
        #[arg(value_name = "ID", help = F04)]
        id: String,
    },
}

fn now_unix() -> i64 {
    glib::DateTime::now_local().map_or(0, |n| n.to_unix())
}

/// Local "HH:MM" of a Unix time (with the weekday when it is not within the next 20 hours).
pub fn fmt_time(unix: i64) -> String {
    let Ok(dt) = glib::DateTime::from_unix_local(unix) else {
        return String::new();
    };
    let pattern = if unix - now_unix() < 20 * 3600 {
        "%H:%M"
    } else {
        "%a %H:%M"
    };
    dt.format(pattern).map_or_default(|s| s.to_string())
}

/// The state in words, for the CLI, the window and screen readers («Фокус: Робота до 17:00»).
pub fn status_line(st: &Status) -> String {
    let (mode, name, _, source, until, break_until, next_mode, next_start, _) = st;
    if !mode.is_empty() {
        return match (*until > 0, source == "session") {
            (true, true) => tr!("Focus session until {time}", time = fmt_time(*until)),
            (true, false) => tr!(
                "Focus: {mode} until {time}",
                mode = name,
                time = fmt_time(*until)
            ),
            (false, _) => tr!("Focus: {mode}", mode = name),
        };
    }
    if *break_until > 0 {
        return tr!("Break until {time}", time = fmt_time(*break_until));
    }
    if *next_start > 0 {
        return tr!(
            "Focus is off; {mode} starts at {time}",
            mode = next_mode,
            time = fmt_time(*next_start)
        );
    }
    tr!("Focus is off")
}

/// "90m", "2h", "1h30m", "45" (minutes) → seconds.
pub fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Ok(m) = s.parse::<i64>() {
        return (m > 0).then_some(m * 60);
    }
    let mut total = 0i64;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: i64 = num.parse().ok()?;
        num.clear();
        total += match c {
            'h' => n * 3600,
            'm' => n * 60,
            _ => return None,
        };
    }
    (num.is_empty() && total > 0).then_some(total)
}

/// The next local occurrence of "HH:MM" as Unix time (today, or tomorrow when it has passed).
pub fn next_time_of_day(hhmm: &str) -> Option<i64> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    let now = glib::DateTime::now_local().ok()?;
    let today =
        glib::DateTime::from_local(now.year(), now.month(), now.day_of_month(), h, m, 0.0).ok()?;
    let t = if today.to_unix() > now.to_unix() {
        today
    } else {
        today.add_days(1).ok()?
    };
    Some(t.to_unix())
}

/// One call to deskd's focus engine on the session bus.
pub fn call<T>(f: impl AsyncFnOnce(&FocusProxy<'static>) -> zbus::Result<T>) -> Result<T, Error> {
    zbus::block_on(async {
        let conn = zbus::Connection::session().await?;
        let p = FocusProxy::new(&conn).await?;
        f(&p).await
    })
    .map_err(|e| Error(tr!("deskd focus service: {error}", error = e)))
}

/// The modes as deskd lists them (JSON objects).
pub fn modes() -> Result<Vec<Value>, Error> {
    let json = call(async |p| p.list_modes().await)?;
    serde_json::from_str(&json).map_err(|e| Error(e.to_string()))
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn print_modes(list: &[Value]) {
    for m in list {
        let s = |k: &str| m[k].as_str().unwrap_or("").to_string();
        let sched = m["schedule"].as_object().map_or_else(String::new, |o| {
            let days: Vec<&str> = o["days"]
                .as_array()
                .map_or_default(|a| a.iter().filter_map(Value::as_str).collect());
            format!(
                "  {} {}–{}",
                days.join(","),
                o["start"].as_str().unwrap_or(""),
                o["end"].as_str().unwrap_or("")
            )
        });
        let mut flags = vec![];
        if m["block_banners"].as_bool() == Some(true) {
            flags.push(tr!("banners hidden"));
        }
        if m["mute_sounds"].as_bool() == Some(true) {
            flags.push(tr!("sounds muted"));
        }
        println!(
            "{:<12} {:<18} [{}]{}",
            s("id"),
            s("label"),
            flags.join(", "),
            sched
        );
        if let Some(a) = m["allowed_apps"].as_array().filter(|a| !a.is_empty()) {
            let apps: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
            println!("  {}", tr!("allowed: {apps}", apps = apps.join(", ")));
        }
    }
}

/// The mode `id` (or a new one) with the given changes, as the JSON `SaveMode` takes.
#[allow(clippy::too_many_arguments, reason = "one argument per CLI flag")]
fn edited_mode(
    existing: Option<Value>,
    id: &str,
    name: Option<String>,
    block_banners: Option<bool>,
    mute_sounds: Option<bool>,
    allowed: Option<Vec<String>>,
    schedule: Option<Value>,
) -> String {
    let mut m = existing.unwrap_or_else(|| json!({ "id": id, "name": id }));
    if let Some(o) = m.as_object_mut() {
        o.remove("label");
        o.remove("builtin");
        if let Some(n) = name {
            o.insert("name".into(), n.into());
        }
        if let Some(b) = block_banners {
            o.insert("block_banners".into(), b.into());
        }
        if let Some(b) = mute_sounds {
            o.insert("mute_sounds".into(), b.into());
        }
        if let Some(a) = allowed {
            o.insert("allowed_apps".into(), json!(a));
        }
        if let Some(s) = schedule {
            o.insert("schedule".into(), s);
        }
    }
    m.to_string()
}

pub fn exec(cmd: FocusCmd, json_out: bool) -> Result<i32, Error> {
    match cmd {
        FocusCmd::Status => {
            let st = call(async |p| p.status().await)?;
            if json_out {
                let (mode, name, icon, source, until, break_until, next_mode, next_start, allowed) =
                    st.clone();
                print(&json!({
                    "mode": mode, "name": name, "icon": icon, "source": source, "until": until,
                    "break_until": break_until, "next_mode": next_mode, "next_start": next_start,
                    "allowed_apps": allowed, "text": status_line(&st),
                }));
            } else {
                println!("{}", status_line(&st));
            }
        }
        FocusCmd::List => {
            let list = modes()?;
            if json_out {
                print(&Value::Array(list));
            } else {
                print_modes(&list);
            }
        }
        FocusCmd::Start {
            id,
            duration,
            until,
        } => {
            let end = match (duration, until) {
                (Some(d), _) => {
                    now_unix()
                        + parse_duration(&d)
                            .ok_or_else(|| Error(tr!("invalid duration “{value}”", value = d)))?
                }
                (None, Some(u)) => next_time_of_day(&u)
                    .ok_or_else(|| Error(tr!("invalid time “{value}”", value = u)))?,
                (None, None) => 0,
            };
            call(async |p| p.start(&id, end).await)?;
            if !json_out {
                println!("{}", status_line(&call(async |p| p.status().await)?));
            }
        }
        FocusCmd::Session { minutes } => {
            call(async |p| p.start_session(minutes.unwrap_or(0)).await)?;
            if !json_out {
                println!("{}", status_line(&call(async |p| p.status().await)?));
            }
        }
        FocusCmd::Stop => {
            call(async |p| p.stop().await)?;
            if !json_out {
                println!("{}", tr!("Focus is off"));
            }
        }
        FocusCmd::Set {
            id,
            name,
            block_banners,
            mute_sounds,
            allow,
            allow_none,
            days,
            from,
            to,
            no_schedule,
        } => {
            let existing = modes()?
                .into_iter()
                .find(|m| m["id"].as_str() == Some(id.as_str()));
            let allowed = (allow_none || !allow.is_empty()).then_some(allow);
            let schedule = if no_schedule {
                Some(Value::Null)
            } else if let (Some(d), Some(f), Some(t)) = (days, from, to) {
                let days: Vec<String> = d.split(',').map(|x| x.trim().to_lowercase()).collect();
                Some(json!({"days": days, "start": f, "end": t}))
            } else {
                None
            };
            let body = edited_mode(
                existing,
                &id,
                name,
                block_banners,
                mute_sounds,
                allowed,
                schedule,
            );
            call(async |p| p.save_mode(&body).await)?;
            if json_out {
                print(&json!({"ok": true}));
            }
        }
        FocusCmd::Delete { id } => {
            call(async |p| p.delete_mode(&id).await)?;
            if json_out {
                print(&json!({"ok": true}));
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("45"), Some(45 * 60));
        assert_eq!(parse_duration("2h"), Some(7200));
        assert_eq!(parse_duration("1h30m"), Some(5400));
        assert_eq!(parse_duration("90m"), Some(5400));
        for bad in ["", "0", "h", "1x", "1h30"] {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }

    #[test]
    fn edits_merge_into_the_existing_mode() {
        let old = json!({"id": "work", "name": "Work", "label": "Робота", "builtin": true,
            "mute_sounds": false, "allowed_apps": ["a"]});
        let v: Value = serde_json::from_str(&edited_mode(
            Some(old),
            "work",
            None,
            None,
            Some(true),
            None,
            Some(Value::Null),
        ))
        .unwrap();
        assert_eq!(v["name"], "Work", "untouched fields stay");
        assert_eq!(v["allowed_apps"], json!(["a"]));
        assert_eq!(v["mute_sounds"], true);
        assert!(v.get("label").is_none() && v.get("builtin").is_none());
    }
}
