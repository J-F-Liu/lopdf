//! Memory profiler for lopdf load / clone / save / extract-text paths.
//!
//! Run with:
//!   cargo run --release --example memory_profile -- [pdf ...]
//! With no arguments it profiles the PDFs under `assets/` that exist.
//!
//! It installs a counting global allocator and reports, per phase:
//! live heap bytes, peak heap bytes since the phase started, allocation
//! count, and a structural census of the loaded `Document` (objects,
//! streams, dictionary entries, stream bytes, ...). Numbers are process
//! heap bytes tracked by the allocator, not RSS: they exclude allocator
//! fragmentation, rayon worker stacks, and the file buffer the caller
//! may still hold, which is exactly why that buffer is reported too.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use lopdf::{Document, Object};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

fn bump_peak(live: usize) {
    let mut peak = PEAK.load(Ordering::Relaxed);
    while live > peak {
        match PEAK.compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(next) => peak = next,
        }
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            bump_peak(live);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                let live = LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed) + (new_size - layout.size());
                bump_peak(live);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
            ALLOC_BYTES.fetch_add(new_size, Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        new_ptr
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

struct Mark {
    live: usize,
    allocs: usize,
    alloc_bytes: usize,
}

fn mark() -> Mark {
    // Peak is measured from the start of each phase.
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
    Mark {
        live: LIVE.load(Ordering::Relaxed),
        allocs: ALLOCS.load(Ordering::Relaxed),
        alloc_bytes: ALLOC_BYTES.load(Ordering::Relaxed),
    }
}

fn report(phase: &str, from: &Mark) {
    let live = LIVE.load(Ordering::Relaxed);
    let peak = PEAK.load(Ordering::Relaxed);
    println!(
        "  {phase:<28} live={} ({:+})  peak_since_phase_start={}  allocs={}  alloc_bytes={}",
        mib(live),
        live as i64 - from.live as i64,
        mib(peak),
        ALLOCS.load(Ordering::Relaxed) - from.allocs,
        ALLOC_BYTES.load(Ordering::Relaxed) - from.alloc_bytes,
    );
}

fn mib(bytes: usize) -> String {
    format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
}

#[derive(Default)]
struct Census {
    objects: usize,
    streams: usize,
    stream_bytes: usize,
    stream_capacity: usize,
    dicts: usize,
    dict_entries: usize,
    dict_key_bytes: usize,
    arrays: usize,
    array_items: usize,
    names: usize,
    name_bytes: usize,
    strings: usize,
    string_bytes: usize,
}

fn census_object(object: &Object, census: &mut Census) {
    match object {
        Object::Name(bytes) => {
            census.names += 1;
            census.name_bytes += bytes.len();
        }
        Object::String(bytes, _) => {
            census.strings += 1;
            census.string_bytes += bytes.len();
        }
        Object::Array(items) => {
            census.arrays += 1;
            census.array_items += items.len();
            for item in items {
                census_object(item, census);
            }
        }
        Object::Dictionary(dict) => {
            census.dicts += 1;
            census.dict_entries += dict.len();
            for (key, value) in dict.iter() {
                census.dict_key_bytes += key.len();
                census_object(value, census);
            }
        }
        Object::Stream(stream) => {
            census.streams += 1;
            census.stream_bytes += stream.content.len();
            census.stream_capacity += stream.content.capacity();
            census.dicts += 1;
            census.dict_entries += stream.dict.len();
            for (key, value) in stream.dict.iter() {
                census.dict_key_bytes += key.len();
                census_object(value, census);
            }
        }
        _ => {}
    }
}

fn profile(path: &str) {
    let file_bytes = std::fs::metadata(path).map(|m| m.len() as usize).unwrap_or(0);
    println!("\n== {path} (file {}) ==", mib(file_bytes));

    let m = mark();
    // Read + load_mem (instead of Document::load) so this example also builds
    // with the `async` feature, where `load` is async. The file buffer is
    // dropped at the end of this block, before retained heap is reported.
    let mut document = match std::fs::read(path).map_err(lopdf::Error::from).and_then(|bytes| {
        let document = Document::load_mem(&bytes);
        drop(bytes);
        document
    }) {
        Ok(document) => document,
        Err(error) => {
            println!("  load failed: {error}");
            return;
        }
    };
    report("load (buffer dropped)", &m);
    println!(
        "     note: peak includes the whole-file buffer ({}) held during parsing; live does not.",
        mib(file_bytes)
    );

    let mut census = Census {
        objects: document.objects.len(),
        ..Census::default()
    };
    for object in document.objects.values() {
        census_object(object, &mut census);
    }
    // Include the trailer dictionary in the dict census.
    census.dicts += 1;
    census.dict_entries += document.trailer.len();
    println!(
        "     objects={} streams={} stream_content={} (capacity {}) dicts={} dict_entries={} dict_key_bytes={} arrays={}/{} names={}/{} strings={}/{} xref_entries={} pages={}",
        census.objects,
        census.streams,
        mib(census.stream_bytes),
        mib(census.stream_capacity),
        census.dicts,
        census.dict_entries,
        census.dict_key_bytes,
        census.arrays,
        census.array_items,
        census.names,
        census.name_bytes,
        census.strings,
        census.string_bytes,
        document.reference_table.entries.len(),
        document.get_pages().len(),
    );

    let m = mark();
    let cloned = document.clone();
    report("Document::clone", &m);
    drop(cloned);

    let pages: Vec<u32> = document.get_pages().keys().copied().collect();
    if !pages.is_empty() {
        let m = mark();
        match document.extract_text(&pages) {
            Ok(text) => {
                report("extract_text(all pages)", &m);
                println!("     extracted_text={}", mib(text.len()));
            }
            Err(error) => println!("  extract_text failed: {error}"),
        }
        let first = pages[0];
        let m = mark();
        let content = document.get_page_content(document.get_pages()[&first]);
        report("get_page_content(page 1)", &m);
        println!("     page_content={}", mib(content.len()));
    }

    let m = mark();
    let mut out: Vec<u8> = Vec::new();
    match document.save_to(&mut out) {
        Ok(()) => {
            report("save_to(Vec)", &m);
            println!("     saved_output={} (held in the Vec above)", mib(out.len()));
        }
        Err(error) => println!("  save_to failed: {error}"),
    }
    drop(out);

    let m = mark();
    let mut modern_out: Vec<u8> = Vec::new();
    match document.save_modern(&mut modern_out) {
        Ok(()) => {
            report("save_modern(Vec)", &m);
            println!("     saved_output={} (held in the Vec above)", mib(modern_out.len()));
        }
        Err(error) => println!("  save_modern failed: {error}"),
    }
    drop(modern_out);

    let m = mark();
    document.decompress();
    report("Document::decompress()", &m);
    let total: usize = document
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .map(|s| s.content.len())
        .sum();
    println!("     stream_content_after_decompress={}", mib(total));
}

fn main() {
    println!(
        "type sizes: Object={} B, Stream={} B, Dictionary={} B, ObjectId={} B",
        std::mem::size_of::<Object>(),
        std::mem::size_of::<lopdf::Stream>(),
        std::mem::size_of::<lopdf::Dictionary>(),
        std::mem::size_of::<lopdf::ObjectId>(),
    );

    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths: Vec<String> = if args.is_empty() {
        ["assets/example.pdf", "assets/AnnotationDemo.pdf", "assets/test.pdf"]
            .iter()
            .filter(|p| std::path::Path::new(p).exists())
            .map(|p| p.to_string())
            .collect()
    } else {
        args
    };

    for path in &paths {
        profile(path);
    }
}
