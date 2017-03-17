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
				.value_name("page numbers")
				.help("e.g. 3,5,7-9")
				.takes_value(true)))
		.subcommand(SubCommand::with_name("prune_objects")
			.about("Prune unused objects"))
		.subcommand(SubCommand::with_name("delete_objects")
			.about("Delete objects")
			.arg(Arg::with_name("ids")
				.value_name("object ids")
				.help("e.g. \"1 0,2 1,35,36\"")
				.takes_value(true)))
		.subcommand(SubCommand::with_name("renumber_objects")
			.about("Renumber objects"))
		.subcommand(SubCommand::with_name("prune_renumber_objects")
			.about("Prune unused objects and renumber objects"))
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
				"prune_objects" => {
					let ids = doc.prune_objects();
					println!("Deleted {:?}", ids);
					let streams = doc.delete_zero_length_streams();
					println!("Deleted zero length streams {:?}", streams);
				}
				"delete_objects" => {
					if let Some(ids) = args.value_of("ids") {
						for id in ids.split(',') {
							let nums: Vec<u32> = id.split(' ').map(|num|u32::from_str(num).unwrap()).collect();
							match nums.len() {
								1 => doc.delete_object(&(nums[0], 0)),
								2 => doc.delete_object(&(nums[0], nums[1] as u16)),
								_ => None
							};
						}
					}
				}
				"renumber_objects" => doc.renumber_objects(),
				"prune_renumber_objects" => {
					let ids = doc.prune_objects();
					println!("Deleted {:?}", ids);
					let streams = doc.delete_zero_length_streams();
					if streams.len() > 0 {
						println!("Deleted zero length streams {:?}", streams);
					}
					doc.renumber_objects();
				}
				_ => {}
			}

			doc.change_producer("https://crates.io/crates/lopdf");

			if let Some(output) = args.value_of("output") {
				println!("Save to {}", output);
				doc.save(output).unwrap();
			}
		}
	}
}
