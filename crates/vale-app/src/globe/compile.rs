//! Shader compiles on worker threads. A Metal pipeline compile can take
//! seconds on the first start of a new build, and iOS ends an app whose
//! main thread does not answer, so no frame waits for one.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui_wgpu::wgpu;

/// The count of the compiles that run.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// The messages of the compiles that failed.
static FAILED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// True: `Compiled::get` waits for the worker. Only offscreen renders set
/// it, so that each of their frames draws all passes.
static WAIT: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The count of the render pipelines that this thread created.
    static CREATED: Cell<u64> = const { Cell::new(0) };
}

/// Makes each frame wait for the compiles that it needs. Only offscreen
/// renders call it.
pub fn wait_in_frames() {
    WAIT.store(true, Ordering::Relaxed);
}

/// The messages of the compiles that failed, such as a pipeline that the
/// GPU does not take.
pub fn failures() -> Vec<String> {
    FAILED.lock().map(|f| f.clone()).unwrap_or_default()
}

/// True while a worker compiles.
pub fn compiling() -> bool {
    RUNNING.load(Ordering::Relaxed) > 0
}

/// The count of the render pipelines that the calling thread created with
/// `render_pipeline`.
pub fn pipelines_on_this_thread() -> u64 {
    CREATED.with(Cell::get)
}

/// Creates a render pipeline, and counts it for the calling thread.
pub fn render_pipeline(
    device: &wgpu::Device,
    descriptor: &wgpu::RenderPipelineDescriptor<'_>,
) -> wgpu::RenderPipeline {
    CREATED.with(|count| count.set(count.get() + 1));
    device.create_render_pipeline(descriptor)
}

type Make<T> = Box<dyn FnOnce() -> T + Send>;

/// A value that a worker thread makes, such as a set of pipelines. The
/// worker starts at the first `get`, or at once with `start`.
pub struct Compiled<T> {
    /// `None`: the worker panicked.
    cell: Arc<OnceLock<Option<T>>>,
    make: Mutex<Option<Make<T>>>,
}

impl<T: Send + Sync + 'static> Compiled<T> {
    /// A value that the worker makes when a view first needs it.
    pub fn lazy(make: impl FnOnce() -> T + Send + 'static) -> Compiled<T> {
        Compiled {
            cell: Arc::new(OnceLock::new()),
            make: Mutex::new(Some(Box::new(make))),
        }
    }

    /// A value that the worker starts to make now.
    pub fn start(make: impl FnOnce() -> T + Send + 'static) -> Compiled<T> {
        let compiled = Compiled::lazy(make);
        compiled.spawn();
        compiled
    }

    fn spawn(&self) {
        let Some(make) = self.make.lock().expect("no panic holds the lock").take() else {
            return;
        };
        let cell = self.cell.clone();
        RUNNING.fetch_add(1, Ordering::Relaxed);
        let run = move || {
            let value = catch_unwind(AssertUnwindSafe(make))
                .map_err(|panic| {
                    let text = (panic.downcast_ref::<String>().cloned())
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "no message".to_string());
                    if let Ok(mut failed) = FAILED.lock() {
                        failed.push(text);
                    }
                })
                .ok();
            let _ = cell.set(value);
            RUNNING.fetch_sub(1, Ordering::Relaxed);
        };
        std::thread::Builder::new()
            .name("vale-compile".to_string())
            .spawn(run)
            .expect("the system can start a thread");
    }

    /// The value, or `None` while the worker makes it. It starts the
    /// worker. It waits only after `wait_in_frames`.
    pub fn get(&self) -> Option<&T> {
        if WAIT.load(Ordering::Relaxed) {
            return self.wait();
        }
        self.spawn();
        self.cell.get()?.as_ref()
    }

    /// The value. It starts the worker and waits for it.
    pub fn wait(&self) -> Option<&T> {
        self.spawn();
        self.cell.wait().as_ref()
    }

    /// True when the worker ended, with a value or with a panic.
    pub fn done(&self) -> bool {
        self.cell.get().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worker_makes_the_value() {
        let here = std::thread::current().id();
        let compiled = Compiled::lazy(|| std::thread::current().id());
        assert!(!compiled.done());
        assert_ne!(compiled.wait(), Some(&here));
        assert!(compiled.done());
    }

    #[test]
    fn a_panic_of_the_worker_gives_no_value_and_a_message() {
        let compiled: Compiled<u32> = Compiled::start(|| panic!("no compile"));
        assert_eq!(compiled.wait(), None);
        assert!(compiled.done());
        assert!(failures().iter().any(|text| text == "no compile"));
    }
}
