extern crate lopdf;
use lopdf::{Document};

#[macro_use]
extern crate clap;
use clap::{App, Arg};

fn main() {
	let arguments = App::new("PDF utility program using lopdf library")
		.version(crate_version!())
		.author(crate_authors!())
		.arg(Arg::with_name("input")
			.short("i")
			.long("input")
			.value_name("input file")
			.takes_value(true)
			.required(true))
		.arg(Arg::with_name("output")
			.short("o")
			.long("output")
			.value_name("output file")
			.takes_value(true))
		.get_matches();

	if let Some(input) = arguments.value_of("input") {
		let mut doc = Document::load(input).unwrap();

		if let Some(output) = arguments.value_of("output") {
			doc.save(output).unwrap();
		}
	}
}
