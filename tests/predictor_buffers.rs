use flate2::{Compression, write::ZlibEncoder};
use lopdf::{DecompressError, Error, Stream, dictionary, filters::png};
use std::io::Write;

fn png_stream(columns: i64, decoded: &[u8], lzw: bool) -> Stream {
    let payload = if lzw {
        weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
            .encode(decoded)
            .unwrap()
    } else {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(decoded).unwrap();
        encoder.finish().unwrap()
    };
    Stream::new(
        dictionary! {
            "Filter" => if lzw { "LZWDecode" } else { "FlateDecode" },
            "DecodeParms" => dictionary! { "Predictor" => 12, "Columns" => columns, "Colors" => 1, "BitsPerComponent" => 8 },
        },
        payload,
    )
}

#[test]
fn bounded_png_predictor_rejects_oversized_rows_before_allocation() {
    for lzw in [false, true] {
        let result = png_stream(1 << 20, &[0, 1, 2, 3], lzw).decompressed_content_with_limit(4096);
        assert!(
            matches!(
                result,
                Err(Error::Decompress(DecompressError::MemoryLimitExceeded { limit: 4096 }))
            ),
            "lzw={lzw}: got {result:?}"
        );
    }
}

#[test]
fn png_empty_input_needs_no_row_buffers() {
    assert_eq!(png::decode_frame(&[], 1, usize::MAX).unwrap(), Vec::<u8>::new());
}

#[test]
fn empty_png_stream_fits_zero_output_limit() {
    for lzw in [false, true] {
        assert_eq!(
            png_stream(1 << 20, &[], lzw)
                .decompressed_content_with_limit(0)
                .unwrap(),
            Vec::<u8>::new()
        );
    }
}

#[test]
fn png_truncated_row_is_rejected_before_allocation() {
    assert_eq!(
        png::decode_frame(&[0], 1, usize::MAX).unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn png_rows_at_the_limit_still_decode() {
    for lzw in [false, true] {
        assert_eq!(
            png_stream(3, &[0, 10, 20, 30], lzw)
                .decompressed_content_with_limit(4)
                .unwrap(),
            vec![10, 20, 30]
        );
        assert_eq!(
            png_stream(2, &[0, 10, 20, 2, 1, 2], lzw)
                .decompressed_content_with_limit(6)
                .unwrap(),
            vec![10, 20, 11, 22]
        );
    }
}
