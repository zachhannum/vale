//! The log file of the app. On iOS, it is in the Documents folder, and the
//! Files app shows it as "On My iPhone > Vale > vale-log.txt". It holds the
//! warnings and the errors of the `log` crate, the panics, and the normal
//! exit. On iOS, it also holds the memory use and the changes between the
//! foreground and the background.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const FILE_NAME: &str = "vale-log.txt";

/// When the file grows past this size, it keeps the newest half.
pub const MAX_LEN: u64 = 1 << 20;

pub struct LogFile {
    start: Instant,
    inner: Mutex<Inner>,
}

struct Inner {
    path: PathBuf,
    file: File,
    len: u64,
    last: Option<String>,
    repeats: u32,
}

impl LogFile {
    /// Opens the file to add lines at its end.
    pub fn open(path: &Path) -> io::Result<LogFile> {
        let (file, len) = open_trimmed(path)?;
        let inner = Inner {
            path: path.to_path_buf(),
            file,
            len,
            last: None,
            repeats: 0,
        };
        Ok(LogFile {
            start: Instant::now(),
            inner: Mutex::new(inner),
        })
    }

    /// Writes one line with the time. A line that is the same as the line
    /// before it is counted, not written.
    pub fn write(&self, text: &str) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if inner.last.as_deref() == Some(text) {
            inner.repeats += 1;
            return;
        }
        if inner.repeats > 0 {
            let repeats = format!("(repeated {} times)", inner.repeats);
            inner.repeats = 0;
            self.put(&mut inner, &repeats);
        }
        self.put(&mut inner, text);
        inner.last = Some(text.to_string());
    }

    fn put(&self, inner: &mut Inner, text: &str) {
        let line = format!(
            "{} +{:.1}s {text}\n",
            utc_time(SystemTime::now()),
            self.start.elapsed().as_secs_f64()
        );
        if inner.file.write_all(line.as_bytes()).is_err() {
            return;
        }
        inner.len += line.len() as u64;
        if inner.len > MAX_LEN
            && let Ok((file, len)) = open_trimmed(&inner.path)
        {
            inner.file = file;
            inner.len = len;
        }
    }
}

/// Opens the file for appending. If the file is larger than `MAX_LEN`, it
/// first keeps only the newest half, from the start of a line.
fn open_trimmed(path: &Path) -> io::Result<(File, u64)> {
    let len = std::fs::metadata(path).map_or(0, |m| m.len());
    if len > MAX_LEN {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(len - MAX_LEN / 2))?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail)?;
        let from = tail.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
        std::fs::write(path, &tail[from..])?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let len = file.metadata()?.len();
    Ok((file, len))
}

/// The time in UTC, for example `2026-11-13 09:41:07Z`.
fn utc_time(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // The civil date of a day count, after Howard Hinnant.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

static LOG: OnceLock<LogFile> = OnceLock::new();

/// Writes one line to the log file of the app, if the app has one.
pub fn write(text: &str) {
    if let Some(log) = LOG.get() {
        log.write(text);
    }
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            write(&format!(
                "{} {}: {}",
                record.level(),
                record.target(),
                record.args()
            ));
        }
    }

    fn flush(&self) {}
}

unsafe extern "C" {
    fn atexit(f: extern "C" fn()) -> std::ffi::c_int;
}

extern "C" fn on_exit() {
    write("exit");
}

/// Makes `path` the log file of the app. It writes the warnings and the
/// errors of the `log` crate, each panic, and the normal exit of the
/// process. Call it one time, at the start.
pub fn install(path: &Path) -> io::Result<()> {
    let log = LogFile::open(path)?;
    log.write(&format!("start, version {}", env!("CARGO_PKG_VERSION")));
    if LOG.set(log).is_err() {
        return Err(io::Error::other("the log file is already set"));
    }
    if log::set_logger(&Logger).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let place = info
            .location()
            .map_or(String::new(), |l| format!(" at {}:{}", l.file(), l.line()));
        let message = info.payload_as_str().unwrap_or("no message");
        write(&format!(
            "panic in thread {}{place}: {message}",
            thread.name().unwrap_or("unnamed")
        ));
        previous(info);
    }));
    // SAFETY: `on_exit` is a function with no arguments that never unwinds.
    unsafe { atexit(on_exit) };
    #[cfg(target_os = "ios")]
    ios::install();
    Ok(())
}

#[cfg(target_os = "ios")]
mod ios {
    use std::time::{Duration, Instant};

    use objc2_foundation::ns_string;

    use super::write;

    const STEP: u64 = 16 << 20;

    /// The fields of `task_vm_info` up to `phys_footprint`.
    #[repr(C, packed(4))]
    #[derive(Default)]
    struct TaskVmInfo {
        virtual_size: u64,
        region_count: i32,
        page_size: i32,
        resident_size: u64,
        resident_size_peak: u64,
        device: u64,
        device_peak: u64,
        internal: u64,
        internal_peak: u64,
        external: u64,
        external_peak: u64,
        reusable: u64,
        reusable_peak: u64,
        purgeable_volatile_pmap: u64,
        purgeable_volatile_resident: u64,
        purgeable_volatile_virtual: u64,
        compressed: u64,
        compressed_peak: u64,
        compressed_lifetime: u64,
        phys_footprint: u64,
    }

    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    }

    const TASK_VM_INFO: u32 = 22;

    /// The memory of the app that counts toward the limit of iOS.
    fn footprint() -> Option<u64> {
        let mut info = TaskVmInfo::default();
        let mut count = (size_of::<TaskVmInfo>() / 4) as u32;
        // SAFETY: `info` has room for `count` 32-bit words, and the kernel
        // writes at most `count` words.
        let result = unsafe {
            task_info(
                mach_task_self_,
                TASK_VM_INFO,
                (&raw mut info).cast(),
                &mut count,
            )
        };
        (result == 0).then_some(info.phys_footprint)
    }

    pub fn install() {
        std::thread::Builder::new()
            .name("log-memory".into())
            .spawn(|| {
                let mut last: Option<(u64, Instant)> = None;
                loop {
                    if let Some(bytes) = footprint() {
                        let due = last.is_none_or(|(before, at)| {
                            bytes.abs_diff(before) >= STEP
                                || at.elapsed() >= Duration::from_secs(60)
                        });
                        if due {
                            write(&format!("memory {} MB", bytes >> 20));
                            last = Some((bytes, Instant::now()));
                        }
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            })
            .ok();

        // The name of each notification is the text of its constant.
        for (name, text) in [
            (
                ns_string!("UIApplicationDidReceiveMemoryWarningNotification"),
                "memory warning",
            ),
            (
                ns_string!("UIApplicationWillResignActiveNotification"),
                "inactive",
            ),
            (
                ns_string!("UIApplicationDidEnterBackgroundNotification"),
                "background",
            ),
            (
                ns_string!("UIApplicationWillEnterForegroundNotification"),
                "foreground",
            ),
            (
                ns_string!("UIApplicationDidBecomeActiveNotification"),
                "active",
            ),
        ] {
            crate::files::cloud::observe(name, move |_| write(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(path: &Path) -> Vec<String> {
        let text = std::fs::read_to_string(path).unwrap();
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn each_line_has_the_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let log = LogFile::open(&path).unwrap();
        log.write("one");
        log.write("two");
        let lines = lines(&path);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("s one"), "{}", lines[0]);
        assert!(lines[0].contains("Z +"), "{}", lines[0]);
        assert!(lines[1].ends_with("s two"), "{}", lines[1]);
    }

    #[test]
    fn a_repeated_line_is_written_one_time_with_its_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let log = LogFile::open(&path).unwrap();
        for _ in 0..4 {
            log.write("same");
        }
        log.write("other");
        let lines = lines(&path);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].ends_with("same"));
        assert!(lines[1].ends_with("(repeated 3 times)"));
        assert!(lines[2].ends_with("other"));
    }

    #[test]
    fn a_log_past_the_limit_keeps_the_newest_half() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let log = LogFile::open(&path).unwrap();
        let mut n = 0;
        while std::fs::metadata(&path).unwrap().len() < MAX_LEN * 3 / 4 || n < 30_000 {
            log.write(&format!("line {n}"));
            n += 1;
        }
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len <= MAX_LEN, "{len}");
        let lines = lines(&path);
        assert!(lines.last().unwrap().ends_with(&format!("line {}", n - 1)));
        assert!(!lines[0].ends_with("line 0"));
        assert!(
            lines[0].contains("Z +"),
            "the first line is whole: {}",
            lines[0]
        );
    }

    #[test]
    fn a_large_file_is_trimmed_when_it_opens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let old: String = (0..100_000).map(|i| format!("old line {i}\n")).collect();
        assert!(old.len() as u64 > MAX_LEN);
        std::fs::write(&path, &old).unwrap();
        LogFile::open(&path).unwrap().write("new");
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len <= MAX_LEN / 2 + 100, "{len}");
        let lines = lines(&path);
        assert!(lines[0].starts_with("old line "), "{}", lines[0]);
        assert!(lines.last().unwrap().ends_with("new"));
    }

    #[test]
    fn the_time_is_the_utc_date_and_time() {
        let at = |secs| utc_time(UNIX_EPOCH + std::time::Duration::from_secs(secs));
        assert_eq!(at(0), "1970-01-01 00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29 00:00:00Z");
        assert_eq!(at(1_794_526_200 + 3_725), "2026-11-13 00:32:05Z");
    }

    #[test]
    fn the_start_screen_does_not_list_the_log() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "line\n").unwrap();
        assert!(crate::projects::list(dir.path()).is_empty());
    }
}
