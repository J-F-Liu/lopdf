use lopdf::Document;
use lopdf::Object;
use log::info;

#[macro_use]
extern crate clap;
use clap::{App, Arg, SubCommand};
use std::str::FromStr;

fn main() {
	env_logger::init();

	let app = App::new("PDF utility program using lopdf library")
		.version(crate_version!())
		.author(crate_authors!())
		.arg(Arg::with_name("input").short("i").long("input").value_name("input file").takes_value(true).global(true))
		.arg(Arg::with_name("output").short("o").long("output").value_name("output file").takes_value(true).global(true))
		.subcommand(
			SubCommand::with_name("process").about("Process PDF document with specified operations").arg(
				Arg::with_name("operations")
					.value_name("operations")
					.help("e.g. prune_objects delete_zero_length_streams renumber_objects")
					.takes_value(true)
					.multiple(true),
			),
		)
		.subcommand(SubCommand::with_name("compress").about("Compress PDF document"))
		.subcommand(SubCommand::with_name("decompress").about("Decompress PDF document"))
		.subcommand(
			SubCommand::with_name("delete_pages")
				.about("Delete pages")
				.arg(Arg::with_name("pages").value_name("page numbers").help("e.g. 3,5,7-9").takes_value(true)),
		)
		.subcommand(
			SubCommand::with_name("extract_pages")
				.about("Extract pages")
				.arg(Arg::with_name("pages").value_name("page numbers").help("e.g. 3,5,7-9").takes_value(true)),
		)
		.subcommand(SubCommand::with_name("prune_objects").about("Prune unused objects"))
		.subcommand(
			SubCommand::with_name("delete_objects")
				.about("Delete objects")
				.arg(Arg::with_name("ids").value_name("object ids").help("e.g. \"1 0,2 1,35,36\"").takes_value(true)),
		)
		.subcommand(
			SubCommand::with_name("extract_text")
				.about("Extract text")
				.arg(Arg::with_name("pages").value_name("page numbers").help("e.g. 3,5,7-9").takes_value(true)),
		)
		.subcommand(
			SubCommand::with_name("replace_text")
				.about("Replace text")
				.arg(Arg::with_name("text").value_name("page_number:old_text=>new_text").takes_value(true)),
		)
		.subcommand(
			SubCommand::with_name("extract_stream")
				.about("Extract stream content")
				.arg(Arg::with_name("ids").value_name("object ids").help("e.g. \"1 0,2 1,35,36\"").takes_value(true)),
		)
		.subcommand(SubCommand::with_name("print_streams").about("Print streams"))
		.subcommand(SubCommand::with_name("renumber_objects").about("Renumber objects"))
		.subcommand(SubCommand::with_name("delete_zero_length_streams").about("Delete zero length stream objects"))
		.get_matches();

	if let (cmd, Some(args)) = app.subcommand() {
		if let Some(input) = args.value_of("input") {
			info!("Open {}", input);
			let mut doc = Document::load(input).unwrap();
			//info!("{:?}", doc.get_pages());

			info!("Do {}", cmd);
			match cmd {
				"process" => {
					if let Some(operations) = args.values_of("operations") {
						for operation in operations {
							info!("Do {}", operation);
							apply_operation(&mut doc, operation);
						}
					}
				}
				"extract_pages" => {
					if let Some(pages) = args.value_of("pages") {
						let page_numbers = compute_page_numbers(pages);
						let total = *doc.get_pages().keys().max().unwrap_or(&0);
						let page_numbers = complement_page_numbers(&page_numbers, total);
						doc.delete_pages(&page_numbers);
					}
				}
				"delete_pages" => {
					if let Some(pages) = args.value_of("pages") {
						let page_numbers = compute_page_numbers(pages);
						doc.delete_pages(&page_numbers);
					}
				}
				"delete_objects" => {
					if let Some(ids) = args.value_of("ids") {
						for id in ids.split(',') {
							let nums: Vec<u32> = id.split(' ').map(|num| u32::from_str(num).unwrap()).collect();
							match nums.len() {
								1 => doc.delete_object((nums[0], 0)),
								2 => doc.delete_object((nums[0], nums[1] as u16)),
								_ => None,
							};
						}
					}
				}
				"extract_text" => {
					if let Some(pages) = args.value_of("pages") {
						let page_numbers = compute_page_numbers(pages);
						let text = doc.extract_text(&page_numbers);
						info!("{}", text.unwrap());
					}
				}
				"replace_text" => {
					if let Some(text) = args.value_of("text") {
						let parts: Vec<&str> = text.splitn(2, ':').collect();
						let page = u32::from_str(parts[0]).unwrap();
						let words: Vec<&str> = parts[1].splitn(2, "=>").collect();
						doc.replace_text(page, words[0], words[1]);
					}
				}
				"print_streams" => for (_, object) in doc.objects.iter() {
					match *object {
						Object::Stream(ref stream) => info!("{:?}", stream.dict),
						_ => (),
					}
				},
				"extract_stream" => {
					if let Some(ids) = args.value_of("ids") {
						for id in ids.split(',') {
							let nums: Vec<u32> = id.split(' ').map(|num| u32::from_str(num).unwrap()).collect();
							match nums.len() {
								1 => doc.extract_stream((nums[0], 0), false).ok(),
								2 => doc.extract_stream((nums[0], nums[1] as u16), false).ok(),
								_ => None,
							};
						}
					}
				}
				operation @ _ => {
					apply_operation(&mut doc, operation);
				}
			}

			doc.change_producer("https://crates.io/crates/lopdf");

			if let Some(output) = args.value_of("output") {
				info!("Save to {}", output);
				doc.save(output).unwrap();
			}
		}
	}

	fn apply_operation(doc: &mut Document, operation: &str) {
		match operation {
			"compress" => doc.compress(),
			"decompress" => doc.decompress(),
			"renumber_objects" => doc.renumber_objects(),
			"prune_objects" => {
				let ids = doc.prune_objects();
				info!("Deleted {:?}", ids);
			}
			"delete_zero_length_streams" => {
				let streams = doc.delete_zero_length_streams();
				if streams.len() > 0 {
					info!("Deleted {:?}", streams);
				}
			}
			_ => {}
		}
	}

	fn compute_page_numbers(pages: &str) -> Vec<u32> {
		let mut page_numbers = vec![];
		for page in pages.split(',') {
			let nums: Vec<u32> = page.split('-').map(|num| u32::from_str(num).unwrap()).collect();
			match nums.len() {
				1 => page_numbers.push(nums[0]),
				2 => page_numbers.append(&mut (nums[0]..nums[1] + 1).collect()),
				_ => {}
			}
		}
		page_numbers
	}

	fn complement_page_numbers(pages: &[u32], total: u32) -> Vec<u32> {
		let mut page_numbers = vec![];
		for page in 1..(total + 1) {
			if !pages.contains(&page) {
				page_numbers.push(page);
			}
		}
		page_numbers
	}
}
