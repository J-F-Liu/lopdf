# lopdf

[![Crates.io](https://img.shields.io/crates/v/lopdf.svg)](https://crates.io/crates/lopdf)
[![Build Status](https://travis-ci.org/J-F-Liu/lopdf.png)](https://travis-ci.org/J-F-Liu/lopdf)
[![Docs]( https://docs.rs/lopdf/badge.svg)](https://docs.rs/lopdf)

A Rust library for PDF document manipulation.

## Example Code

* Create PDF document

```rust
#[macro_use]
extern crate lopdf;
use lopdf::{Document, Object, Stream};
use lopdf::content::{Content, Operation};

let mut doc = Document::with_version("1.5");
let pages_id = doc.new_object_id();
let font_id = doc.add_object(dictionary! {
    "Type" => "Font",
    "Subtype" => "Type1",
    "BaseFont" => "Courier",
});
let resources_id = doc.add_object(dictionary! {
    "Font" => dictionary! {
        "F1" => font_id,
    },
});
let content = Content {
    operations: vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 48.into()]),
        Operation::new("Td", vec![100.into(), 600.into()]),
        Operation::new("Tj", vec![Object::string_literal("Hello World!")]),
        Operation::new("ET", vec![]),
    ],
};
let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
let page_id = doc.add_object(dictionary! {
    "Type" => "Page",
    "Parent" => pages_id,
    "Contents" => content_id,
});
let pages = dictionary! {
    "Type" => "Pages",
    "Kids" => vec![page_id.into()],
    "Count" => 1,
    "Resources" => resources_id,
    "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
};
doc.objects.insert(pages_id, Object::Dictionary(pages));
let catalog_id = doc.add_object(dictionary! {
    "Type" => "Catalog",
    "Pages" => pages_id,
});
doc.trailer.set("Root", catalog_id);
doc.compress();
doc.save("example.pdf").unwrap();
```

* Merge PDF documents

```rust
#[macro_use]
extern crate lopdf;

use std::collections::BTreeMap;

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, ObjectId, Stream, BookMark};

pub fn generate_fake_document() -> Document {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Courier",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 48.into()]),
            Operation::new("Td", vec![100.into(), 600.into()]),
            Operation::new("Tj", vec![Object::string_literal("Hello World!")]),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![page_id.into()],
        "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    doc
}

fn main() {
    // Generate a stack of Documents to merge
    let documents = vec![
        generate_fake_document(),
        generate_fake_document(),
        generate_fake_document(),
        generate_fake_document(),
    ];

    // Define a starting max_id (will be used as start index for object_ids)
    let mut max_id = 1;
    let mut pagenum = 1;
    // Collect all Documents Objects grouped by a map
    let mut documents_pages = BTreeMap::new();
    let mut documents_objects = BTreeMap::new();
    let mut document = Document::with_version("1.5");

    for mut doc in documents {
        let mut first = false;
        doc.renumber_objects_with(max_id);

        max_id = doc.max_id + 1;

        documents_pages.extend(
            doc
                    .get_pages()
                    .into_iter()
                    .map(|(_, object_id)| {
                        if !first {
                            let bookmark = BookMark::new(String::from(format!("Page_{}", pagenum)), [0.0, 0.0, 1.0], 0, object_id);
                            document.add_bookmark(bookmark, None);
                            first = true;
                            pagenum += 1;
                        }

                        (
                            object_id,
                            doc.get_object(object_id).unwrap().to_owned(),
                        )
                    })
                    .collect::<BTreeMap<ObjectId, Object>>(),
        );
        documents_objects.extend(doc.objects);
    }

    // Catalog and Pages are mandatory
    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    // Process all objects except "Page" type
    for (object_id, object) in documents_objects.iter() {
        // We have to ignore "Page" (as are processed later), "Outlines" and "Outline" objects
        // All other objects should be collected and inserted into the main Document
        match object.type_name().unwrap_or("") {
            "Catalog" => {
                // Collect a first "Catalog" object and use it for the future "Pages"
                catalog_object = Some((
                    if let Some((id, _)) = catalog_object {
                        id
                    } else {
                        *object_id
                    },
                    object.clone(),
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
                            *object_id
                        },
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            "Page" => {}     // Ignored, processed later and separately
            "Outlines" => {} // Ignored, not supported yet
            "Outline" => {}  // Ignored, not supported yet
            _ => {
                document.objects.insert(*object_id, object.clone());
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

            document
                    .objects
                    .insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    // If no "Catalog" found abort
    if catalog_object.is_none() {
        println!("Catalog root not found.");

        return;
    }

    let catalog_object = catalog_object.unwrap();
    let pages_object = pages_object.unwrap();

    // Build a new "Pages" with updated fields
    if let Ok(dictionary) = pages_object.1.as_dict() {
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

        document
                .objects
                .insert(pages_object.0, Object::Dictionary(dictionary));
    }

    // Build a new "Catalog" with updated fields
    if let Ok(dictionary) = catalog_object.1.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_object.0);
        dictionary.remove(b"Outlines"); // Outlines not supported in merged PDFs

        document
                .objects
                .insert(catalog_object.0, Object::Dictionary(dictionary));
    }

    document.trailer.set("Root", catalog_object.0);

    // Update the max internal ID as wasn't updated before due to direct objects insertion
    document.max_id = document.objects.len() as u32;

    // Reorder all new Document objects
    document.renumber_objects();

     //Set any Bookmarks to the First child if they are not set to a page
    document.adjust_zero_pages();

    //Set all bookmarks to the PDF Object tree then set the Outlines to the Bookmark content map.
    if let Some(n) = document.build_outline() {
        if let Ok(x) = document.get_object_mut(catalog_object.0) {
            if let Object::Dictionary(ref mut dict) = x {
                dict.set("Outlines", Object::Reference(n));
            }
        }
    }

    document.compress();

    // Save the merged PDF
    document.save("merged.pdf").unwrap();
}
```

* Modify PDF document

```rust
let mut doc = Document::load("example.pdf")?;
doc.version = "1.4".to_string();
doc.replace_text(1, "Hello World!", "Modified text!");
doc.save("modified.pdf")?;
```

## FAQ

* Why keeping everything in memory as high-level objects until finally serializing the entire document?

    Normally a PDF document won't be very large, ranging form tens of KB to hundreds of MB. Memory size is not a bottle neck for today's computer.
    By keep the whole document in memory, stream length can be pre-calculated, no need to use a reference object for the Length entry,
    the resulting PDF file is smaller for distribution and faster for PDF consumers to process.

    Producing is a one-time effort, while consuming is many more.
