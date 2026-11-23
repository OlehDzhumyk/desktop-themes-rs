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
/// Types that mark a secret (or content that must not be recorded): any of them in the offered
/// list skips the copy. `x-kde-passwordManagerHint` is set by KeePassXC, KWalletManager and
/// Firefox's password manager on Linux; the `org.nspasteboard` names (and their old MIME-style
/// spelling) come from the nspasteboard.org convention of macOS tools, which cross-platform apps
/// reuse; `ExcludeClipboardContentFromMonitorProcessing` is the Windows clipboard-history opt-out
/// that Chromium-based apps set. Keep in sync with `SECRET_HINTS` in bridge/src/clipboard/watcher.ts.
pub const SECRET_HINTS: [&str; 5] = [
    "x-kde-passwordManagerHint",
    "application/x-nspasteboard-concealed-type",
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
    "ExcludeClipboardContentFromMonitorProcessing",
];
/// How long a text that looks like a secret ([`looks_secret`]) stays in the history (seconds).
/// It is never written to disk.
pub const SECRET_TTL: i64 = 120;
/// Image types that are recorded, with their file extension. Only raster formats whose bytes
/// start with a known signature ([`sniff`]): the shell renders the files, and SVG (or anything a
/// loader would parse as SVG) would run a vector renderer inside the shell process (S-9).
const IMAGE_TYPES: [(&str, &str); 5] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/jpg", "jpg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];
/// 2: `hash` is FNV-1a (1: `DefaultHasher`, recomputed on load).
const STORE_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub history_size: usize,
    pub images: bool,
    pub image_max_bytes: usize,
    /// Загальний бюджет байтів тимчасових зображень у `$XDG_RUNTIME_DIR` (tmpfs).
    pub image_budget_bytes: usize,
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
            image_budget_bytes: 256 * 1024 * 1024,
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
    /// A probable secret: removed at this unix time, never saved. Pinning clears it.
    #[serde(skip)]
    pub expires: Option<i64>,
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
    /// Our own paste coming back ([`desk_proto::INTERNAL_MIME`] offered).
    Internal,
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

fn image_ext(mime: &str) -> Option<&'static str> {
    IMAGE_TYPES
        .iter()
        .find(|(m, _)| m.eq_ignore_ascii_case(mime))
        .map(|(_, e)| *e)
}

/// The bytes are of the type the extension names (the shell's image loader goes by content).
fn sniff(ext: &str, data: &[u8]) -> bool {
    match ext {
        "png" => data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" => data.starts_with(&[0xff, 0xd8, 0xff]),
        "gif" => data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"),
        "webp" => data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP",
        _ => false,
    }
}

/// An image file name deskd itself made: 16 hex digits of the content hash and a known
/// extension. Names read from history.json are checked against it before they touch a path (S-8).
pub fn valid_image_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else {
        return false;
    };
    stem.len() == 16
        && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && IMAGE_TYPES.iter().any(|(_, e)| *e == ext)
}

/// Shannon entropy of the characters, in bits per character.
fn entropy(t: &str) -> f64 {
    let mut counts = std::collections::HashMap::<char, u32>::new();
    for c in t.chars() {
        *counts.entry(c).or_default() += 1;
    }
    let n = t.chars().count() as f64;
    counts
        .values()
        .map(|&k| {
            let p = f64::from(k) / n;
            -p * p.log2()
        })
        .sum()
}

fn is_uuid(t: &str) -> bool {
    let parts: Vec<&str> = t.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
        && parts
            .iter()
            .all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Text that looks like a password or a token copied without a hint: one short word (10..=64
/// characters, no whitespace) of letters and digits plus a third character class (another
/// letter case or symbols), with high entropy (>= 3 bits per character); URLs, paths, e-mail
/// addresses and UUIDs are not. Conservative on purpose: such text is still recorded, it only
/// stays in memory for [`SECRET_TTL`] and is never written to disk.
pub fn looks_secret(text: &str) -> bool {
    let t = text.trim();
    let n = t.chars().count();
    if !(10..=64).contains(&n) || t.chars().any(char::is_whitespace) {
        return false;
    }
    if t.contains("://") || t.starts_with(['/', '~', '.']) || is_uuid(t) {
        return false;
    }
    if let Some((user, domain)) = t.split_once('@')
        && !user.is_empty()
        && domain.contains('.')
        && !domain.contains('@')
        && t.chars()
            .all(|c| c.is_alphanumeric() || "._%+-@".contains(c))
    {
        return false;
    }
    let lower = t.chars().any(char::is_lowercase);
    let upper = t.chars().any(char::is_uppercase);
    let digit = t.chars().any(|c| c.is_ascii_digit());
    let other = t.chars().any(|c| !c.is_alphanumeric());
    let classes = [lower, upper, digit, other].iter().filter(|b| **b).count();
    (lower || upper) && digit && classes >= 3 && entropy(t) >= 3.0
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
    let fmt = image_ext(mime).unwrap_or("img").to_uppercase();
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

pub(crate) fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
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

pub(crate) fn ensure_private_dir(p: &Path) -> std::io::Result<()> {
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
        let home = std::env::var_os("HOME").map_or_else(|| "/tmp".into(), PathBuf::from);
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
        if !valid_image_name(&e.image) {
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
        if offered
            .iter()
            .any(|m| m.eq_ignore_ascii_case(desk_proto::INTERNAL_MIME))
        {
            return Some(Skip::Internal);
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
        } else if let Some(ext) = image_ext(mime) {
            if !self.cfg.images {
                return Some(Skip::ImagesOff);
            }
            if data.len() > self.cfg.image_max_bytes {
                return Some(Skip::TooLarge);
            }
            if !sniff(ext, data) {
                return Some(Skip::Unsupported);
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
        let expires =
            (is_text && looks_secret(&String::from_utf8_lossy(data))).then_some(now + SECRET_TTL);
        if let Some(pos) = self
            .items
            .iter()
            .position(|e| e.hash == hash && e.kind == k)
        {
            let mut e = self.items.remove(pos);
            e.time = now;
            if !e.pinned {
                e.expires = expires;
            }
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
            expires,
        };
        if is_text {
            e.text = String::from_utf8_lossy(data).into_owned();
        } else {
            let name = format!("{hash:016x}.{}", image_ext(mime).unwrap_or("img"));
            if let Err(skip) = self.store_image(&name, data) {
                return Pushed::Skipped(skip);
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
                .map_or_default(|p| p.to_string_lossy().into_owned()),
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

    /// Pin or unpin. Pinning is the user's choice to keep an item: a probable secret then loses
    /// its expiry and is saved like any other item.
    pub fn pin(&mut self, id: u64, pinned: bool) -> bool {
        let Some(e) = self.items.iter_mut().find(|e| e.id == id) else {
            return false;
        };
        let changed = e.pinned != pinned;
        e.pinned = pinned;
        if pinned {
            e.expires = None;
        }
        if !pinned {
            self.trim();
        }
        changed
    }

    /// Remove the probable secrets whose time is over. Returns whether any went.
    pub fn expire(&mut self, now: i64) -> bool {
        let (gone, keep): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|e| e.expires.is_some_and(|t| t <= now));
        self.items = keep;
        for e in &gone {
            self.remove_image_file(e);
        }
        !gone.is_empty()
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
        self.shrink_images_to_budget();
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
            .filter(|e| e.expires.is_none())
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
        let before = store.items.len();
        self.items = store
            .items
            .into_iter()
            .filter(|e| match e.kind.as_str() {
                kind::TEXT => e.image.is_empty(),
                kind::IMAGE => self.image_path(e).is_some(),
                _ => false,
            })
            .collect();
        if self.items.len() < before {
            eprintln!(
                "deskd: dropped {} history entries with a missing or invalid image file",
                before - self.items.len()
            );
        }
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

/// Запас вільного місця в runtime-теці, який зображення не чіпають: там сокети PipeWire, Wayland
/// і D-Bus, яким теж потрібні байти.
const RUNTIME_RESERVE: u64 = 32 * 1024 * 1024;

/// Вільні байти файлової системи теки (`statvfs`: `f_bavail * f_frsize`, через GIO).
fn free_bytes(dir: &Path) -> Option<u64> {
    use gio::prelude::FileExt;
    let info = gio::File::for_path(dir)
        .query_filesystem_info(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE, gio::Cancellable::NONE)
        .ok()?;
    Some(info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE))
}

/// Бюджет байтів і вільне місце для тимчасових зображень (R-5): окремо від решти рушія.
impl Engine {
    /// Зображення лежать у runtime-теці (tmpfs), а не в постійній.
    fn images_in_runtime(&self) -> bool {
        !(self.cfg.persist && self.cfg.persist_images)
    }

    /// Байти зображень в історії.
    fn image_bytes(&self) -> u64 {
        self.items
            .iter()
            .filter(|e| !e.image.is_empty())
            .map(|e| e.size)
            .sum()
    }

    /// Прибрати найстаріший незакріплений запис із зображенням. `false`, якщо такого немає.
    fn evict_oldest_image(&mut self) -> bool {
        let Some(pos) = self
            .items
            .iter()
            .rposition(|e| !e.pinned && !e.image.is_empty())
        else {
            return false;
        };
        let e = self.items.remove(pos);
        self.remove_image_file(&e);
        true
    }

    /// Знизити суму байтів зображень до бюджету (після зменшення бюджету в налаштуваннях).
    fn shrink_images_to_budget(&mut self) {
        if !self.images_in_runtime() {
            return;
        }
        let budget = self.cfg.image_budget_bytes as u64;
        while self.image_bytes() > budget && self.evict_oldest_image() {}
    }

    /// Звільнити місце під зображення в `need` байт: бюджет сумарних байтів, потім вільне місце
    /// файлової системи (`free` повертає його; `None` — невідомо, не блокуємо). Найстаріші
    /// зображення витісняються першими, закріплені — ніколи. `false`: місця не набралося.
    fn make_room(&mut self, need: u64, free: &dyn Fn() -> Option<u64>) -> bool {
        let budget = self.cfg.image_budget_bytes as u64;
        if need > budget {
            return false;
        }
        while self.image_bytes() + need > budget {
            if !self.evict_oldest_image() {
                return false;
            }
        }
        while free().is_some_and(|f| f < need.saturating_add(RUNTIME_RESERVE)) {
            if !self.evict_oldest_image() {
                return false;
            }
        }
        true
    }

    /// Записати файл зображення у відповідну теку. У runtime-теці — з урахуванням бюджету й
    /// вільного місця.
    fn store_image(&mut self, name: &str, data: &[u8]) -> Result<(), Skip> {
        let dir = self.images_dir(!self.images_in_runtime());
        ensure_private_dir(&dir).map_err(|_| Skip::Unsupported)?;
        if self.images_in_runtime() && !self.make_room(data.len() as u64, &|| free_bytes(&dir)) {
            return Err(Skip::TooLarge);
        }
        write_private(&dir.join(name), data).map_err(|_| Skip::Unsupported)
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

    fn image_of(n: usize, tag: u8) -> Vec<u8> {
        let mut v = png(2, 2);
        v.resize(n, tag);
        v
    }

    /// Бюджет байтів: найстаріші зображення йдуть першими, закріплені лишаються, а зображення,
    /// що не вміщується навіть після витіснення, пропускається.
    #[test]
    fn image_budget_evicts_oldest_first() {
        let cfg = Config {
            image_budget_bytes: 3000,
            ..Config::default()
        };
        let (mut e, d) = engine("budget", cfg);
        let Pushed::Added(a) = e.push("image/png", &image_of(1000, 1), &[], "", 1) else {
            panic!("a")
        };
        let Pushed::Added(b) = e.push("image/png", &image_of(1000, 2), &[], "", 2) else {
            panic!("b")
        };
        let Pushed::Added(c) = e.push("image/png", &image_of(1000, 3), &[], "", 3) else {
            panic!("c")
        };
        assert_eq!(e.image_bytes(), 3000);
        // четверте витісняє найстаріше (a); файл теж зникає
        let a_path = e.image_path(e.find(a).unwrap()).unwrap();
        let Pushed::Added(_) = e.push("image/png", &image_of(1000, 4), &[], "", 4) else {
            panic!("d")
        };
        assert!(e.find(a).is_none() && !a_path.exists());
        assert!(e.find(b).is_some() && e.find(c).is_some());
        // закріплене не витісняється: b закріплено, тож іде c
        e.pin(b, true);
        let Pushed::Added(_) = e.push("image/png", &image_of(1000, 5), &[], "", 5) else {
            panic!("e")
        };
        assert!(e.find(b).is_some() && e.find(c).is_none());
        // більше за весь бюджет — пропуск без шкоди для історії
        let n = e.len();
        assert_eq!(
            e.push("image/png", &image_of(3001, 6), &[], "", 6),
            Pushed::Skipped(Skip::TooLarge)
        );
        assert_eq!(e.len(), n);
        assert!(e.image_bytes() <= 3000);
        // усе закріплено — нічого витісняти: пропуск
        let ids: Vec<u64> = e.list("", 0).iter().map(|i| i.id).collect();
        for id in ids {
            e.pin(id, true);
        }
        assert_eq!(
            e.push("image/png", &image_of(2500, 7), &[], "", 7),
            Pushed::Skipped(Skip::TooLarge)
        );
        let _ = std::fs::remove_dir_all(d);
    }

    /// Зменшення бюджету в налаштуваннях одразу витісняє зайве.
    #[test]
    fn lowering_the_budget_evicts() {
        let (mut e, d) = engine("budget-lower", Config::default());
        for t in 1..=3u8 {
            e.push("image/png", &image_of(1000, t), &[], "", i64::from(t));
        }
        assert_eq!(e.image_bytes(), 3000);
        let cfg = Config {
            image_budget_bytes: 1500,
            ..e.cfg.clone()
        };
        assert!(e.set_config(cfg));
        assert_eq!(e.image_bytes(), 1000);
        let _ = std::fs::remove_dir_all(d);
    }

    /// Мало вільного місця у файловій системі: витісняємо свої зображення, а коли їх немає —
    /// не пишемо.
    #[test]
    fn free_space_is_respected() {
        let (mut e, d) = engine("budget-free", Config::default());
        e.push("image/png", &image_of(1000, 1), &[], "", 1);
        e.push("image/png", &image_of(1000, 2), &[], "", 2);
        // вільно менше за запас: витісняє обидва, і все одно не вміщається
        assert!(!e.make_room(10, &|| Some(RUNTIME_RESERVE - 1)));
        assert_eq!(e.image_bytes(), 0);
        // достатньо місця — нічого не чіпає; невідомо — теж
        e.push("image/png", &image_of(1000, 3), &[], "", 3);
        assert!(e.make_room(10, &|| Some(RUNTIME_RESERVE + 10)));
        assert!(e.make_room(10, &|| None));
        assert_eq!(e.image_bytes(), 1000);
        // справжній statvfs працює на існуючій теці
        assert!(free_bytes(&d).is_some());
        let _ = std::fs::remove_dir_all(d);
    }

    /// S-8: names in a tampered history.json never reach a path; text entries carry no image.
    #[test]
    fn load_drops_untrusted_image_names() {
        let cfg = Config {
            persist_images: true,
            ..Config::default()
        };
        let (_, d) = engine("s8", cfg.clone());
        let data = d.join("data");
        std::fs::create_dir_all(data.join("images")).unwrap();
        let good = "0123456789abcdef.png";
        std::fs::write(data.join("images").join(good), png(1, 1)).unwrap();
        // a file outside the images directory that a crafted name would point at
        std::fs::write(d.join("victim"), b"keep").unwrap();
        let entry = |id: u64, kind: &str, image: &str| {
            format!(
                r#"{{"id":{id},"kind":"{kind}","mime":"image/png","text":"t{id}","image":"{image}","pinned":true,"time":1,"size":1,"hash":{id}}}"#
            )
        };
        let items = [
            entry(1, "image", good),
            entry(2, "image", "../../victim"),
            entry(3, "image", "/etc/passwd"),
            entry(4, "image", "0123456789ABCDEF.png"),
            entry(5, "image", "0123456789abcdef.svg"),
            entry(6, "text", "../../victim"),
            entry(7, "other", ""),
        ];
        let store = format!(
            r#"{{"version":2,"next_id":8,"items":[{}]}}"#,
            items.join(",")
        );
        std::fs::write(data.join("history.json"), store).unwrap();
        let mut e = Engine::new(cfg, data, d.join("run"));
        e.load();
        let ids: Vec<u64> = e.list("", 0).iter().map(|i| i.id).collect();
        assert_eq!(ids, [1]);
        assert!(e.get(2).is_none());
        assert!(!e.delete(2));
        assert!(d.join("victim").exists());
        for (name, ok) in [
            (good, true),
            ("0123456789abcdef.jpg", true),
            ("0123456789abcdef.webp", true),
            ("0123456789abcdef", false),
            ("0123456789abcde.png", false),
            ("0123456789abcdefg.png", false),
            ("../123456789abcdef.png", false),
            ("0123456789abcdef.png/..", false),
            ("", false),
        ] {
            assert_eq!(valid_image_name(name), ok, "{name}");
        }
        let _ = std::fs::remove_dir_all(d);
    }

    /// S-9: SVG is not recorded, and neither is anything whose bytes do not match its type.
    #[test]
    fn only_sniffed_raster_images() {
        let (mut e, d) = engine("s9", Config::default());
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#;
        for (mime, data) in [
            ("image/svg+xml", svg.as_slice()),
            ("image/png", svg.as_slice()),
            ("image/jpeg", b"\x89PNG\r\n\x1a\n".as_slice()),
            ("image/bmp", b"BM\0\0".as_slice()),
        ] {
            assert_eq!(
                e.push(mime, data, &[], "", 1),
                Pushed::Skipped(Skip::Unsupported),
                "{mime}"
            );
        }
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0, 0x10];
        assert!(matches!(
            e.push("image/jpeg", &jpeg, &[], "", 1),
            Pushed::Added(_)
        ));
        let webp = b"RIFF\x10\0\0\0WEBPVP8 ";
        assert!(matches!(
            e.push("image/webp", webp, &[], "", 1),
            Pushed::Added(_)
        ));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn secret_heuristic() {
        for t in [
            "hR7#kq2!Lm9z",
            "Tr0ub4dor&3xyz",
            "ghp_x9A2bLq7Zr4TmW1cVd8EoPk3Ny6sHu0F",
            "sk-proj-4fT9xQ2mZ7",
            "  aB3dE5gH7jK9  ",
        ] {
            assert!(looks_secret(t), "{t}");
        }
        for t in [
            "hello world 123",
            "password",
            "abc123",
            "getConnectionCredentials",
            "8991754",
            "0123456789abcdef0123456789abcdef01234567",
            "550e8400-e29b-41d4-a716-446655440000",
            "https://example.org/a?b=C1",
            "/home/user/Docs/File1.txt",
            "User.Name1@example.org",
            "aaaaaaaaaaaaA1",
            "Привіт, Світ",
        ] {
            assert!(!looks_secret(t), "{t}");
        }
    }

    /// A probable secret: recorded, never saved, gone after SECRET_TTL; pinning keeps it.
    #[test]
    fn probable_secrets_are_memory_only() {
        let (mut e, d) = engine("ttl", Config::default());
        assert_eq!(e.push(T, b"plain text", &[], "", 100), Pushed::Added(1));
        assert_eq!(e.push(T, b"hR7#kq2!Lm9z", &[], "", 100), Pushed::Added(2));
        assert_eq!(e.push(T, b"Tr0ub4dor&3xyz", &[], "", 100), Pushed::Added(3));
        e.save().unwrap();
        let saved = std::fs::read_to_string(d.join("data/history.json")).unwrap();
        assert!(saved.contains("plain text"));
        assert!(!saved.contains("hR7#kq2!Lm9z") && !saved.contains("Tr0ub4dor"));
        assert!(e.pin(3, true));
        assert!(!e.expire(100 + SECRET_TTL - 1));
        assert!(e.expire(100 + SECRET_TTL));
        let ids: Vec<u64> = e.list("", 0).iter().map(|i| i.id).collect();
        assert_eq!(ids, [3, 1]);
        e.save().unwrap();
        let saved = std::fs::read_to_string(d.join("data/history.json")).unwrap();
        assert!(saved.contains("Tr0ub4dor"), "pinned: saved");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn hints_and_own_pastes_are_skipped() {
        let (mut e, d) = engine("hints", Config::default());
        for hint in SECRET_HINTS {
            let offered = vec![T.to_string(), hint.to_ascii_lowercase()];
            assert_eq!(
                e.push(T, b"x", &offered, "", 1),
                Pushed::Skipped(Skip::Sensitive),
                "{hint}"
            );
        }
        let ours = vec![T.to_string(), desk_proto::INTERNAL_MIME.to_string()];
        assert_eq!(
            e.push(T, b"x", &ours, "", 1),
            Pushed::Skipped(Skip::Internal)
        );
        assert!(e.is_empty());
        let _ = std::fs::remove_dir_all(d);
    }
}
