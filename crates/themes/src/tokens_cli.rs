// SPDX-License-Identifier: GPL-3.0-or-later
//! `themes tokens ...`: check, generate (with timing) and "theme from wallpaper".

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Subcommand;
use desk_settings::{n_, tr};
use serde_json::json;
use themes_core::theme::{Theme, all_themes, get_theme, write_conf};
use themes_core::tokens::theme_tokens;
use themes_core::{Ctx, Error, tokens, util};

// Тексти довідки (ідентифікатори gettext; cli.rs перекладає їх під час запуску).
const T01: &str =
    n_("Check tokens.toml (all themes or the given ones): syntax, ranges, WCAG AA contrast");
const T02: &str = n_(
    "Generate the files from the tokens into the cache (~/.cache/desktop-themes/<id>) and show the time",
);
const T03: &str = n_("Regenerate even if the cache is up to date");
const T04: &str = n_("Measure N forced generations (min/median/max)");
const T05: &str =
    n_("dconf keys derived from the tokens (`dconf load /` format; without Ptyxis profile keys)");
const T06: &str = n_(
    "Create a theme from the palette of an image (wallpaper): themes/<id>/tokens.toml + theme.conf",
);
const T07: &str = n_("Id of the new theme");
const T08: &str = n_("Name of the theme (NAME in theme.conf)");
const T09: &str = n_("Overwrite an existing theme");
const T10: &str = n_("Write into this folder instead of the themes root");
const T11: &str = n_("Do not use the image as the theme's wallpaper (generate a gradient)");

#[derive(Subcommand)]
pub enum TokensCmd {
    #[command(about = T01)]
    Check { ids: Vec<String> },
    #[command(about = T02)]
    Generate {
        id: String,
        #[arg(long, help = T03)]
        force: bool,
        #[arg(long, value_name = "N", help = T04)]
        repeat: Option<u32>,
    },
    #[command(about = T05)]
    Keys { id: String },
    #[command(about = T06)]
    FromImage {
        path: PathBuf,
        #[arg(long, help = T07)]
        name: String,
        #[arg(long, help = T08)]
        title: Option<String>,
        #[arg(long, help = T09)]
        force: bool,
        #[arg(long, value_name = "DIR", help = T10)]
        out: Option<PathBuf>,
        #[arg(long, help = T11)]
        no_wallpaper: bool,
    },
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

pub fn exec(ctx: &Ctx, cmd: TokensCmd, json: bool) -> Result<i32, Error> {
    match cmd {
        TokensCmd::Check { ids } => check(ctx, &ids, json),
        TokensCmd::Keys { id } => keys(ctx, &id, json),
        TokensCmd::Generate { id, force, repeat } => generate(ctx, &id, force, repeat, json),
        TokensCmd::FromImage {
            path,
            name,
            title,
            force,
            out,
            no_wallpaper,
        } => from_image(ctx, &path, &name, title, force, out, no_wallpaper, json),
    }
}

fn keys(ctx: &Ctx, id: &str, json: bool) -> Result<i32, Error> {
    let t = get_theme(ctx, id)?;
    tokens::check(&t)?;
    let shell = t.handmade_shell_theme();
    let mut all = themes_core::dconf::Settings::new();
    for m in themes_core::Module::ALL {
        all.extend(tokens::keys_for(&t, m, shell));
    }
    all.retain(|k, _| !k.contains("@DEFAULT_PTYXIS_PROFILE@"));
    if json {
        println!("{}", serde_json::to_string_pretty(&all).unwrap_or_default());
    } else {
        print!("{}", themes_core::dconf::to_text(&all));
    }
    Ok(0)
}

fn check(ctx: &Ctx, ids: &[String], json: bool) -> Result<i32, Error> {
    let themes: Vec<Theme> = if ids.is_empty() {
        all_themes(ctx)
            .into_iter()
            .filter(tokens::has_tokens)
            .collect()
    } else {
        ids.iter()
            .map(|i| get_theme(ctx, i))
            .collect::<Result<_, _>>()?
    };
    let mut bad = 0;
    let mut out = Vec::new();
    for t in &themes {
        let (ok, errors, warnings) = match tokens::load(t) {
            None => (true, vec![], vec![tr!("no tokens.toml")]),
            Some(Ok(l)) => (
                true,
                vec![],
                l.warnings.iter().map(|i| i.to_string()).collect(),
            ),
            Some(Err(e)) => (
                false,
                e.errors.iter().map(|i| i.to_string()).collect(),
                e.warnings.iter().map(|i| i.to_string()).collect(),
            ),
        };
        if !ok {
            bad += 1;
        }
        if json {
            out.push(json!({"theme": t.id, "ok": ok, "errors": errors, "warnings": warnings}));
        } else {
            println!("{} {}", if ok { "OK  " } else { "FAIL" }, t.id);
            for e in &errors {
                println!("  {}", tr!("error: {text}", text = e));
            }
            for w in &warnings {
                println!("  {}", tr!("warning: {text}", text = w));
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    }
    Ok(if bad > 0 { 1 } else { 0 })
}

fn generate(
    ctx: &Ctx,
    id: &str,
    force: bool,
    repeat: Option<u32>,
    json: bool,
) -> Result<i32, Error> {
    let t = get_theme(ctx, id)?;
    if !tokens::has_tokens(&t) {
        return Err(Error(tr!(
            "the theme “{theme}” has no tokens.toml",
            theme = id
        )));
    }
    if let Some(n) = repeat {
        // end-to-end per run: read files, parse, merge, validate, render, write
        let n = n.max(1);
        let mut v: Vec<f64> = Vec::new();
        for _ in 0..n {
            let t0 = Instant::now();
            let th = Theme::new(ctx, id);
            tokens::materialize(&th, true)?;
            v.push(ms(t0.elapsed()));
        }
        let t0 = Instant::now();
        let th = Theme::new(ctx, id);
        tokens::materialize(&th, false)?;
        let cached = ms(t0.elapsed());
        v.sort_by(f64::total_cmp);
        let (min, med, max) = (v[0], v[v.len() / 2], v[v.len() - 1]);
        if json {
            println!(
                "{}",
                json!({"theme": id, "runs": n, "min_ms": min, "median_ms": med, "max_ms": max, "cached_ms": cached})
            );
        } else {
            println!(
                "{id}: {}",
                tr!(
                    "{runs} generations: min {min} ms, median {median} ms, max {max} ms; from the cache {cached} ms",
                    runs = n,
                    min = format!("{min:.2}"),
                    median = format!("{med:.2}"),
                    max = format!("{max:.2}"),
                    cached = format!("{cached:.2}")
                )
            );
        }
        return Ok(0);
    }
    let t0 = Instant::now();
    let (files, _) = tokens::materialize(&t, force)?;
    let took = ms(t0.elapsed());
    if json {
        println!("{}", json!({"theme": id, "ms": took, "files": files}));
    } else {
        for f in &files {
            println!("  {} ({})", f.path, f.status);
        }
        println!(
            "{id}: {}",
            tr!(
                "files: {count}, {ms} ms",
                count = files.len(),
                ms = format!("{took:.2}")
            )
        );
    }
    Ok(0)
}

/// Packed RGB pixels of the image, scaled down to at most 256 px on the long side
/// (gdk-pixbuf: every format with an installed loader, including JPEG XL and WebP).
fn read_rgb(path: &Path) -> Result<Vec<u8>, Error> {
    let pb = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, 256, 256, true).map_err(|e| {
        Error(tr!(
            "could not read the image {path}: {error}",
            path = path.display(),
            error = e
        ))
    })?;
    // pixbuf sizes are never negative; a bad value becomes 0 and the loops below do nothing
    let us = |v: i32| usize::try_from(v).unwrap_or(0);
    let (w, h) = (us(pb.width()), us(pb.height()));
    let n = us(pb.n_channels());
    let stride = us(pb.rowstride());
    let bytes = pb.read_pixel_bytes();
    let data: &[u8] = &bytes;
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let i = y * stride + x * n;
            if i + 2 >= data.len() {
                continue;
            }
            // fully transparent pixels say nothing about the palette
            if n == 4 && data[i + 3] < 16 {
                continue;
            }
            rgb.extend_from_slice(&data[i..i + 3]);
        }
    }
    Ok(rgb)
}

#[allow(clippy::too_many_arguments)]
fn from_image(
    ctx: &Ctx,
    path: &Path,
    id: &str,
    title: Option<String>,
    force: bool,
    out: Option<PathBuf>,
    no_wallpaper: bool,
    json: bool,
) -> Result<i32, Error> {
    if !util::valid_id(id) {
        return Err(Error(tr!("invalid theme id: {id}", id = format!("{id:?}"))));
    }
    let dir = out.unwrap_or_else(|| ctx.themes.join(id));
    if dir.join("tokens.toml").exists() && !force {
        return Err(Error(tr!(
            "{path} already exists (--force to overwrite it)",
            path = dir.display()
        )));
    }
    let t0 = Instant::now();
    let rgb = read_rgb(path)?;
    if rgb.is_empty() {
        return Err(Error(tr!("the image has no opaque pixels")));
    }
    let (clusters, pal) = theme_tokens::palette::from_pixels(&rgb);
    let name = title.unwrap_or_else(|| id.to_string());
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let home = ctx.home.to_string_lossy();
    let image = abs.to_string_lossy().into_owned();
    let image = match image.strip_prefix(&format!("{home}/")) {
        Some(rest) => format!("~/{rest}"),
        None => image,
    };
    let text =
        theme_tokens::palette::tokens_toml(&pal, &name, (!no_wallpaper).then_some(image.as_str()));
    // the result must pass the same validation as any theme
    theme_tokens::load_str("tokens.toml", &text).map_err(|e| Error(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("tokens.toml"), &text)?;
    write_conf(
        &dir.join("theme.conf"),
        &[
            ("NAME", name.clone()),
            (
                "DESCRIPTION",
                tr!(
                    "Theme from the palette of the image {file}",
                    file = abs
                        .file_name()
                        .map_or_default(|f| f.to_string_lossy().into_owned())
                ),
            ),
        ],
    )?;
    let took = ms(t0.elapsed());
    if json {
        println!(
            "{}",
            json!({
                "theme": id, "dir": dir, "seed": pal.seed.hex(), "ms": took,
                "clusters": clusters.iter().map(|c| json!({"color": c.color.hex(), "share": c.share})).collect::<Vec<_>>(),
            })
        );
    } else {
        println!(
            "{}",
            tr!(
                "Theme “{name}” ({id}) created: {path}",
                name = name,
                id = id,
                path = dir.display()
            )
        );
        println!(
            "  {}",
            tr!("seed colour: {colour}", colour = pal.seed.hex())
        );
        let cs: Vec<String> = clusters
            .iter()
            .map(|c| format!("{} {:.0}%", c.color.hex(), c.share * 100.0))
            .collect();
        println!("  {}", tr!("clusters: {list}", list = cs.join(", ")));
        println!(
            "  {}",
            tr!(
                "{ms} ms. To apply: themes apply {id} --only look,apps",
                ms = format!("{took:.1}"),
                id = id
            )
        );
    }
    Ok(0)
}
