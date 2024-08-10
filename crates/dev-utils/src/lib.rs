use std::fmt::{Debug, Display};
use std::ops::{Add, Sub};
use std::path::Path;

use tracing_subscriber::fmt::format::FmtSpan;

static mut GUARD: Option<tracing_chrome::FlushGuard> = None;

pub fn setup_test_log() {
    color_backtrace::install();
    use tracing_chrome::ChromeLayerBuilder;
    use tracing_subscriber::{prelude::*, registry::Registry};
    if option_env!("DEBUG").is_some() {
        // suffix should be current date time
        let time_suffix = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();
        // create dir if not exists
        std::fs::create_dir_all("./log").unwrap();
        let (chrome_layer, _guard) = ChromeLayerBuilder::new()
            .include_args(true)
            .include_locations(true)
            .file(Path::new(
                format!("./log/trace-{}.json", time_suffix).as_str(),
            ))
            .build();
        // SAFETY: Test
        unsafe { GUARD = Some(_guard) };
        tracing::subscriber::set_global_default(
            Registry::default()
                // .with(
                //     HierarchicalLayer::new(4)
                //         .with_indent_lines(true)
                //         .with_targets(targets)
                // )
                .with(
                    tracing_subscriber::fmt::Layer::default()
                        .with_span_events(FmtSpan::NEW)
                        .without_time()
                        .with_line_number(true)
                        .with_target(false)
                        .with_file(true),
                )
                .with(chrome_layer),
        )
        .unwrap();
    }
}

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counter;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ret = System.alloc(layout);
        if !ret.is_null() {
            ALLOCATED.fetch_add(layout.size(), Relaxed);
        }
        ret
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        ALLOCATED.fetch_sub(layout.size(), Relaxed);
    }
}

#[global_allocator]
static A: Counter = Counter;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteSize(pub usize);

impl Debug for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (size, unit) = match self.0 {
            bytes if bytes < 1024 => (bytes as f64, "B"),
            bytes if bytes < 1024 * 1024 => (bytes as f64 / 1024.0, "KB"),
            bytes if bytes < 1024 * 1024 * 1024 => (bytes as f64 / (1024.0 * 1024.0), "MB"),
            bytes => (bytes as f64 / (1024.0 * 1024.0 * 1024.0), "GB"),
        };
        write!(f, "{:.2} {}", size, unit)
    }
}

impl Display for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (size, unit) = match self.0 {
            bytes if bytes < 1024 => (bytes as f64, "B"),
            bytes if bytes < 1024 * 1024 => (bytes as f64 / 1024.0, "KB"),
            bytes if bytes < 1024 * 1024 * 1024 => (bytes as f64 / (1024.0 * 1024.0), "MB"),
            bytes => (bytes as f64 / (1024.0 * 1024.0 * 1024.0), "GB"),
        };
        write!(f, "{:.2} {}", size, unit)
    }
}

impl Add for ByteSize {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        ByteSize(self.0 + rhs.0)
    }
}

impl Sub for ByteSize {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        ByteSize(self.0 - rhs.0)
    }
}

pub fn get_mem_usage() -> ByteSize {
    ByteSize(ALLOCATED.load(Relaxed))
}
