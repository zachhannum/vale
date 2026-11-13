//! The log file of the app. On iOS, it is in the Documents folder, and the
//! Files app shows it as "On My iPhone > Vale > vale-log.txt". It holds the
//! warnings and the errors of the `log` crate, the panics, and the normal
//! exit. It also holds the actions of the app and the memory at each one.
//! On iOS, it holds the memory use and the changes between the foreground
//! and the background.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use vale_store::PageId;

use crate::ui::{AppState, Screen, Workspace};

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
            let repeats = match inner.repeats {
                1 => "(and 1 more time)".to_string(),
                n => format!("(and {n} more times)"),
            };
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

/// The version of the app. On iOS, it is the version and the build number of
/// the bundle, for example `0.1.456 (640.1)`.
fn version() -> String {
    #[cfg(target_os = "ios")]
    if let Some(version) = ios::version() {
        return version;
    }
    env!("CARGO_PKG_VERSION").to_string()
}

/// The first line of each run, for example `start, version 0.1.456 (640.1),
/// memory limit 3375 MB`.
fn start_line(version: &str, limit: Option<u64>) -> String {
    match limit {
        Some(bytes) => format!("start, version {version}, memory limit {} MB", bytes >> 20),
        None => format!("start, version {version}"),
    }
}

/// The footprint at which iOS stops the app: the footprint now plus the
/// memory that the system still gives the app. `None` off iOS.
fn memory_limit() -> Option<u64> {
    #[cfg(target_os = "ios")]
    if let (Some(footprint), Some(available)) = (ios::footprint(), crate::memory::available()) {
        return Some(footprint + available);
    }
    None
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

static HEAP: AtomicUsize = AtomicUsize::new(0);

/// The system allocator, with a count of the bytes that it holds.
pub struct CountingAllocator;

// SAFETY: each method passes its arguments to the system allocator, and only
// counts the result.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller keeps the contract of `alloc`.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            HEAP.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller keeps the contract of `alloc_zeroed`.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            HEAP.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller keeps the contract of `dealloc`.
        unsafe { System.dealloc(ptr, layout) };
        HEAP.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller keeps the contract of `realloc`.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            HEAP.fetch_add(new_size, Ordering::Relaxed);
            HEAP.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }
}

/// The bytes that `CountingAllocator` holds.
pub fn heap_bytes() -> usize {
    HEAP.load(Ordering::Relaxed)
}

/// The memory of the app, for example `memory 812 MB, gpu 530 MB, heap
/// 190 MB`. The footprint and the GPU memory are on iOS only.
pub fn memory() -> String {
    #[cfg_attr(not(target_os = "ios"), expect(unused_mut))]
    let mut text = String::new();
    #[cfg(target_os = "ios")]
    {
        if let Some(bytes) = ios::footprint() {
            text += &format!("memory {} MB, ", bytes >> 20);
        }
        if let Some(bytes) = ios::gpu_bytes() {
            text += &format!("gpu {} MB, ", bytes >> 20);
        }
    }
    text + &format!("heap {} MB", heap_bytes() >> 20)
}

/// The sizes of the caches of the app, which `Probe` sets.
static CACHES: Mutex<String> = Mutex::new(String::new());

#[cfg_attr(not(target_os = "ios"), expect(dead_code))]
fn caches() -> String {
    CACHES.lock().map_or(String::new(), |c| c.clone())
}

/// What the app shows, for the action lines of the log.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    screen: Screen,
    workspace: Workspace,
    project: Option<String>,
    page: PageId,
    /// The zoom of the page in half steps of 2: 2 is a zoom of 2.
    zoom: i32,
}

impl Seen {
    fn of(state: &AppState) -> Seen {
        let project = state.session.as_ref().map(|s| {
            let name = s.path().file_name().unwrap_or_default();
            name.to_string_lossy().into_owned()
        });
        Seen {
            screen: state.screen,
            workspace: state.workspace,
            project,
            page: state.atlas.page,
            zoom: (state.atlas.view.zoom.log2() * 2.0).round() as i32,
        }
    }

    /// The actions from `self` to `now`.
    fn changes(&self, now: &Seen) -> Vec<String> {
        let mut out = Vec::new();
        if now.screen != self.screen {
            out.push(format!("screen {:?}", now.screen));
        }
        if now.project != self.project {
            let name = now.project.as_deref().unwrap_or("none");
            out.push(format!("project {name}"));
        }
        if now.workspace != self.workspace {
            out.push(format!("workspace {:?}", now.workspace));
        }
        if now.workspace == Workspace::Atlas {
            if now.page != self.page {
                out.push(format!("page {}", now.page.0));
            }
            if now.zoom != self.zoom {
                out.push(format!("zoom {:.1}x", 2f64.powf(f64::from(now.zoom) / 2.0)));
            }
        }
        out
    }
}

/// Writes the actions of the app to the log, each with the memory, and keeps
/// the sizes of the caches for the memory lines.
#[derive(Default)]
pub struct Probe {
    seen: Option<Seen>,
    caches_at: Option<Instant>,
}

impl Probe {
    /// Call it one time in each pass of the UI.
    pub fn update(&mut self, state: &AppState) {
        if LOG.get().is_none() {
            return;
        }
        let now = Seen::of(state);
        if let Some(seen) = &self.seen {
            for change in seen.changes(&now) {
                write(&format!("{change}; {}", memory()));
            }
        }
        self.seen = Some(now);
        if self
            .caches_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
        {
            let text = format!(
                "tiles {}, details {} MB, masks {}, projections {}",
                state.atlas.tile_count(),
                state.globe.detail_bytes() >> 20,
                state.atlas.mask_count(),
                state.pipeline.cache_count()
            );
            if let Ok(mut caches) = CACHES.lock() {
                *caches = text;
            }
            self.caches_at = Some(Instant::now());
        }
    }
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
    log.write(&start_line(&version(), memory_limit()));
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
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSBundle, NSString, ns_string};

    use super::{caches, memory, write};

    pub fn version() -> Option<String> {
        let bundle = NSBundle::mainBundle();
        let key = |key: &NSString| {
            let value = bundle.objectForInfoDictionaryKey(key)?;
            Some(value.downcast::<NSString>().ok()?.to_string())
        };
        let version = key(ns_string!("CFBundleShortVersionString"))?;
        Some(match key(ns_string!("CFBundleVersion")) {
            Some(build) => format!("{version} ({build})"),
            None => version,
        })
    }

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

    unsafe extern "C" {
        fn MTLCreateSystemDefaultDevice() -> *mut AnyObject;
    }

    /// The memory that the Metal device of the app holds. On iOS, the app
    /// has one device, and wgpu uses it.
    pub fn gpu_bytes() -> Option<u64> {
        // The address of the device, which stays for the life of the process.
        static DEVICE: OnceLock<usize> = OnceLock::new();
        // SAFETY: the function has no arguments, and it returns a retained
        // device or null. The device is never released.
        let device = *DEVICE.get_or_init(|| unsafe { MTLCreateSystemDefaultDevice() } as usize);
        let device = (device as *mut AnyObject).cast_const();
        // SAFETY: the device is live, and `currentAllocatedSize` returns an
        // `NSUInteger`. A device can be used from any thread.
        let device = unsafe { device.as_ref() }?;
        let bytes: usize = unsafe { msg_send![device, currentAllocatedSize] };
        Some(bytes as u64)
    }

    /// The memory of the app that counts toward the limit of iOS.
    pub fn footprint() -> Option<u64> {
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
                            write(&format!("{}; {}", memory(), caches()));
                            last = Some((bytes, Instant::now()));
                        }
                    }
                    std::thread::sleep(Duration::from_secs(1));
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
    fn the_start_line_has_the_memory_limit() {
        let line = start_line("0.1.0 (1.1)", Some(3375 << 20));
        assert_eq!(line, "start, version 0.1.0 (1.1), memory limit 3375 MB");
        assert_eq!(start_line("0.1.0", None), "start, version 0.1.0");
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
        assert!(lines[1].ends_with("(and 3 more times)"));
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
    fn the_counting_allocator_counts_the_bytes_that_it_holds() {
        let before = heap_bytes();
        let layout = Layout::from_size_align(1000, 8).unwrap();
        // SAFETY: the layout has a size, and each pointer goes back with
        // the layout that it came with.
        unsafe {
            let p = CountingAllocator.alloc(layout);
            assert_eq!(heap_bytes() - before, 1000);
            let p = CountingAllocator.realloc(p, layout, 3000);
            assert_eq!(heap_bytes() - before, 3000);
            let bigger = Layout::from_size_align(3000, 8).unwrap();
            CountingAllocator.dealloc(p, bigger);
        }
        assert_eq!(heap_bytes(), before);
    }

    fn seen() -> Seen {
        Seen {
            screen: Screen::Editor,
            workspace: Workspace::Globe,
            project: Some("World.gpkg".into()),
            page: PageId(1),
            zoom: 0,
        }
    }

    #[test]
    fn each_action_is_one_line() {
        let before = seen();
        assert!(before.changes(&before).is_empty());
        let atlas = Seen {
            workspace: Workspace::Atlas,
            ..before.clone()
        };
        assert_eq!(before.changes(&atlas), ["workspace Atlas"]);
        let zoomed = Seen {
            page: PageId(2),
            zoom: 3,
            ..atlas.clone()
        };
        assert_eq!(atlas.changes(&zoomed), ["page 2", "zoom 2.8x"]);
        let other = Seen {
            project: None,
            screen: Screen::Browser,
            ..before.clone()
        };
        assert_eq!(before.changes(&other), ["screen Browser", "project none"]);
    }

    #[test]
    fn the_page_and_the_zoom_count_only_in_the_atlas() {
        let before = seen();
        let now = Seen {
            page: PageId(2),
            zoom: 4,
            ..before.clone()
        };
        assert!(before.changes(&now).is_empty());
    }

    #[test]
    fn the_start_screen_does_not_list_the_log() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "line\n").unwrap();
        assert!(crate::projects::list(dir.path()).is_empty());
    }
}
