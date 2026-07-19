//! Demonstrates bounding stream decompression to reject decompression bombs.
//!
//! Run with:  cargo run --example decompression_bomb
//!
//! A decompression bomb is a tiny compressed stream that inflates to an enormous
//! size. This example builds one and decodes it with a size limit, for a direct
//! stream, a chained-filter stream, and a bomb embedded in a PDF that is decoded
//! during `Document::load`.

use std::io::Write;
use std::time::Instant;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use lopdf::{Dictionary, Document, LoadOptions, Object, Stream};

const MIB: usize = 1024 * 1024;

/// A zlib stream that decodes to `target` zero-bytes, built by streaming zeros
/// through the compressor so this process never holds `target` bytes at once.
fn flate_bomb(target: usize) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    let zeros = [0u8; 64 * 1024];
    let mut remaining = target;
    while remaining > 0 {
        let n = remaining.min(zeros.len());
        encoder.write_all(&zeros[..n]).unwrap();
        remaining -= n;
    }
    encoder.finish().unwrap()
}

fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn main() {
    println!("== lopdf decompression-bomb guard ==\n");

    // ---- 1. A single-filter bomb -----------------------------------------
    let logical_output = 256 * MIB;
    let bomb = flate_bomb(logical_output);
    println!(
        "Built a FlateDecode bomb: {} bytes compressed -> {} bytes decompressed ({}x amplification).",
        bomb.len(),
        logical_output,
        logical_output / bomb.len().max(1)
    );

    let mut dict = Dictionary::new();
    dict.set("Filter", "FlateDecode");
    let stream = Stream::new(dict, bomb.clone());

    let limit = 8 * MIB;
    let start = Instant::now();
    match stream.decompressed_content_with_limit(limit) {
        Ok(data) => println!("  UNEXPECTED: decompressed {} bytes", data.len()),
        Err(e) => println!(
            "  decompressed_content_with_limit({} MiB) rejected it in {:?}: {e}",
            limit / MIB,
            start.elapsed()
        ),
    }
    println!(
        "  (The default decompressed_content() has NO limit and would allocate all {} MiB.)\n",
        logical_output / MIB
    );

    // ---- 2. A chained-filter bomb: [FlateDecode FlateDecode] -------------
    // Each layer amplifies ~1000x, so two layers reach ~1,000,000x. The guard
    // bounds every layer, so the bomb is caught at the second one.
    let inner = flate_bomb(256 * MIB);
    let outer = zlib_compress(&inner);
    let mut dict = Dictionary::new();
    dict.set("Filter", vec![Object::from("FlateDecode"), Object::from("FlateDecode")]);
    let chained = Stream::new(dict, outer.clone());
    println!(
        "Built a chained [FlateDecode FlateDecode] bomb: {} bytes -> ~256 MiB after two layers.",
        outer.len()
    );
    match chained.decompressed_content_with_limit(limit) {
        Ok(data) => println!("  UNEXPECTED: decompressed {} bytes", data.len()),
        Err(e) => println!("  rejected at a filter layer with an {} MiB limit: {e}\n", limit / MIB),
    }

    // ---- 3. A bomb inside a PDF's cross-reference stream ----------------
    // The xref stream is decoded during Document::load to build the xref
    // table, so the limit is supplied through LoadOptions.
    let pdf = xref_stream_bomb_pdf(&bomb);
    println!("Loading a PDF whose cross-reference stream is the bomb...");
    match Document::load_mem(&pdf) {
        Ok(_) => println!("  load_mem (no limit): parsed after decoding the full output"),
        Err(e) => println!("  load_mem (no limit): {e}"),
    }
    match Document::load_mem_with_options(&pdf, LoadOptions::with_max_decompressed_size(limit)) {
        Ok(_) => println!("  UNEXPECTED: guarded load succeeded"),
        Err(e) => println!(
            "  load_mem_with_options(max_decompressed_size = {} MiB): {e}",
            limit / MIB
        ),
    }
}

fn xref_stream_bomb_pdf(bomb: &[u8]) -> Vec<u8> {
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.5\n");
    let obj_offset = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n");
    pdf.extend_from_slice(
        format!(
            "<< /Type /XRef /Size 1 /W [1 1 1] /Root 1 0 R /Filter /FlateDecode /Length {} >>\n",
            bomb.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(b"stream\n");
    pdf.extend_from_slice(bomb);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    pdf.extend_from_slice(format!("startxref\n{obj_offset}\n%%EOF").as_bytes());
    pdf
}
