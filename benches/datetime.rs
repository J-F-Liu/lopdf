#![feature(test)]
extern crate test;
use test::Bencher;

extern crate lopdf;
use lopdf::Object;

extern crate chrono;
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
