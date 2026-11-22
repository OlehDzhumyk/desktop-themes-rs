// SPDX-License-Identifier: GPL-3.0-or-later
//! Рушій історії буфера обміну: записи, закріплення, пошук, видалення, приватний режим,
//! обмеження розміру, збереження на диск і евристики чутливого вмісту.
//!
//! Чиста логіка без D-Bus і GSettings (їх під'єднує `service.rs`), тож усе покрито тестами.
//! Захоплення буфера в GNOME Wayland робить оболонка (desk-bridge), рушій лише отримує вміст.

use desk_settings::tr;
use std::io::Write;
use std::path::{Path, PathBuf};

use desk_proto::{ClipItem, kind};
use serde::{Deserialize, Serialize};

/// Найбільший текстовий запис, який зберігається (довший текст обрізати не можна — пропускаємо).
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// Типи, якими менеджери паролів позначають секрети (KDE/KeePassXC, macOS-сумісні).
pub const SECRET_HINTS: [&str; 2] = [
    "x-kde-passwordManagerHint",
    "application/x-nspasteboard-concealed-type",
];
/// 2: `hash` is FNV-1a (1: `DefaultHasher`, recomputed on load).
const STORE_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub history_size: usize,
    pub images: bool,
    pub image_max_bytes: usize,
    pub persist: bool,
    pub persist_images: bool,
    pub private: bool,
    pub excluded_apps: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            history_size: 50,
            images: true,
            image_max_bytes: 8192 * 1024,
            persist: true,
            persist_images: false,
            private: false,
            excluded_apps: vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub kind: String,
    pub mime: String,
    /// Повний текст (для зображень — опис).
    pub text: String,
    /// Ім'я файлу зображення в теці зображень (порожньо для тексту).
    #[serde(default)]
    pub image: String,
    pub pinned: bool,
    pub time: i64,
    pub size: u64,
    pub hash: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Skip {
    Disabled,
    Private,
    Sensitive,
    ExcludedApp,
    Empty,
    TooLarge,
    ImagesOff,
    Unsupported,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Pushed {
    Added(u64),
    /// Такий самий вміст уже був — запис піднято нагору.
    Bumped(u64),
    Skipped(Skip),
}

#[derive(Serialize, Deserialize)]
struct Store {
    version: u32,
    next_id: u64,
    items: Vec<Entry>,
}

pub struct Engine {
    pub cfg: Config,
    /// Найновіші спершу.
    items: Vec<Entry>,
    next_id: u64,
    /// ~/.local/share/deskd/clipboard
    data_dir: PathBuf,
    /// $XDG_RUNTIME_DIR/deskd/clipboard (зображення, які не зберігаються між сесіями)
    runtime_dir: PathBuf,
}

pub fn is_text_mime(m: &str) -> bool {
    m.starts_with("text/plain") || m == "UTF8_STRING" || m == "STRING" || m == "TEXT"
}

/// Content hash for de-duplication; stored in history.json, so it must be stable
/// (FNV-1a 64 over the kind and the bytes; version 1 of the store used `DefaultHasher`).
fn hash_of(mime_kind: &str, data: &[u8]) -> u64 {
    desk_settings::stable_hash::fnv1a64(&[mime_kind.as_bytes(), data])
}

fn image_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => "img",
    }
}

/// Розміри PNG/GIF із заголовка (без декодування).
pub fn image_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") && &data[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(data[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(data[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if data.len() >= 10 && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes([data[6], data[7]]) as u32;
        let h = u16::from_le_bytes([data[8], data[9]]) as u32;
        return Some((w, h));
    }
    None
}

fn image_label(mime: &str, data: &[u8]) -> String {
    let fmt = image_ext(mime).to_uppercase();
    match image_size(data) {
        // TRANSLATORS: description of a copied image in the clipboard history, e.g. "PNG image 640×480"
        Some((w, h)) => tr!(
            "{format} image {width}×{height}",
            format = fmt,
            width = w,
            height = h
        ),
        None => tr!("{format} image", format = fmt),
    }
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

fn ensure_private_dir(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !p.is_dir() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(p)?;
    }
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
}

impl Engine {
    pub fn new(cfg: Config, data_dir: PathBuf, runtime_dir: PathBuf) -> Engine {
        Engine {
            cfg,
            items: vec![],
            next_id: 1,
            data_dir,
            runtime_dir,
        }
    }

    /// Типові теки: `$XDG_DATA_HOME/deskd/clipboard` і `$XDG_RUNTIME_DIR/deskd/clipboard`
    /// (або `$DESKD_RUNTIME_DIR/clipboard`).
    pub fn default_dirs() -> (PathBuf, PathBuf) {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/tmp".into());
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        // DESKD_RUNTIME_DIR: тести (tools/headless) тримають тимчасові файли окремо
        let rt = std::env::var_os("DESKD_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("XDG_RUNTIME_DIR")
                    .map(|p| PathBuf::from(p).join("deskd"))
                    .filter(|p| p.is_absolute())
            })
            .unwrap_or_else(|| std::env::temp_dir().join("deskd"));
        (data.join("deskd/clipboard"), rt.join("clipboard"))
    }

    fn store_path(&self) -> PathBuf {
        self.data_dir.join("history.json")
    }

    fn images_dir(&self, persistent: bool) -> PathBuf {
        if persistent {
            self.data_dir.join("images")
        } else {
            self.runtime_dir.join("images")
        }
    }

    pub fn image_path(&self, e: &Entry) -> Option<PathBuf> {
        if e.image.is_empty() {
            return None;
        }
        [true, false]
            .into_iter()
            .map(|p| self.images_dir(p).join(&e.image))
            .find(|p| p.is_file())
    }

    /// Migration from store version 1: recompute every `hash` with the stable function, so that
    /// copying the same content again still finds the old entry. Image files keep their names.
    fn rehash(&mut self) {
        let hashes: Vec<u64> = self
            .items
            .iter()
            .map(|e| {
                let data = if e.kind == kind::TEXT {
                    Some(e.text.clone().into_bytes())
                } else {
                    self.image_path(e).and_then(|p| std::fs::read(p).ok())
                };
                data.map_or(e.hash, |d| hash_of(&e.kind, &d))
            })
            .collect();
        for (e, h) in self.items.iter_mut().zip(hashes) {
            e.hash = h;
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn check(&self, mime: &str, data: &[u8], offered: &[String], app: &str) -> Option<Skip> {
        if !self.cfg.enabled {
            return Some(Skip::Disabled);
        }
        if self.cfg.private {
            return Some(Skip::Private);
        }
        if offered
            .iter()
            .chain(std::iter::once(&mime.to_string()))
            .any(|m| SECRET_HINTS.iter().any(|h| m.eq_ignore_ascii_case(h)))
        {
            return Some(Skip::Sensitive);
        }
        let app = app.trim();
        if !app.is_empty()
            && self
                .cfg
                .excluded_apps
                .iter()
                .any(|x| x.eq_ignore_ascii_case(app))
        {
            return Some(Skip::ExcludedApp);
        }
        if data.is_empty() {
            return Some(Skip::Empty);
        }
        if is_text_mime(mime) {
            if data.len() > MAX_TEXT_BYTES {
                return Some(Skip::TooLarge);
            }
            if String::from_utf8_lossy(data).trim().is_empty() {
                return Some(Skip::Empty);
            }
        } else if mime.starts_with("image/") {
            if !self.cfg.images {
                return Some(Skip::ImagesOff);
            }
            if data.len() > self.cfg.image_max_bytes {
                return Some(Skip::TooLarge);
            }
        } else {
            return Some(Skip::Unsupported);
        }
        None
    }

    /// Новий вміст буфера обміну.
    pub fn push(
        &mut self,
        mime: &str,
        data: &[u8],
        offered: &[String],
        source_app: &str,
        now: i64,
    ) -> Pushed {
        if let Some(s) = self.check(mime, data, offered, source_app) {
            return Pushed::Skipped(s);
        }
        let is_text = is_text_mime(mime);
        let k = if is_text { kind::TEXT } else { kind::IMAGE };
        let hash = hash_of(k, data);
        if let Some(pos) = self
            .items
            .iter()
            .position(|e| e.hash == hash && e.kind == k)
        {
            let mut e = self.items.remove(pos);
            e.time = now;
            let id = e.id;
            self.items.insert(0, e);
            return Pushed::Bumped(id);
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut e = Entry {
            id,
            kind: k.to_string(),
            mime: if is_text {
                "text/plain;charset=utf-8".to_string()
            } else {
                mime.to_string()
            },
            text: String::new(),
            image: String::new(),
            pinned: false,
            time: now,
            size: data.len() as u64,
            hash,
        };
        if is_text {
            e.text = String::from_utf8_lossy(data).into_owned();
        } else {
            let name = format!("{hash:016x}.{}", image_ext(mime));
            let dir = self.images_dir(self.cfg.persist && self.cfg.persist_images);
            if ensure_private_dir(&dir)
                .and_then(|_| write_private(&dir.join(&name), data))
                .is_err()
            {
                return Pushed::Skipped(Skip::Unsupported);
            }
            e.image = name;
            e.text = image_label(mime, data);
        }
        self.items.insert(0, e);
        self.trim();
        Pushed::Added(id)
    }

    fn remove_image_file(&self, e: &Entry) {
        if let Some(p) = self.image_path(e) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Прибрати найстаріші незакріплені записи понад `history_size`.
    fn trim(&mut self) {
        let mut unpinned = 0usize;
        let mut drop = vec![];
        for (i, e) in self.items.iter().enumerate() {
            if !e.pinned {
                unpinned += 1;
                if unpinned > self.cfg.history_size {
                    drop.push(i);
                }
            }
        }
        for i in drop.into_iter().rev() {
            let e = self.items.remove(i);
            self.remove_image_file(&e);
        }
    }

    pub fn find(&self, id: u64) -> Option<&Entry> {
        self.items.iter().find(|e| e.id == id)
    }

    pub fn item(&self, e: &Entry) -> ClipItem {
        ClipItem {
            id: e.id,
            kind: e.kind.clone(),
            text: e.text.clone(),
            mime: e.mime.clone(),
            image: self
                .image_path(e)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            pinned: e.pinned,
            time: e.time,
            size: e.size,
        }
    }

    /// Записи (найновіші спершу), що містять `query` (без урахування регістру).
    pub fn list(&self, query: &str, limit: u32) -> Vec<ClipItem> {
        let q = query.trim().to_lowercase();
        let it = self
            .items
            .iter()
            .filter(|e| q.is_empty() || e.text.to_lowercase().contains(&q));
        let v: Vec<ClipItem> = if limit == 0 {
            it.map(|e| self.item(e)).collect()
        } else {
            it.take(limit as usize).map(|e| self.item(e)).collect()
        };
        v
    }

    pub fn get(&self, id: u64) -> Option<(String, Vec<u8>)> {
        let e = self.find(id)?;
        if e.kind == kind::TEXT {
            return Some((e.mime.clone(), e.text.clone().into_bytes()));
        }
        let data = std::fs::read(self.image_path(e)?).ok()?;
        Some((e.mime.clone(), data))
    }

    /// Вміст запису; запис стає найновішим.
    pub fn activate(&mut self, id: u64, now: i64) -> Option<(String, Vec<u8>)> {
        let out = self.get(id)?;
        let pos = self.items.iter().position(|e| e.id == id)?;
        let mut e = self.items.remove(pos);
        e.time = now;
        self.items.insert(0, e);
        Some(out)
    }

    pub fn pin(&mut self, id: u64, pinned: bool) -> bool {
        let Some(e) = self.items.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        let changed = e.pinned != pinned;
        e.pinned = pinned;
        if !pinned {
            self.trim();
        }
        changed
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let Some(pos) = self.items.iter().position(|e| e.id == id) else {
            return false;
        };
        let e = self.items.remove(pos);
        self.remove_image_file(&e);
        true
    }

    pub fn clear(&mut self, keep_pinned: bool) -> bool {
        let before = self.items.len();
        let (keep, drop): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|e| keep_pinned && e.pinned);
        for e in &drop {
            self.remove_image_file(e);
        }
        self.items = keep;
        before != self.items.len()
    }

    /// Нові налаштування: обрізати історію, перенести зображення між тимчасовою й постійною
    /// теками, прибрати збережений файл, якщо збереження вимкнули. Повертає, чи змінився список.
    pub fn set_config(&mut self, cfg: Config) -> bool {
        let before = self.items.len();
        let old_img_persist = self.cfg.persist && self.cfg.persist_images;
        self.cfg = cfg;
        let new_img_persist = self.cfg.persist && self.cfg.persist_images;
        if old_img_persist != new_img_persist {
            let to = self.images_dir(new_img_persist);
            if ensure_private_dir(&to).is_ok() {
                for e in &self.items {
                    if let Some(p) = self.image_path(e) {
                        let dest = to.join(&e.image);
                        if p != dest
                            && std::fs::rename(&p, &dest).is_err()
                            && std::fs::copy(&p, &dest).is_ok()
                        {
                            let _ = std::fs::remove_file(&p);
                        }
                    }
                }
            }
        }
        self.trim();
        if !self.cfg.persist {
            let _ = std::fs::remove_file(self.store_path());
        }
        before != self.items.len()
    }

    /// Записати історію на диск (лише якщо збереження ввімкнене; зображення — лише з
    /// `persist_images`).
    pub fn save(&self) -> std::io::Result<()> {
        if !self.cfg.persist {
            return Ok(());
        }
        ensure_private_dir(&self.data_dir)?;
        let items: Vec<Entry> = self
            .items
            .iter()
            .filter(|e| e.kind == kind::TEXT || self.cfg.persist_images)
            .cloned()
            .collect();
        let store = Store {
            version: STORE_VERSION,
            next_id: self.next_id,
            items,
        };
        let json = serde_json::to_vec(&store).map_err(std::io::Error::other)?;
        write_private(&self.store_path(), &json)
    }

    /// Прочитати збережену історію (якщо ввімкнено збереження). Записи зображень без файлу
    /// відкидаються; тимчасова тека зображень від попередньої сесії очищається.
    pub fn load(&mut self) {
        if let Ok(rd) = std::fs::read_dir(self.images_dir(false)) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        if !self.cfg.persist {
            return;
        }
        let Ok(bytes) = std::fs::read(self.store_path()) else {
            return;
        };
        let Ok(store) = serde_json::from_slice::<Store>(&bytes) else {
            eprintln!(
                "deskd: {} is damaged, ignoring it",
                self.store_path().display()
            );
            return;
        };
        self.next_id = store.next_id.max(1);
        self.items = store
            .items
            .into_iter()
            .filter(|e| e.kind == kind::TEXT || self.image_path(e).is_some())
            .collect();
        if store.version < 2 {
            self.rehash();
        }
        if let Some(m) = self.items.iter().map(|e| e.id).max() {
            self.next_id = self.next_id.max(m + 1);
        }
        // осиротілі файли зображень
        if let Ok(rd) = std::fs::read_dir(self.images_dir(true)) {
            for f in rd.flatten() {
                let n = f.file_name().to_string_lossy().into_owned();
                if !self.items.iter().any(|e| e.image == n) {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        self.trim();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deskd-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn engine(tag: &str, cfg: Config) -> (Engine, PathBuf) {
        let d = tmp(tag);
        (Engine::new(cfg, d.join("data"), d.join("run")), d)
    }

    const T: &str = "text/plain;charset=utf-8";

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend([8, 6, 0, 0, 0]);
        v
    }

    #[test]
    fn push_dedup_and_order() {
        let (mut e, d) = engine("order", Config::default());
        assert_eq!(e.push(T, b"one", &[], "", 1), Pushed::Added(1));
        assert_eq!(e.push(T, b"two", &[], "", 2), Pushed::Added(2));
        assert_eq!(e.push("UTF8_STRING", b"one", &[], "", 3), Pushed::Bumped(1));
        let l = e.list("", 0);
        assert_eq!(
            l.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(),
            ["one", "two"]
        );
        assert_eq!(l[0].time, 3);
        assert_eq!(l[0].mime, T);
        assert_eq!(
            e.push(T, b"   \n", &[], "", 4),
            Pushed::Skipped(Skip::Empty)
        );
        assert_eq!(
            e.push("text/html", b"<b>", &[], "", 4),
            Pushed::Skipped(Skip::Unsupported)
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn sensitive_private_excluded() {
        let cfg = Config {
            excluded_apps: vec!["org.keepassxc.KeePassXC".into()],
            ..Config::default()
        };
        let (mut e, d) = engine("sens", cfg);
        let hint = vec![T.to_string(), "x-kde-passwordManagerHint".to_string()];
        assert_eq!(
            e.push(T, b"secret", &hint, "", 1),
            Pushed::Skipped(Skip::Sensitive)
        );
        assert_eq!(
            e.push(T, b"secret", &[], "org.keepassxc.keepassxc", 1),
            Pushed::Skipped(Skip::ExcludedApp)
        );
        e.cfg.private = true;
        assert_eq!(e.push(T, b"x", &[], "", 1), Pushed::Skipped(Skip::Private));
        e.cfg.private = false;
        e.cfg.enabled = false;
        assert_eq!(e.push(T, b"x", &[], "", 1), Pushed::Skipped(Skip::Disabled));
        assert!(e.is_empty());
        let big = vec![b'a'; MAX_TEXT_BYTES + 1];
        e.cfg.enabled = true;
        assert_eq!(e.push(T, &big, &[], "", 1), Pushed::Skipped(Skip::TooLarge));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn history_size_and_pins() {
        let cfg = Config {
            history_size: 3,
            ..Config::default()
        };
        let (mut e, d) = engine("size", cfg);
        for i in 0..3 {
            e.push(T, format!("t{i}").as_bytes(), &[], "", i);
        }
        assert!(e.pin(1, true));
        for i in 3..6 {
            e.push(T, format!("t{i}").as_bytes(), &[], "", i);
        }
        let texts: Vec<String> = e.list("", 0).into_iter().map(|i| i.text).collect();
        assert_eq!(
            texts,
            ["t5", "t4", "t3", "t0"],
            "pinned t0 survives trimming"
        );
        assert!(e.clear(true));
        assert_eq!(e.len(), 1);
        assert!(e.pin(1, false));
        assert!(e.clear(false));
        assert!(e.is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn search_get_activate_delete() {
        let (mut e, d) = engine("search", Config::default());
        e.push(T, "Привіт, Світ".as_bytes(), &[], "", 1);
        e.push(T, b"hello", &[], "", 2);
        e.push(T, b"other", &[], "", 3);
        let r = e.list("СВІТ", 0);
        assert_eq!(r.len(), 1);
        assert_eq!(e.list("", 2).len(), 2);
        assert_eq!(e.get(2).unwrap(), (T.to_string(), b"hello".to_vec()));
        e.activate(1, 9).unwrap();
        assert_eq!(e.list("", 1)[0].id, 1);
        assert!(e.delete(2));
        assert!(!e.delete(2));
        assert!(e.get(2).is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn images_runtime_vs_persistent() {
        let cfg = Config {
            image_max_bytes: 1000,
            ..Config::default()
        };
        let (mut e, d) = engine("img", cfg);
        let img = png(640, 480);
        let Pushed::Added(id) = e.push("image/png", &img, &[], "", 1) else {
            panic!()
        };
        let it = e.list("", 0).remove(0);
        assert_eq!(it.kind, "image");
        assert_eq!(it.text, "PNG image 640×480");
        assert!(it.image.starts_with(d.join("run").to_str().unwrap()));
        assert_eq!(e.get(id).unwrap().1, img);
        assert_eq!(
            e.push("image/png", &vec![0u8; 2000], &[], "", 2),
            Pushed::Skipped(Skip::TooLarge)
        );
        // збереження зображень увімкнули → файл переїжджає в постійну теку
        let mut c = e.cfg.clone();
        c.persist_images = true;
        e.set_config(c);
        let it = e.list("", 0).remove(0);
        assert!(
            it.image.starts_with(d.join("data").to_str().unwrap()),
            "{}",
            it.image
        );
        e.save().unwrap();
        let mut e2 = Engine::new(e.cfg.clone(), d.join("data"), d.join("run"));
        e2.load();
        assert_eq!(e2.get(id).unwrap().1, img);
        assert!(e2.delete(id));
        assert!(!Path::new(&it.image).exists());
        e2.cfg.images = false;
        assert_eq!(
            e2.push("image/png", &img, &[], "", 3),
            Pushed::Skipped(Skip::ImagesOff)
        );
        let _ = std::fs::remove_dir_all(d);
    }

    /// A history written before the stable hash (store version 1, hashes from `DefaultHasher`)
    /// still de-duplicates after loading: the hashes are recomputed.
    #[test]
    fn version_1_store_is_migrated() {
        let (e, d) = engine("migrate", Config::default());
        let data = d.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let old = r#"{"version":1,"next_id":2,"items":[{"id":1,"kind":"text","mime":"text/plain;charset=utf-8","text":"old entry","pinned":false,"time":1,"size":9,"hash":12345}]}"#;
        std::fs::write(data.join("history.json"), old).unwrap();
        let mut e2 = Engine::new(e.cfg.clone(), data, d.join("run"));
        e2.load();
        assert_eq!(e2.len(), 1);
        assert_eq!(e2.push(T, b"old entry", &[], "", 5), Pushed::Bumped(1));
        assert_eq!(e2.len(), 1);
        e2.save().unwrap();
        let saved = std::fs::read_to_string(d.join("data/history.json")).unwrap();
        assert!(saved.contains(&format!("\"version\":{STORE_VERSION}")));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn persistence_roundtrip_and_privacy() {
        let (mut e, d) = engine("persist", Config::default());
        e.push(T, b"keep me", &[], "", 1);
        e.push("image/png", &png(1, 1), &[], "", 2);
        e.pin(1, true);
        e.save().unwrap();
        let path = d.join("data/history.json");
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let dmode = std::fs::metadata(d.join("data"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dmode, 0o700);
        }
        let mut e2 = Engine::new(Config::default(), d.join("data"), d.join("run"));
        e2.load();
        // зображення не збережено (persist_images=false), а тимчасові файли попередньої сесії прибрано
        assert_eq!(e2.len(), 1);
        assert!(e2.find(1).unwrap().pinned);
        assert_eq!(e2.push(T, b"new", &[], "", 5), Pushed::Added(3));
        // збереження вимкнули → файл історії видаляється
        let mut c = e2.cfg.clone();
        c.persist = false;
        e2.set_config(c);
        assert!(!path.exists());
        e2.save().unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn png_gif_sizes() {
        assert_eq!(image_size(&png(3, 7)), Some((3, 7)));
        assert_eq!(image_size(b"GIF89a\x02\x00\x05\x00"), Some((2, 5)));
        assert_eq!(image_size(b"nope"), None);
    }
}
