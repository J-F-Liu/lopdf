#![feature(test)]
extern crate test;
use test::Bencher;

use lopdf::Object;
use chrono::prelude::{Local, Timelike};

#[bench]
fn create_and_parse_datetime(b: &mut Bencher) {
    b.iter(|| {
        let time = Local::now().with_nanosecond(0).unwrap();
        let text: Object = time.into();
        let time2 = text.as_datetime();
        assert!(time2.is_some());
    });
}

#[bench]
fn bench_integer_write(b: &mut test::Bencher) {
    b.iter(|| {
        let mut buf = ::std::io::Cursor::new(Vec::<u8>::new());
        let mut doc = lopdf::Document::new();
        doc.add_object(Object::Integer(5));
        doc.save_to(&mut buf).unwrap();
    })
}

#[bench]
fn bench_floating_point_write(b: &mut test::Bencher) {
    b.iter(|| {
        let mut buf = ::std::io::Cursor::new(Vec::<u8>::new());
        let mut doc = lopdf::Document::new();
        doc.add_object(Object::Real(5.0));
        doc.save_to(&mut buf).unwrap();
    })
}

#[bench]
fn bench_boolean_write(b: &mut test::Bencher) {
    b.iter(|| {
        let mut buf = ::std::io::Cursor::new(Vec::<u8>::new());
        let mut doc = lopdf::Document::new();
        doc.add_object(Object::Boolean(false));
        doc.save_to(&mut buf).unwrap();
    })
}