// SPDX-License-Identifier: GPL-3.0-or-later
//! Timing of token generation (budget: < 50 ms per theme at apply time). Not run by default:
//! `cargo test --release -p theme-tokens --test perf -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::time::Instant;

use theme_tokens::generate::Target;
use theme_tokens::{load_chain, materialize};

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

#[test]
#[ignore]
fn generation_time() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tmp = std::env::temp_dir().join(format!("theme-tokens-perf-{}", std::process::id()));
    for id in ["lavender", "macos", "windows11"] {
        let text =
            std::fs::read_to_string(repo.join("themes").join(id).join("tokens.toml")).unwrap();
        let tg = Target {
            theme_id: id.into(),
            theme_name: id.into(),
            home: "/home/user".into(),
            cache_dir: PathBuf::from(&tmp).join(id),
            handmade_gtk4: false,
            handmade_shell_theme: false,
        };
        let (mut parse, mut cold, mut warm) = (vec![], vec![], vec![]);
        for _ in 0..200 {
            let t0 = Instant::now();
            let l = load_chain(&[(id.into(), text.clone())]).unwrap();
            parse.push(t0.elapsed().as_secs_f64() * 1e3);
            let t0 = Instant::now();
            materialize(&l, &tg, true).unwrap();
            cold.push(t0.elapsed().as_secs_f64() * 1e3);
            let t0 = Instant::now();
            assert!(materialize(&l, &tg, false).unwrap().cached);
            warm.push(t0.elapsed().as_secs_f64() * 1e3);
        }
        let (p, c, w) = (median(parse), median(cold), median(warm));
        println!("{id}: parse+validate {p:.3} ms, render+write {c:.3} ms, cache check {w:.3} ms");
        assert!(p + c < 50.0, "{id} over budget");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
