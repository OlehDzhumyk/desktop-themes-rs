// SPDX-License-Identifier: GPL-3.0-or-later
//! OCR core of deskd (`docs/spec/features/ocr.md`): text post-processing, language helpers and an
//! isolated Tesseract runner. Pure functions plus one child process; no D-Bus, no settings.
//!
//! The D-Bus engine that wires this module in comes in the follow-up change, so nothing here has a
//! caller yet; the module-level `allow(dead_code)` below goes away with that change.
#![allow(dead_code)]

use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::clipboard::ensure_private_dir;

/// The recognition limit of the spec (the process is killed after it).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// Why [`run_tesseract`] gave no text.
#[derive(Debug)]
pub enum OcrError {
    /// The `tesseract` binary does not exist.
    MissingEngine,
    /// The language list is empty, so there is nothing to recognise with.
    NoLanguages,
    /// The process ran past the time limit and was killed.
    Timeout,
    /// The process exited with an error; the text is its (trimmed) standard error.
    Failed(String),
    /// A file or process operation failed.
    Io(std::io::Error),
}

impl fmt::Display for OcrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OcrError::MissingEngine => write!(f, "tesseract is not installed"),
            OcrError::NoLanguages => write!(f, "no OCR language is available"),
            OcrError::Timeout => write!(f, "tesseract timed out and was killed"),
            OcrError::Failed(why) => write!(f, "tesseract failed: {why}"),
            OcrError::Io(e) => write!(f, "OCR input/output error: {e}"),
        }
    }
}

impl std::error::Error for OcrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OcrError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for OcrError {
    fn from(e: std::io::Error) -> Self {
        OcrError::Io(e)
    }
}

// ---------------------------------------------------------------------------------------------
// Text post-processing
// ---------------------------------------------------------------------------------------------

/// A line ends in a word broken with `-` and the next line goes on in lower case: the hyphen is
/// a line-break artefact, not part of a compound or a list marker (`"- item"`, `"a -"`).
fn is_hyphenated_break(prev: &str, next: &str) -> bool {
    let Some(head) = prev.strip_suffix('-') else {
        return false;
    };
    head.chars().next_back().is_some_and(char::is_alphabetic)
        && next
            .trim_start()
            .chars()
            .next()
            .is_some_and(char::is_lowercase)
}

/// Clean the raw output of Tesseract for the clipboard.
///
/// * line ends `\r\n` and `\r` become `\n`; form feeds (Tesseract adds one per page) are removed;
/// * trailing whitespace of every line is cut;
/// * a word broken at a line end (`"hy-\nphen"`) is joined, only for a letter before the hyphen and
///   a lower-case letter after the break;
/// * paragraph breaks (blank lines) are kept, but a run of blank lines becomes one;
///   blank lines at the start and the end are removed;
/// * with `keep_line_breaks == false` the lines of a paragraph are joined with one space.
#[must_use]
pub fn clean_text(raw: &str, keep_line_breaks: bool) -> String {
    let normalised = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\x0c', "");

    // Hyphenation join, one pass: the merged line can end in a hyphen again ("a-\nb-\nc").
    let mut lines: Vec<String> = Vec::new();
    for line in normalised.lines() {
        let line = line.trim_end();
        if let Some(prev) = lines.last_mut()
            && is_hyphenated_break(prev, line)
        {
            prev.pop();
            prev.push_str(line.trim_start());
            continue;
        }
        lines.push(line.to_string());
    }

    // Paragraphs: runs of non-blank lines.
    let mut paragraphs: Vec<Vec<&str>> = Vec::new();
    let mut open = false;
    for line in &lines {
        if line.trim().is_empty() {
            open = false;
        } else if open {
            if let Some(p) = paragraphs.last_mut() {
                p.push(line);
            }
        } else {
            paragraphs.push(vec![line]);
            open = true;
        }
    }

    let render = |p: &Vec<&str>| {
        if keep_line_breaks {
            p.join("\n")
        } else {
            p.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ")
        }
    };
    paragraphs
        .iter()
        .map(render)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The first non-empty line, cut to `max_chars` characters (Unicode scalar values) with `…` when
/// it was longer. Empty when the text has no visible character.
#[must_use]
pub fn preview_line(text: &str, max_chars: usize) -> String {
    let Some(line) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return String::new();
    };
    match line.char_indices().nth(max_chars) {
        None => line.to_string(),
        Some((end, _)) => format!("{}…", &line[..end]),
    }
}

// ---------------------------------------------------------------------------------------------
// Languages
// ---------------------------------------------------------------------------------------------

/// The language codes in the output of `tesseract --list-langs`: the header line, empty lines and
/// the orientation model `osd` (not a language) are left out.
#[must_use]
pub fn parse_tesseract_langs(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("List of available languages") && *l != "osd")
        .map(str::to_string)
        .collect()
}

/// Split the requested codes into `(available, missing)`, in the requested order and without
/// repeats. The caller warns about the `missing` ones (`tesseract-langpack-<code>`) and runs
/// with the `available` ones.
#[must_use]
pub fn filter_languages(requested: &[String], installed: &[String]) -> (Vec<String>, Vec<String>) {
    let mut available: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for code in requested {
        if available.contains(code) || missing.contains(code) {
            continue;
        }
        if installed.contains(code) {
            available.push(code.clone());
        } else {
            missing.push(code.clone());
        }
    }
    (available, missing)
}

// ---------------------------------------------------------------------------------------------
// Isolated runner
// ---------------------------------------------------------------------------------------------

/// A 0600 file in the private OCR directory that is deleted when the guard drops: on success, on
/// error, on timeout and while a panic unwinds (the pixels are as sensitive as the screen).
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn create(dir: &Path, data: &[u8]) -> std::io::Result<TempFile> {
        use std::os::unix::fs::OpenOptionsExt;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let pid = std::process::id();
        loop {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("ocr-{pid}-{n}.img"));
            // `create_new` (O_EXCL): never write through a file or link that already exists
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(mut file) => {
                    // the guard exists before the first write: a failed write deletes the file
                    let guard = TempFile { path };
                    file.write_all(data)?;
                    return Ok(guard);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Which pipe a reader thread drained.
enum Pipe {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

fn drain(mut pipe: impl Read + Send + 'static, wrap: fn(Vec<u8>) -> Pipe, tx: mpsc::Sender<Pipe>) {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        // a read error keeps what was read so far; the exit status tells success from failure
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(wrap(buf));
    });
}

fn describe_failure(status: ExitStatus, stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        status.to_string()
    } else {
        stderr.to_string()
    }
}

/// Run `bin` on `image_bytes` and return the raw text (feed it to [`clean_text`]).
///
/// The image goes to a 0600 file in `<runtime_dir>/ocr/` (a 0700 directory) that is deleted when the
/// call ends, whatever the outcome. The command line is `<file> stdout -l <langs joined with +>`
/// with no standard input. After `timeout` the child is killed and reaped
/// ([`OcrError::Timeout`]). The wait blocks on a channel; nothing polls.
///
/// A child that starts its own children and leaves them running keeps the pipes open after
/// the kill; the reader threads then end when those children do. Tesseract starts none.
///
/// # Errors
/// [`OcrError::NoLanguages`] for an empty `langs` (nothing is written), [`OcrError::MissingEngine`]
/// when `bin` does not exist, [`OcrError::Failed`] for a non-zero exit, [`OcrError::Timeout`], and
/// [`OcrError::Io`] for any other file or process failure.
pub fn run_tesseract(
    bin: &Path,
    image_bytes: &[u8],
    langs: &[String],
    runtime_dir: &Path,
    timeout: Duration,
) -> Result<String, OcrError> {
    if langs.is_empty() {
        return Err(OcrError::NoLanguages);
    }
    let dir = runtime_dir.join("ocr");
    ensure_private_dir(&dir)?;
    let image = TempFile::create(&dir, image_bytes)?;

    let mut child = Command::new(bin)
        .arg(&image.path)
        .arg("stdout")
        .arg("-l")
        .arg(langs.join("+"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                OcrError::MissingEngine
            } else {
                OcrError::Io(e)
            }
        })?;

    let (tx, rx) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        drain(out, Pipe::Stdout, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        drain(err, Pipe::Stderr, tx.clone());
    }
    drop(tx);

    // Both pipes reach end of file when the process exits.
    let deadline = Instant::now() + timeout;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    for _ in 0..2 {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Pipe::Stdout(b)) => stdout = b,
            Ok(Pipe::Stderr(b)) => stderr = b,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(OcrError::Timeout);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = child.wait()?;
    if status.success() {
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    } else {
        Err(OcrError::Failed(describe_failure(
            status,
            &String::from_utf8_lossy(&stderr),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_string()).collect()
    }

    // ----- clean_text -----

    #[test]
    fn clean_text_cases() {
        // (raw, keep_line_breaks, expected)
        let cases: &[(&str, bool, &str)] = &[
            // hyphenation: Latin and Cyrillic, both modes
            ("hy-\nphen", true, "hyphen"),
            ("hy-\nphen", false, "hyphen"),
            ("роз-\nпізнавання тексту", true, "розпізнавання тексту"),
            ("Пере-\nнос слова", false, "Перенос слова"),
            // chained break across three lines
            ("a-\nb-\nc", true, "abc"),
            // continuation indent is dropped with the join
            ("hy-\n   phen", true, "hyphen"),
            // not joined: upper case or digit after the break, no letter before the hyphen, list items
            ("foo-\nBar", true, "foo-\nBar"),
            ("x-\n1", true, "x-\n1"),
            ("foo -\nbar", true, "foo -\nbar"),
            ("list:\n- one\n- two", true, "list:\n- one\n- two"),
            ("well-known fact", true, "well-known fact"),
            // not joined across a paragraph break
            ("end-\n\nnext", true, "end-\n\nnext"),
            // line ends, form feed, trailing whitespace
            ("one\r\ntwo\rthree", true, "one\ntwo\nthree"),
            ("one  \t\ntwo \n", true, "one\ntwo"),
            ("text\n\x0c", true, "text"),
            ("page one\n\x0cpage two\n\x0c", true, "page one\npage two"),
            // paragraphs kept, blank-line runs collapse, outer blank lines go
            (
                "\n\n  \nfirst\nline\n\n\n\nsecond\n\n",
                true,
                "first\nline\n\nsecond",
            ),
            ("a\n   \nb", true, "a\n\nb"),
            // keep_line_breaks = false joins inside a paragraph only
            (
                "first\nline\n\nsecond\n  line  ",
                false,
                "first line\n\nsecond line",
            ),
            (
                "Привіт,\nсвіте\n\nДобрий\nдень",
                false,
                "Привіт, світе\n\nДобрий день",
            ),
            // indentation survives with line breaks kept
            ("  indented\nnext", true, "  indented\nnext"),
            // empty and whitespace-only input
            ("", true, ""),
            ("\x0c\n  \r\n", false, ""),
        ];
        for (raw, keep, want) in cases {
            assert_eq!(clean_text(raw, *keep), *want, "raw {raw:?}, keep {keep}");
        }
    }

    // ----- preview_line -----

    #[test]
    fn preview_line_cases() {
        assert_eq!(preview_line("short", 60), "short");
        assert_eq!(preview_line("", 60), "");
        assert_eq!(preview_line("\n  \n", 60), "");
        // first non-empty line, trimmed
        assert_eq!(preview_line("\n\n  first line \nsecond", 60), "first line");
        // exactly at the limit: no ellipsis
        assert_eq!(preview_line("abcde", 5), "abcde");
        assert_eq!(preview_line("abcdef", 5), "abcde…");
        assert_eq!(preview_line("abc", 0), "…");
    }

    #[test]
    fn preview_line_counts_characters_not_bytes() {
        let ukrainian = "Розпізнавання тексту з екрана працює локально без мережі і хмари ще довше";
        assert!(ukrainian.chars().count() > 60);
        let got = preview_line(ukrainian, 60);
        assert_eq!(got.chars().count(), 61);
        assert!(got.ends_with('…'));
        assert!(got.starts_with("Розпізнавання тексту"));
        // 60 Ukrainian letters are 120 bytes and still fit
        let exact = "ї".repeat(60);
        assert_eq!(preview_line(&exact, 60), exact);
    }

    // ----- languages -----

    #[test]
    fn parse_tesseract_langs_skips_header_osd_and_blanks() {
        let out = "List of available languages in \"/usr/share/tesseract/tessdata/\" (4):\neng\nosd\nukr\n\n  deu \n";
        assert_eq!(parse_tesseract_langs(out), s(&["eng", "ukr", "deu"]));
        assert_eq!(parse_tesseract_langs(""), Vec::<String>::new());
        assert_eq!(
            parse_tesseract_langs("List of available languages (1):\nosd\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn filter_languages_splits_available_and_missing() {
        let installed = s(&["eng", "deu"]);
        // all present
        assert_eq!(
            filter_languages(&s(&["eng", "deu"]), &installed),
            (s(&["eng", "deu"]), vec![])
        );
        // partly missing, requested order kept
        assert_eq!(
            filter_languages(&s(&["ukr", "eng"]), &installed),
            (s(&["eng"]), s(&["ukr"]))
        );
        // all missing
        assert_eq!(
            filter_languages(&s(&["ukr", "pol"]), &installed),
            (vec![], s(&["ukr", "pol"]))
        );
        // repeats count once
        assert_eq!(
            filter_languages(&s(&["eng", "ukr", "eng", "ukr"]), &installed),
            (s(&["eng"]), s(&["ukr"]))
        );
        assert_eq!(filter_languages(&[], &installed), (vec![], vec![]));
    }

    // ----- run_tesseract -----

    /// Writing and starting a fresh executable while another test thread forks can fail with
    /// "text file busy" (the fork still holds the write descriptor). One lock for all runner tests.
    static SPAWN: Mutex<()> = Mutex::new(());

    struct Sandbox {
        dir: PathBuf,
    }

    impl Sandbox {
        fn new(tag: &str) -> Sandbox {
            let dir = std::env::temp_dir().join(format!("deskd-ocr-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Sandbox { dir }
        }

        fn runtime(&self) -> PathBuf {
            self.dir.join("run")
        }

        fn script(&self, body: &str) -> PathBuf {
            let path = self.dir.join("fake-tesseract");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        /// Names left in `<runtime>/ocr/`.
        fn leftovers(&self) -> Vec<String> {
            std::fs::read_dir(self.runtime().join("ocr"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn run_returns_stdout_and_deletes_the_image() {
        let _lock = SPAWN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sb = Sandbox::new("ok");
        let seen = sb.dir.join("seen");
        let bin = sb.script(&format!(
            "cat \"$1\" > '{0}.img'\nstat -c %a \"$1\" > '{0}.mode'\nprintf '%s|' \"$2\" \"$3\" \"$4\" > '{0}.args'\nprintf 'Привіт, world\\n\\f'",
            seen.display()
        ));
        let out = run_tesseract(
            &bin,
            b"PIXELS",
            &s(&["ukr", "eng"]),
            &sb.runtime(),
            Duration::from_secs(20),
        )
        .unwrap();
        assert_eq!(out, "Привіт, world\n\x0c");
        assert_eq!(clean_text(&out, true), "Привіт, world");

        let read =
            |ext: &str| std::fs::read_to_string(format!("{}.{ext}", seen.display())).unwrap();
        assert_eq!(read("img"), "PIXELS");
        assert_eq!(read("mode").trim(), "600");
        assert_eq!(read("args"), "stdout|-l|ukr+eng|");
        assert!(sb.leftovers().is_empty());
        let mode = std::fs::metadata(sb.runtime().join("ocr"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn run_failure_returns_stderr_and_deletes_the_image() {
        let _lock = SPAWN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sb = Sandbox::new("fail");
        let bin = sb.script("echo 'Error opening data file' >&2\nexit 3");
        let err =
            run_tesseract(&bin, b"x", &s(&["eng"]), &sb.runtime(), DEFAULT_TIMEOUT).unwrap_err();
        assert!(
            matches!(&err, OcrError::Failed(why) if why == "Error opening data file"),
            "{err:?}"
        );
        assert!(sb.leftovers().is_empty());

        // no message on stderr: the exit status is reported instead
        let bin = sb.script("exit 3");
        let err =
            run_tesseract(&bin, b"x", &s(&["eng"]), &sb.runtime(), DEFAULT_TIMEOUT).unwrap_err();
        assert!(
            matches!(&err, OcrError::Failed(why) if why.contains('3')),
            "{err:?}"
        );
        assert!(sb.leftovers().is_empty());
    }

    #[test]
    fn run_missing_binary_is_missing_engine_and_deletes_the_image() {
        let sb = Sandbox::new("missing");
        let err = run_tesseract(
            &sb.dir.join("no-such-tesseract"),
            b"x",
            &s(&["eng"]),
            &sb.runtime(),
            DEFAULT_TIMEOUT,
        )
        .unwrap_err();
        assert!(matches!(err, OcrError::MissingEngine), "{err:?}");
        assert!(sb.leftovers().is_empty());
    }

    #[test]
    fn run_timeout_kills_the_child_and_deletes_the_image() {
        let _lock = SPAWN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sb = Sandbox::new("hang");
        let pid_file = sb.dir.join("pid");
        // `exec` keeps the pid, so the kill reaches the sleeping process itself
        let bin = sb.script(&format!(
            "echo $$ > '{}'\nexec sleep 30",
            pid_file.display()
        ));
        let err = run_tesseract(
            &bin,
            b"x",
            &s(&["eng"]),
            &sb.runtime(),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(matches!(err, OcrError::Timeout), "{err:?}");
        assert!(sb.leftovers().is_empty());
        // The child was killed and reaped: its pid is gone. (On a very slow start it was killed
        // before it wrote the pid, which is also fine.)
        if let Ok(pid) = std::fs::read_to_string(&pid_file) {
            assert!(
                !Path::new("/proc").join(pid.trim()).exists(),
                "pid {pid} still alive"
            );
        }
    }

    #[test]
    fn run_without_languages_writes_nothing() {
        let sb = Sandbox::new("nolang");
        let err = run_tesseract(
            &sb.dir.join("no-such-tesseract"),
            b"x",
            &[],
            &sb.runtime(),
            DEFAULT_TIMEOUT,
        )
        .unwrap_err();
        assert!(matches!(err, OcrError::NoLanguages), "{err:?}");
        assert!(!sb.runtime().join("ocr").exists());
    }

    #[test]
    fn temp_file_is_deleted_on_panic() {
        let sb = Sandbox::new("panic");
        let dir = sb.runtime().join("ocr");
        ensure_private_dir(&dir).unwrap();
        let result = std::panic::catch_unwind(|| {
            let _file = TempFile::create(&dir, b"x").unwrap();
            panic!("boom");
        });
        assert!(result.is_err());
        assert!(sb.leftovers().is_empty());
    }
}
