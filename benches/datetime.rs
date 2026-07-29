#[cfg(feature = "chrono-clock")]
use chrono::prelude::{Local, Timelike};
use criterion::{Criterion, criterion_group, criterion_main};
use lopdf::Object;

// Only the date benchmark needs a date backend, and only this one needs the
// system clock. Without the gate the whole bench target fails to compile
// under `--no-default-features`, which is why
// `cargo clippy --all-targets --no-default-features` does not build.
#[cfg(feature = "chrono-clock")]
fn create_and_parse_datetime(c: &mut Criterion) {
    c.bench_function("create_and_parse_datetime", |b| {
        b.iter(|| {
            let time = Local::now().with_nanosecond(0).unwrap();
            let text: Object = time.into();
            let time2 = text.as_datetime();
            assert!(time2.is_some());
        });
    });
}

#[cfg(not(feature = "chrono-clock"))]
fn create_and_parse_datetime(c: &mut Criterion) {
    // The parse half still measures without any backend, through the
    // accessor that needs none.
    c.bench_function("create_and_parse_datetime", |b| {
        b.iter(|| {
            let text = Object::string_literal("D:20260710143000+02'00'");
            let parsed = text.as_datetime();
            assert!(parsed.is_some());
        });
    });
}

fn integer_write(c: &mut Criterion) {
    c.bench_function("integer_write", |b| {
        b.iter(|| {
            let mut buf = std::io::Cursor::new(Vec::<u8>::new());
            let mut doc = lopdf::Document::new();
            doc.add_object(Object::Integer(5));
            doc.save_to(&mut buf).unwrap();
        });
    });
}

fn floating_point_write(c: &mut Criterion) {
    c.bench_function("floating_point_write", |b| {
        b.iter(|| {
            let mut buf = std::io::Cursor::new(Vec::<u8>::new());
            let mut doc = lopdf::Document::new();
            doc.add_object(Object::Real(5.0));
            doc.save_to(&mut buf).unwrap();
        });
    });
}

fn boolean_write(c: &mut Criterion) {
    c.bench_function("boolean_write", |b| {
        b.iter(|| {
            let mut buf = std::io::Cursor::new(Vec::<u8>::new());
            let mut doc = lopdf::Document::new();
            doc.add_object(Object::Boolean(false));
            doc.save_to(&mut buf).unwrap();
        });
    });
}

criterion_group!(
    benches,
    create_and_parse_datetime,
    integer_write,
    floating_point_write,
    boolean_write
);
criterion_main!(benches);
