#![feature(test)]
extern crate test;
use test::Bencher;

use lopdf;
use lopdf::Object;

use chrono::prelude::{Local, Timelike};

#[bench]
fn create_and_parse_datetime(b: &mut Bencher) {
    b.iter(|| {
        let time = Local::now().with_nanosecond(0).unwrap();
        let text: Object = time.into();
        let time2 = text.as_datetime();
        assert_eq!(time2, Some(time));
    });
}

// new (with itoa):       4,660 ns/iter (+/- 121)
// old (with formatting): 4,899 ns/iter (+/- 3,581)
#[bench]
fn bench_integer_write(b: &mut test::Bencher) {
    b.iter(|| {
        let mut buf = ::std::io::Cursor::new(Vec::<u8>::new());
        let mut doc = lopdf::Document::new();
        doc.add_object(Object::Integer(5));
        doc.save_to(&mut buf).unwrap();
    })
}

// new (with dtoa):       4,801 ns/iter (+/- 183)
// old (with formatting): 5,007 ns/iter (+/- 211)
#[bench]
fn bench_floating_point_write(b: &mut test::Bencher) {
    b.iter(|| {
        let mut buf = ::std::io::Cursor::new(Vec::<u8>::new());
        let mut doc = lopdf::Document::new();
        doc.add_object(Object::Real(5.0));
        doc.save_to(&mut buf).unwrap();
    })
}

// new (with true / false): 4,547 ns/iter (+/- 70)
// old (with formatting):   4,598 ns/iter (+/- 194)
#[bench]
fn bench_boolean_write(b: &mut test::Bencher) {
    b.iter(|| {
        let mut buf = ::std::io::Cursor::new(Vec::<u8>::new());
        let mut doc = lopdf::Document::new();
        doc.add_object(Object::Boolean(false));
        doc.save_to(&mut buf).unwrap();
    })
}
