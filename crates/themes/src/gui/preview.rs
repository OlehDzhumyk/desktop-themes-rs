// SPDX-License-Identifier: GPL-3.0-or-later
//! Мініатюри прев'ю (кеш у ~/.cache/desktop-themes/thumbs, обрізка до 16:9).

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Once;

use gtk::{gdk, gdk_pixbuf::Pixbuf, glib};

fn thumb_dir() -> PathBuf {
    glib::user_cache_dir().join("desktop-themes/thumbs")
}

/// File names of the stable-hash scheme start with this; older thumbnails (named by
/// `DefaultHasher`, which is not guaranteed stable) are orphaned and removed once per run.
const THUMB_PREFIX: &str = "v5-";

fn purge_old_thumbs() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Ok(rd) = std::fs::read_dir(thumb_dir()) else {
            return;
        };
        for f in rd.flatten() {
            let name = f.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".png") && !name.starts_with(THUMB_PREFIX) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    });
}

/// Готує мініатюру у фоні; повертає шлях до PNG.
fn make_thumb(path: &Path, width: i32) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    purge_old_thumbs();
    let key = desk_settings::stable_hash::fnv1a64(&[
        path.as_os_str().as_bytes(),
        mtime.to_string().as_bytes(),
        width.to_string().as_bytes(),
    ]);
    let thumb = thumb_dir().join(format!("{THUMB_PREFIX}{key:016x}.png"));
    if thumb.is_file() {
        return Some(thumb);
    }
    let pb = Pixbuf::from_file_at_scale(path, width * 2, -1, true).ok()?;
    let (pw, ph) = (pb.width(), pb.height());
    let height = (width as f64 * 9.0 / 16.0).round() as i32;
    let (mut cw, mut ch) = (pw, (pw as f64 * 9.0 / 16.0).round() as i32);
    if ch > ph {
        ch = ph;
        cw = (ph as f64 * 16.0 / 9.0).round() as i32;
    }
    let sub = pb.new_subpixbuf((pw - cw) / 2, (ph - ch) / 2, cw, ch);
    let scaled = sub.scale_simple(width, height, gtk::gdk_pixbuf::InterpType::Bilinear)?;
    std::fs::create_dir_all(thumb_dir()).ok()?;
    scaled.savev(&thumb, "png", &[]).ok()?;
    Some(thumb)
}

pub async fn load(path: String, width: i32) -> Option<gdk::Texture> {
    if path.is_empty() || !Path::new(&path).exists() {
        return None;
    }
    let thumb = gtk::gio::spawn_blocking(move || make_thumb(Path::new(&path), width))
        .await
        .ok()
        .flatten()?;
    gdk::Texture::from_filename(thumb).ok()
}
