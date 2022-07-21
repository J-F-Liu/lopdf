#![feature(test)]
use std::fs::File;
use std::io::{Cursor, Read};

extern crate test;
use lopdf::Document;

#[bench]
fn bench_load(b: &mut test::test::Bencher) {
    let mut buffer = Vec::new();
    File::open("assets/example.pdf")
        .unwrap()
        .read_to_end(&mut buffer)
        .unwrap();

    b.iter(|| {
        Document::load_from(Cursor::new(&buffer)).unwrap();
    })
}

#[bench]
fn bench_load_incremental_pdf(b: &mut test::test::Bencher) {
    let mut buffer = Vec::new();
    File::open("assets/Incremental.pdf")
        .unwrap()
        .read_to_end(&mut buffer)
        .unwrap();

    b.iter(|| {
        Document::load_from(Cursor::new(&buffer)).unwrap();
    })
}
