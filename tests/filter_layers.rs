//! A stream is decoded only if it names at most `Stream::MAX_FILTERS` filters. The size limit
//! bounds each filter's output, not the number of filters, so without this a small stream could
//! make the decoder produce the limit's worth of bytes again for every filter it names.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;

use lopdf::{DecompressError, Dictionary, Error, Object, ObjectStream, Stream};

/// A zlib stream of stored blocks that decodes to `data`, as large as `data` itself.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = data.chunks(usize::from(u16::MAX)).collect();
    for (index, chunk) in chunks.iter().enumerate() {
        out.push(u8::from(index + 1 == chunks.len()));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// A stream with `filters` `FlateDecode` filters that decodes to `data`. Every filter but the
/// first produces about as many bytes as `data`, and the content is far smaller than that.
fn layered_stream(data: &[u8], filters: usize) -> Stream {
    let mut layered = data.to_vec();
    for _ in 1..filters {
        layered = zlib_stored(&layered);
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&layered).unwrap();
    let mut dict = Dictionary::new();
    dict.set("Filter", vec![Object::from("FlateDecode"); filters]);
    Stream::new(dict, encoder.finish().unwrap())
}

fn assert_too_many_filters<T: std::fmt::Debug>(result: lopdf::Result<T>, count: usize) {
    match result {
        Err(Error::Decompress(DecompressError::TooManyFilters { count: reported, limit })) => {
            assert_eq!(reported, count);
            assert_eq!(limit, Stream::MAX_FILTERS);
        }
        other => panic!("expected TooManyFilters, got {other:?}"),
    }
}

#[test]
fn a_stream_with_the_most_filters_decodes() {
    let data = vec![7u8; 100_000];
    let stream = layered_stream(&data, Stream::MAX_FILTERS);
    assert!(stream.content.len() < 2_000, "{} bytes", stream.content.len());

    assert_eq!(stream.decompressed_content().unwrap(), data);
    assert_eq!(
        stream.decompressed_content_with_limit(data.len() + 1_000).unwrap(),
        data
    );
    assert_eq!(stream.get_plain_content().unwrap(), data);
}

#[test]
fn a_stream_with_more_filters_is_not_decoded() {
    let count = Stream::MAX_FILTERS + 1;
    let stream = layered_stream(&[7u8; 1_000], count);

    assert_too_many_filters(stream.decompressed_content(), count);
    assert_too_many_filters(stream.decompressed_content_with_limit(1 << 20), count);
    assert_too_many_filters(stream.get_plain_content(), count);
    assert_too_many_filters(stream.get_plain_content_with_limit(1 << 20), count);
    assert_too_many_filters(stream.decompress_to_writer(&mut Vec::new(), 1 << 20), count);

    let mut decompressed = stream.clone();
    assert_too_many_filters(decompressed.decompress(), count);
    assert_eq!(decompressed, stream, "a stream that was not decoded stays as it was");
}

#[test]
fn many_filters_are_rejected_before_any_is_decoded() {
    // Content that no filter can decode: the bound is checked before the first filter runs, so
    // the error is the bound and not a decoding error.
    let count = 100_000;
    let mut dict = Dictionary::new();
    dict.set("Filter", vec![Object::from("ASCIIHexDecode"); count]);
    let stream = Stream::new(dict, b"not hex".to_vec());
    assert_too_many_filters(stream.decompressed_content(), count);
}

#[test]
fn an_object_stream_with_more_filters_is_not_decoded() {
    let mut stream = layered_stream(b"1 0 42", Stream::MAX_FILTERS + 1);
    stream.dict.set("Type", "ObjStm");
    stream.dict.set("N", 1i64);
    stream.dict.set("First", 4i64);
    assert_too_many_filters(ObjectStream::new_with_limit(&stream, None), Stream::MAX_FILTERS + 1);
}
