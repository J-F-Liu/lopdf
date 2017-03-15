extern crate lopdf;
use lopdf::Document;

#[macro_use]
extern crate clap;
use clap::{App, Arg, SubCommand};

use std::str::FromStr;

fn main() {
	let app = App::new("PDF utility program using lopdf library")
		.version(crate_version!())
		.author(crate_authors!())
		.arg(Arg::with_name("input")
			.short("i")
			.long("input")
			.value_name("input file")
			.takes_value(true)
			.global(true))
		.arg(Arg::with_name("output")
			.short("o")
			.long("output")
			.value_name("output file")
			.takes_value(true)
			.global(true))
		.subcommand(SubCommand::with_name("compress")
			.about("Compress PDF document"))
		.subcommand(SubCommand::with_name("decompress")
			.about("Decompress PDF document"))
		.subcommand(SubCommand::with_name("delete_pages")
			.about("Delete pages")
			.arg(Arg::with_name("pages")
				.long("pages")
				.value_name("page numbers")
				.takes_value(true)))
		.subcommand(SubCommand::with_name("delete_unused_objects")
			.about("Delete unused objects"))
		.subcommand(SubCommand::with_name("delete_objects")
			.about("Delete objects")
			.arg(Arg::with_name("ids")
				.long("ids")
				.value_name("object ids")
				.takes_value(true)))
		.subcommand(SubCommand::with_name("renumber_objects")
			.about("Renumber objects"))
		.get_matches();

	if let (cmd, Some(args)) = app.subcommand() {
		if let Some(input) = args.value_of("input") {

			println!("Open {}", input);
			let mut doc = Document::load(input).unwrap();

			println!("Do {}", cmd);
			match cmd {
				"compress" => doc.compress(),
				"decompress" => doc.decompress(),
				"delete_pages" => {
					if let Some(pages) = args.value_of("pages") {
						let mut page_numbers = vec![];
						for page in pages.split(',') {
							let nums: Vec<u32> = page.split('-').map(|num|u32::from_str(num).unwrap()).collect();
							match nums.len() {
								1 => page_numbers.push(nums[0]),
								2 => page_numbers.append(&mut (nums[0]..nums[1]+1).collect()),
								_ => {}
							}
						}
						doc.delete_pages(&page_numbers);
					}
				}
				"delete_unused_objects" => doc.delete_unused_objects(),
				"delete_objects" => {
					if let Some(ids) = args.value_of("ids") {
						for id in ids.split(',') {
							let nums: Vec<u32> = id.split(' ').map(|num|u32::from_str(num).unwrap()).collect();
							doc.delete_object(&(nums[0], nums[1] as u16));
						}
					}
				},
				"renumber_objects" => doc.renumber_objects(),
				_ => (),
			}

			doc.change_producer("https://crates.io/crates/lopdf");

			if let Some(output) = args.value_of("output") {
				println!("Save to {}", output);
				doc.save(output).unwrap();
			}
		}
	}
}
