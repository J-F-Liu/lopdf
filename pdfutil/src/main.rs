use std::collections::BTreeMap;
use lopdf::{Bookmark, Document, Object, ObjectId};
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
		.arg(Arg::with_name("merge").short("m").long("merge").value_name("merge files").takes_value(true).multiple(true).global(true))
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

	if let Some(_) = app.value_of("merge") {
		let filenames: Vec<&str> = app.values_of("merge").unwrap().collect();
		let documents: Vec<Document> = filenames.into_iter().map(|f| Document::load(f)).flatten().collect();
		// We use this to keep track of the last Parent per layer depth.
		let mut layer_parent: [Option<u32>; 4] = [None; 4];

		// This is the last layer ran.
		let mut last_layer = 0;

		// Define a starting max_id (will be used as start index for object_ids)
		let mut max_id = 1;
		let mut pagenum = 1;
		// Collect all Documents Objects grouped by a map
		let mut documents_pages = BTreeMap::new();
		let mut documents_objects = BTreeMap::new();
		let mut document = Document::with_version("1.5");

		// Lets try to set these to be bigger to avoid multi allocations for faster handling of files.
		// We are just saying each Document it about 1000 objects in size. can be adjusted for better speeds.
		// This can only be used if you use nightly or the #![feature(extend_one)] is stablized.
		// documents_pages.extend_reserve(documents.len() * 1000);
		// documents_objects.extend_reserve(documents.len() * 1000);

		// Add a Table of Contents
		// We set the object page to (0,0) which means it will point to the first object after it.
		layer_parent[0] = Some(document.add_bookmark(
				Bookmark::new("Table of Contents".to_string(), [0.0, 0.0, 0.0], 0, (0, 0)),
				None,
		));

		// Can set bookmark formatting and color per report bookmark added.
		// Formating is 1 for italic 2 for bold 3 for bold and italic
		// Color is RGB 0.0..255.0
		let mut layer = 0;
		for mut doc in documents {
			let color = [0.0, 0.0, 0.0];
			let format = 0;
			let mut display = String::new();

			doc.renumber_objects_with(max_id);

			max_id = doc.max_id + 1;

			let mut first_object = None;

			let pages = doc.get_pages();

			// This is actually better than extend as we use less allocations and cloning then.
			pages
				.into_iter()
				.map(|(_, object_id)| {
					// We use this as the return object for Bookmarking to deturmine what it points too.
					// We only want to do this for the first page though.
					if first_object.is_none() {
						first_object = Some(object_id);
						display = format!("Page {}", pagenum);
						pagenum += 1;
					}

					(object_id, doc.get_object(object_id).unwrap().to_owned())
				})
			.for_each(|(key, value)| {
				documents_pages.insert(key, value);
			});

			documents_objects.extend(doc.objects);

			// Lets shadow our pointer back if nothing then set to (0,0) tto point to the next page
			let object = first_object.unwrap_or((0, 0));

			// This will use the layering to implement children under Parents in the bookmarks
			// Example as we are generating it here.
			// Table of Contents
			// - Page 1
			// -- Page 2
			// -- Page 3
			// --- Page 4

			if layer == 0 {
				layer_parent[0] = Some(document.add_bookmark(Bookmark::new(display, color, format, object), None));
				last_layer = 0;
			} else if layer == 1 {
				layer_parent[1] =
					Some(document.add_bookmark(Bookmark::new(display, color, format, object), layer_parent[0]));
				last_layer = 1;
			} else if last_layer >= layer || last_layer == layer - 1 {
				layer_parent[layer as usize] = Some(document.add_bookmark(
						Bookmark::new(display, color, format, object),
						layer_parent[(layer - 1) as usize],
				));
				last_layer = layer;
			} else if last_layer > 0 {
				layer_parent[last_layer as usize] = Some(document.add_bookmark(
						Bookmark::new(display, color, format, object),
						layer_parent[(last_layer - 1) as usize],
				));
			} else {
				layer_parent[1] =
					Some(document.add_bookmark(Bookmark::new(display, color, format, object), layer_parent[0]));
				last_layer = 1;
			}
			layer += 1;
		}

		// Catalog and Pages are mandatory
		let mut catalog_object: Option<(ObjectId, Object)> = None;
		let mut pages_object: Option<(ObjectId, Object)> = None;

		// Process all objects except "Page" type
		for (object_id, object) in documents_objects.into_iter() {
			// We have to ignore "Page" (as are processed later), "Outlines" and "Outline" objects
			// All other objects should be collected and inserted into the main Document
			match object.type_name().unwrap_or("") {
				"Catalog" => {
					// Collect a first "Catalog" object and use it for the future "Pages"
					catalog_object = Some((
							if let Some((id, _)) = catalog_object {
								id
							} else {
								object_id
							},
							object,
					));
				}
				"Pages" => {
					// Collect and update a first "Pages" object and use it for the future "Catalog"
					// We have also to merge all dictionaries of the old and the new "Pages" object
					if let Ok(dictionary) = object.as_dict() {
						let mut dictionary = dictionary.clone();
						if let Some((_, ref object)) = pages_object {
							if let Ok(old_dictionary) = object.as_dict() {
								dictionary.extend(old_dictionary);
							}
						}

						pages_object = Some((
								if let Some((id, _)) = pages_object {
									id
								} else {
									object_id
								},
								Object::Dictionary(dictionary),
						));
					}
				}
				"Page" => {}     // Ignored, processed later and separately
				"Outlines" => {} // Ignored, not supported yet
				"Outline" => {}  // Ignored, not supported yet
				_ => {
					document.objects.insert(object_id, object);
				}
			}
		}

		// If no "Pages" found abort
		if pages_object.is_none() {
			println!("Pages root not found.");

			return;
		}

		// Iter over all "Page" and collect with the parent "Pages" created before
		for (object_id, object) in documents_pages.iter() {
			if let Ok(dictionary) = object.as_dict() {
				let mut dictionary = dictionary.clone();
				dictionary.set("Parent", pages_object.as_ref().unwrap().0);

				document.objects.insert(*object_id, Object::Dictionary(dictionary));
			}
		}

		// If no "Catalog" found abort
		if catalog_object.is_none() {
			println!("Catalog root not found.");

			return;
		}

		let (catalog_id, catalog_object) = catalog_object.unwrap();
		let (page_id, page_object) = pages_object.unwrap();

		// Build a new "Pages" with updated fields
		if let Ok(dictionary) = page_object.as_dict() {
			let mut dictionary = dictionary.clone();

			// Set new pages count
			dictionary.set("Count", documents_pages.len() as u32);

			// Set new "Kids" list (collected from documents pages) for "Pages"
			dictionary.set(
				"Kids",
				documents_pages
				.into_iter()
				.map(|(object_id, _)| Object::Reference(object_id))
				.collect::<Vec<_>>(),
			);

			document.objects.insert(page_id, Object::Dictionary(dictionary));
		}

		// Build a new "Catalog" with updated fields
		if let Ok(dictionary) = catalog_object.as_dict() {
			let mut dictionary = dictionary.clone();
			dictionary.set("Pages", page_id);
			dictionary.set("PageMode", "UseOutlines");
			dictionary.remove(b"Outlines"); // Outlines not supported in merged PDFs

			document.objects.insert(catalog_id, Object::Dictionary(dictionary));
		}

		document.trailer.set("Root", catalog_id);

		// Update the max internal ID as wasn't updated before due to direct objects insertion
		document.max_id = document.objects.len() as u32;

		// Reorder all new Document objects
		document.renumber_objects();

		//Set any Bookmarks to the First child if they are not set to a page
		document.adjust_zero_pages();

		//Set all bookmarks to the PDF Object tree then set the Outlines to the Bookmark content map.
		if let Some(n) = document.build_outline() {
			if let Ok(Object::Dictionary(ref mut dict)) = document.get_object_mut(catalog_id) {
				dict.set("Outlines", Object::Reference(n));
			}
		}

		// Most of the time this does nothing unless there are a lot of streams
		// Can be disabled to speed up the process.
		// document.compress();

		// Save the merged PDF
		// Store file in current working directory.
		if let Some(output) = app.value_of("output") {
			info!("Save to {}", output);
			document.save(output).unwrap();
		} else {
			document.save("merged.pdf").unwrap();
		}
	}

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
						let _ = doc.replace_text(page, words[0], words[1]);
					}
				}
				"print_streams" => for (_, object) in doc.objects.iter() {
					if let Object::Stream(ref stream) = *object {
						info!("{:?}", stream.dict);
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
				operation => {
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
				if !streams.is_empty() {
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
