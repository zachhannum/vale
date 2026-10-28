//! A 16384 × 8192 image loads into a tile pyramid in a small amount of
//! memory.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs::File;
use std::io::BufWriter;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use vale_import::pyramid::{self, PyramidBuilder};
use vale_import::rgba::RowReader;

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(size: usize) {
    let now = CURRENT.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            grow(new_size - layout.size());
        } else {
            CURRENT.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const MIB: usize = 1 << 20;

fn reset_peak() {
    PEAK.store(CURRENT.load(Ordering::Relaxed), Ordering::Relaxed);
}

#[test]
fn large_png_loads_in_small_memory() {
    let (w, h) = (16384u32, 8192u32);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("world.png");

    let start = Instant::now();
    reset_peak();
    {
        let file = BufWriter::new(File::create(&path).unwrap());
        let mut enc = png::Encoder::new(file, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut stream = enc.write_header().unwrap().into_stream_writer().unwrap();
        let mut row = vec![0u8; w as usize * 4];
        for y in 0..h {
            for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                px.copy_from_slice(&[x as u8, y as u8, (x >> 6) as u8, (y >> 5) as u8]);
            }
            std::io::Write::write_all(&mut stream, &row).unwrap();
        }
        stream.finish().unwrap();
    }
    let write_peak = PEAK.load(Ordering::Relaxed);
    let written = start.elapsed();

    let start = Instant::now();
    reset_peak();
    let mut tiles = vec![0u32; pyramid::levels(w, h) as usize];
    let mut area = vec![0u64; tiles.len()];
    let mut checksum = 0u64;
    {
        let mut reader = RowReader::open(&path).unwrap();
        assert_eq!((reader.width(), reader.height()), (w, h));
        assert!(reader.has_alpha());
        let mut builder = PyramidBuilder::new(w, h, |level, _, _, tw, th, rgba: &[u8]| {
            tiles[level as usize] += 1;
            area[level as usize] += tw as u64 * th as u64;
            checksum += rgba[0] as u64;
        });
        let mut strip = Vec::new();
        loop {
            let n = reader.next_strip(256, &mut strip).unwrap();
            if n == 0 {
                break;
            }
            builder.push_strip(&strip, n);
        }
        builder.finish();
    }
    let read_peak = PEAK.load(Ordering::Relaxed);
    eprintln!(
        "write: {:.1} MiB in {:.1?}; read and pyramid: {:.1} MiB in {:.1?}; checksum {checksum}",
        write_peak as f64 / MIB as f64,
        written,
        read_peak as f64 / MIB as f64,
        start.elapsed(),
    );

    assert_eq!(tiles.len(), 7);
    assert_eq!(tiles[0], 64 * 32);
    assert_eq!(tiles[6], 1);
    for (level, &a) in area.iter().enumerate() {
        let (lw, lh) = pyramid::level_size(w, h, level as u32);
        assert_eq!(a, lw as u64 * lh as u64);
    }
    assert!(write_peak < 128 * MIB, "write peak {write_peak}");
    assert!(read_peak < 128 * MIB, "read peak {read_peak}");
}
