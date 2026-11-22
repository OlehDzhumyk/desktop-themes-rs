// SPDX-License-Identifier: GPL-3.0-or-later
//! Чисті обчислення: керовані ключі по модулях, бажані значення dconf, набір розширень.

use std::collections::{BTreeMap, BTreeSet};

use crate::dconf::Settings;
use crate::modules::Module;

/// Вхідні дані для обчислення ключів dconf.
pub struct DconfInput<'a> {
    pub baseline: &'a Settings,
    pub current: &'a Settings,
    /// Ключі, які кожен модуль записав минулого разу (state.module_keys).
    pub prev_keys: &'a BTreeMap<Module, Vec<String>>,
    /// Ключі, які задає хоч одна тема в кожному модулі.
    pub theme_keys: &'a BTreeMap<Module, BTreeSet<String>>,
    /// Що задає обране джерело кожного модуля (після застосування).
    pub targets: &'a BTreeMap<Module, Settings>,
    /// Модулі, які застосовуються зараз.
    pub affected: &'a [Module],
}

#[derive(Debug, Default, PartialEq)]
pub struct DconfPlan {
    /// Ключі, які треба записати (лише ті, що відрізняються від поточних).
    pub writes: Settings,
    /// Ключі, які треба скинути до типового (лише ті, що зараз задані).
    pub resets: Vec<String>,
    /// Усі бажані значення зачеплених ключів (None — типове схеми).
    pub desired: BTreeMap<String, Option<String>>,
    /// Ключі, що повертаються до baseline/типового, бо джерело модуля їх не задає.
    pub to_baseline: Vec<String>,
}

/// Керовані ключі модуля: записані раніше ∪ задані будь-якою темою ∪ задані ціллю.
pub fn managed(m: Module, inp: &DconfInput) -> BTreeSet<String> {
    let mut s: BTreeSet<String> = inp
        .prev_keys
        .get(&m)
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    if let Some(t) = inp.theme_keys.get(&m) {
        s.extend(t.iter().cloned());
    }
    if let Some(t) = inp.targets.get(&m) {
        s.extend(t.keys().cloned());
    }
    s
}

/// Бажане значення ключа: значення з цілі пізнішого модуля перемагає; інакше baseline; інакше None.
pub fn desired_value(key: &str, inp: &DconfInput) -> Option<String> {
    for m in Module::ALL.iter().rev() {
        if let Some(v) = inp.targets.get(m).and_then(|t| t.get(key)) {
            return Some(v.clone());
        }
    }
    inp.baseline.get(key).cloned()
}

pub fn compute_dconf(inp: &DconfInput) -> DconfPlan {
    let mut plan = DconfPlan::default();
    let mut affected_keys = BTreeSet::new();
    for m in inp.affected {
        affected_keys.extend(managed(*m, inp));
    }
    for k in affected_keys {
        let set_by_target = Module::ALL
            .iter()
            .any(|m| inp.targets.get(m).is_some_and(|t| t.contains_key(&k)));
        if !set_by_target {
            plan.to_baseline.push(k.clone());
        }
        let d = desired_value(&k, inp);
        match &d {
            Some(v) if inp.current.get(&k) != Some(v) => {
                plan.writes.insert(k.clone(), v.clone());
            }
            None if inp.current.contains_key(&k) => plan.resets.push(k.clone()),
            _ => {}
        }
        plan.desired.insert(k, d);
    }
    plan
}

/// Набір розширень після застосування.
pub struct ExtInput<'a> {
    pub base_en: &'a [String],
    pub cur_en: &'a [String],
    pub cur_dis: &'a [String],
    /// Розширення, якими керують теми (усі extensions.txt + застосовані раніше).
    pub theme_exts: &'a BTreeSet<String>,
    /// extensions.txt джерела модуля «вигляд».
    pub target: &'a [String],
    pub permanent: &'a [&'a str],
    pub broken: &'a BTreeSet<String>,
}

pub fn compute_extensions(inp: &ExtInput) -> (Vec<String>, Vec<String>) {
    let mut keep: Vec<String> = Vec::new();
    // baseline + увімкнені вручну, крім тих, якими керують теми
    for u in inp.base_en.iter().chain(inp.cur_en) {
        if !inp.theme_exts.contains(u) && !keep.contains(u) {
            keep.push(u.clone());
        }
    }
    let mut en = keep;
    for u in inp.target {
        if !en.contains(u) {
            en.push(u.clone());
        }
    }
    en.retain(|u| !inp.broken.contains(u));
    for p in inp.permanent {
        if !en.iter().any(|u| u == p) {
            en.push(p.to_string());
        }
    }
    let dis: Vec<String> = inp
        .cur_dis
        .iter()
        .filter(|u| {
            !inp.theme_exts.contains(*u)
                && !inp.target.contains(u)
                && !inp.permanent.contains(&u.as_str())
        })
        .cloned()
        .collect();
    (en, dis)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(pairs: &[(&str, &str)]) -> Settings {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn keys(m: &Settings) -> BTreeSet<String> {
        m.keys().cloned().collect()
    }

    /// Дві теми: A (look+behavior), B (look+apps). Перевіряємо скидання по модулях.
    fn fixture() -> (Settings, BTreeMap<Module, BTreeMap<&'static str, Settings>>) {
        let baseline = s(&[
            ("/i/color-scheme", "'prefer-dark'"),
            ("/tp/two-finger", "true"),
        ]);
        let mut t: BTreeMap<Module, BTreeMap<&str, Settings>> = BTreeMap::new();
        t.entry(Module::Look).or_default().insert(
            "a",
            s(&[("/i/gtk-theme", "'A'"), ("/i/color-scheme", "'default'")]),
        );
        t.entry(Module::Look).or_default().insert(
            "b",
            s(&[("/i/gtk-theme", "'B'"), ("/i/icon-theme", "'Bi'")]),
        );
        t.entry(Module::Behavior).or_default().insert(
            "a",
            s(&[("/m/edge-tiling", "false"), ("/tp/two-finger", "false")]),
        );
        t.entry(Module::Apps)
            .or_default()
            .insert("b", s(&[("/p/palette", "'campbell'")]));
        (baseline, t)
    }

    fn theme_keys(
        t: &BTreeMap<Module, BTreeMap<&str, Settings>>,
    ) -> BTreeMap<Module, BTreeSet<String>> {
        t.iter()
            .map(|(m, th)| (*m, th.values().flat_map(keys).collect()))
            .collect()
    }

    fn targets(
        t: &BTreeMap<Module, BTreeMap<&str, Settings>>,
        sel: &[(Module, &str)],
    ) -> BTreeMap<Module, Settings> {
        sel.iter()
            .map(|(m, id)| {
                (
                    *m,
                    t.get(m)
                        .and_then(|x| x.get(id))
                        .cloned()
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    #[test]
    fn managed_keys_per_module() {
        let (baseline, t) = fixture();
        let tk = theme_keys(&t);
        let prev: BTreeMap<Module, Vec<String>> =
            [(Module::Behavior, vec!["/old/behavior-key".to_string()])].into();
        let tg = targets(&t, &[(Module::Look, "a")]);
        let cur = Settings::new();
        let inp = DconfInput {
            baseline: &baseline,
            current: &cur,
            prev_keys: &prev,
            theme_keys: &tk,
            targets: &tg,
            affected: &[Module::Look],
        };
        assert_eq!(
            managed(Module::Look, &inp),
            ["/i/color-scheme", "/i/gtk-theme", "/i/icon-theme"]
                .iter()
                .map(|x| x.to_string())
                .collect()
        );
        assert_eq!(
            managed(Module::Behavior, &inp),
            ["/m/edge-tiling", "/old/behavior-key", "/tp/two-finger"]
                .iter()
                .map(|x| x.to_string())
                .collect()
        );
        assert!(managed(Module::Sounds, &inp).is_empty());
    }

    #[test]
    fn full_apply_then_switch_resets_to_baseline() {
        let (baseline, t) = fixture();
        let tk = theme_keys(&t);
        let prev = BTreeMap::new();
        // поточний стан = baseline
        let cur = baseline.clone();
        let sel_a = [
            (Module::Look, "a"),
            (Module::Behavior, "a"),
            (Module::Sounds, "a"),
            (Module::Apps, "a"),
        ];
        let tg = targets(&t, &sel_a);
        let inp = DconfInput {
            baseline: &baseline,
            current: &cur,
            prev_keys: &prev,
            theme_keys: &tk,
            targets: &tg,
            affected: &Module::ALL,
        };
        let p = compute_dconf(&inp);
        assert_eq!(
            p.writes,
            s(&[
                ("/i/gtk-theme", "'A'"),
                ("/i/color-scheme", "'default'"),
                ("/m/edge-tiling", "false"),
                ("/tp/two-finger", "false")
            ])
        );
        assert!(p.resets.is_empty());

        // стан після A; перемикаємося на B повністю
        let mut cur_a = cur.clone();
        cur_a.extend(p.writes.clone());
        let sel_b = [
            (Module::Look, "b"),
            (Module::Behavior, "b"),
            (Module::Sounds, "b"),
            (Module::Apps, "b"),
        ];
        let tg = targets(&t, &sel_b);
        let inp = DconfInput {
            baseline: &baseline,
            current: &cur_a,
            prev_keys: &prev,
            theme_keys: &tk,
            targets: &tg,
            affected: &Module::ALL,
        };
        let p = compute_dconf(&inp);
        assert_eq!(
            p.writes,
            s(&[
                ("/i/gtk-theme", "'B'"),
                ("/i/icon-theme", "'Bi'"),
                ("/i/color-scheme", "'prefer-dark'"), // назад до baseline
                ("/tp/two-finger", "true"),           // назад до baseline
                ("/p/palette", "'campbell'"),
            ])
        );
        assert_eq!(p.resets, vec!["/m/edge-tiling"]); // у baseline немає → скинути
    }

    #[test]
    fn partial_apply_touches_only_selected_module() {
        let (baseline, t) = fixture();
        let tk = theme_keys(&t);
        let prev = BTreeMap::new();
        // зараз активна B для всього; користувач змінив шпалери вручну (некерований ключ)
        let mut cur = baseline.clone();
        cur.extend(s(&[
            ("/i/gtk-theme", "'B'"),
            ("/i/icon-theme", "'Bi'"),
            ("/p/palette", "'campbell'"),
            ("/bg/uri", "'mine'"),
        ]));
        // поведінку беремо з A, решта лишається з B
        let sel = [
            (Module::Look, "b"),
            (Module::Behavior, "a"),
            (Module::Sounds, "b"),
            (Module::Apps, "b"),
        ];
        let tg = targets(&t, &sel);
        let inp = DconfInput {
            baseline: &baseline,
            current: &cur,
            prev_keys: &prev,
            theme_keys: &tk,
            targets: &tg,
            affected: &[Module::Behavior],
        };
        let p = compute_dconf(&inp);
        assert_eq!(
            p.writes,
            s(&[("/m/edge-tiling", "false"), ("/tp/two-finger", "false")])
        );
        assert!(p.resets.is_empty());
        assert!(!p.desired.contains_key("/i/gtk-theme"));

        // тепер поведінку з fedora (нічого не задає) → ключі поведінки до baseline
        let mut cur2 = cur.clone();
        cur2.extend(p.writes);
        let sel = [
            (Module::Look, "b"),
            (Module::Behavior, "fedora"),
            (Module::Sounds, "b"),
            (Module::Apps, "b"),
        ];
        let tg = targets(&t, &sel);
        let inp = DconfInput {
            baseline: &baseline,
            current: &cur2,
            prev_keys: &prev,
            theme_keys: &tk,
            targets: &tg,
            affected: &[Module::Behavior],
        };
        let p = compute_dconf(&inp);
        assert_eq!(p.writes, s(&[("/tp/two-finger", "true")]));
        assert_eq!(p.resets, vec!["/m/edge-tiling"]);
        assert_eq!(p.to_baseline, vec!["/m/edge-tiling", "/tp/two-finger"]);
    }

    #[test]
    fn overlap_between_modules_later_module_wins() {
        // ключ задано і у «вигляді» A, і в «поведінці» B: при застосуванні лише вигляду
        // значення поведінки не повинно затертися baseline-ом
        let baseline = s(&[("/k", "'base'")]);
        let tk: BTreeMap<Module, BTreeSet<String>> = [
            (Module::Look, ["/k".to_string()].into()),
            (Module::Behavior, ["/k".to_string()].into()),
        ]
        .into();
        let tg: BTreeMap<Module, Settings> = [
            (Module::Look, Settings::new()),
            (Module::Behavior, s(&[("/k", "'beh'")])),
        ]
        .into();
        let cur = s(&[("/k", "'beh'")]);
        let prev = BTreeMap::new();
        let inp = DconfInput {
            baseline: &baseline,
            current: &cur,
            prev_keys: &prev,
            theme_keys: &tk,
            targets: &tg,
            affected: &[Module::Look],
        };
        let p = compute_dconf(&inp);
        assert!(p.writes.is_empty() && p.resets.is_empty(), "{p:?}");
    }

    #[test]
    fn previously_applied_keys_of_deleted_theme_are_reset() {
        let baseline = Settings::new();
        let tk = BTreeMap::new();
        let prev: BTreeMap<Module, Vec<String>> =
            [(Module::Sounds, vec!["/s/theme-name".to_string()])].into();
        let tg = BTreeMap::new();
        let cur = s(&[("/s/theme-name", "'gone'")]);
        let inp = DconfInput {
            baseline: &baseline,
            current: &cur,
            prev_keys: &prev,
            theme_keys: &tk,
            targets: &tg,
            affected: &[Module::Sounds],
        };
        assert_eq!(compute_dconf(&inp).resets, vec!["/s/theme-name"]);
    }

    #[test]
    fn extensions_keep_user_and_permanent() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let base_en = v(&["background-logo@fedorahosted.org"]);
        let cur_en = v(&[
            "background-logo@fedorahosted.org",
            "dash-to-dock@x",
            "my-own@me",
        ]);
        let cur_dis = v(&["dash-to-panel@x", "something@else"]);
        let theme_exts: BTreeSet<String> =
            v(&["dash-to-dock@x", "dash-to-panel@x", "arcmenu@x", "broken@x"])
                .into_iter()
                .collect();
        let target = v(&["dash-to-panel@x", "arcmenu@x", "broken@x"]);
        let broken: BTreeSet<String> = v(&["broken@x"]).into_iter().collect();
        let (en, dis) = compute_extensions(&ExtInput {
            base_en: &base_en,
            cur_en: &cur_en,
            cur_dis: &cur_dis,
            theme_exts: &theme_exts,
            target: &target,
            permanent: &["desk-bridge@desktop-themes-rs"],
            broken: &broken,
        });
        assert_eq!(
            en,
            v(&[
                "background-logo@fedorahosted.org",
                "my-own@me",
                "dash-to-panel@x",
                "arcmenu@x",
                "desk-bridge@desktop-themes-rs"
            ])
        );
        assert_eq!(dis, v(&["something@else"]));
    }
}
