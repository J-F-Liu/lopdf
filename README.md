# lopdf

[![Crates.io](https://img.shields.io/crates/v/lopdf.svg)](https://crates.io/crates/lopdf)
[![CI](https://github.com/J-F-Liu/lopdf/actions/workflows/ci.yml/badge.svg)](https://github.com/J-F-Liu/lopdf/actions/workflows/ci.yml)
[![Docs]( https://docs.rs/lopdf/badge.svg)](https://docs.rs/lopdf)

A Rust library for PDF document manipulation.

A useful reference for understanding the PDF file format and the
eventual usage of this library is the
[PDF 1.7 Reference Document](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf).
The PDF 2.0 specification is available [here](https://www.pdfa.org/announcing-no-cost-access-to-iso-32000-2-pdf-2-0/).

## Example Code

* Create PDF document

```rust
use lopdf::dictionary;
use lopdf::{Document, Object, Stream};
use lopdf::content::{Content, Operation};

// `with_version` specifes the PDF version this document complies with.
let mut doc = Document::with_version("1.5");
// Object IDs are used for cross referencing in PDF documents.
// `lopdf` helps keep track of them for us. They are simple integers.
// Calls to `doc.new_object_id` and `doc.add_object` return an object ID.

// "Pages" is the root node of the page tree.
let pages_id = doc.new_object_id();

// Fonts are dictionaries. The "Type", "Subtype" and "BaseFont" tags
// are straight out of the PDF spec.
//
// The dictionary macro is a helper that allows complex
// key-value relationships to be represented in a simpler
// visual manner, similar to a match statement.
// A dictionary is implemented as an IndexMap of Vec<u8>, and Object
let font_id = doc.add_object(dictionary! {
    // type of dictionary
    "Type" => "Font",
    // type of font, type1 is simple postscript font
    "Subtype" => "Type1",
    // basefont is postscript name of font for type1 font.
    // See PDF reference document for more details
    "BaseFont" => "Courier",
});

// Font dictionaries need to be added into resource
// dictionaries in order to be used.
// Resource dictionaries can contain more than just fonts,
// but normally just contains fonts.
// Only one resource dictionary is allowed per page tree root.
let resources_id = doc.add_object(dictionary! {
    // Fonts are actually triplely nested dictionaries. Fun!
    "Font" => dictionary! {
        // F1 is the font name used when writing text.
        // It must be unique in the document. It does not
        // have to be F1
        "F1" => font_id,
    },
});

// `Content` is a wrapper struct around an operations struct that contains
// a vector of operations. The operations struct contains a vector of
// that match up with a particular PDF operator and operands.
// Refer to the PDF spec for more details on the operators and operands
// Note, the operators and operands are specified in a reverse order
// from how they actually appear in the PDF file itself.
let content = Content {
    operations: vec![
        // BT begins a text element. It takes no operands.
        Operation::new("BT", vec![]),
        // Tf specifies the font and font size.
        // Font scaling is complicated in PDFs.
        // Refer to the spec for more info.
        // The `into()` methods convert the types into
        // an enum that represents the basic object types in PDF documents.
        Operation::new("Tf", vec!["F1".into(), 48.into()]),
        // Td adjusts the translation components of the text matrix.
        // When used for the first time after BT, it sets the initial
        // text position on the page.
        // Note: PDF documents have Y=0 at the bottom. Thus 600 to print text near the top.
        Operation::new("Td", vec![100.into(), 600.into()]),
        // Tj prints a string literal to the page. By default, this is black text that is
        // filled in. There are other operators that can produce various textual effects and
        // colors
        Operation::new("Tj", vec![Object::string_literal("Hello World!")]),
        // ET ends the text element.
        Operation::new("ET", vec![]),
    ],
};

// Streams are a dictionary followed by a (possibly encoded) sequence of bytes.
// What that sequence of bytes represents, depends on the context.
// The stream dictionary is set internally by lopdf and normally doesn't
// need to be manually manipulated. It contains keys such as
// Length, Filter, DecodeParams, etc.
let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));

// Page is a dictionary that represents one page of a PDF file.
// Its required fields are "Type", "Parent" and "Contents".
let page_id = doc.add_object(dictionary! {
    "Type" => "Page",
    "Parent" => pages_id,
    "Contents" => content_id,
});

// Again, "Pages" is the root of the page tree. The ID was already created
// at the top of the page, since we needed it to assign to the parent element
// of the page dictionary.
//
// These are just the basic requirements for a page tree root object.
// There are also many additional entries that can be added to the dictionary,
// if needed. Some of these can also be defined on the page dictionary itself,
// and not inherited from the page tree root.
let pages = dictionary! {
    // Type of dictionary
    "Type" => "Pages",
    // Vector of page IDs in document. Normally would contain more than one ID
    // and be produced using a loop of some kind.
    "Kids" => vec![page_id.into()],
    // Page count
    "Count" => 1,
    // ID of resources dictionary, defined earlier
    "Resources" => resources_id,
    // A rectangle that defines the boundaries of the physical or digital media.
    // This is the "page size".
    "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
};

// Using `insert()` here, instead of `add_object()` since the ID is already known.
doc.objects.insert(pages_id, Object::Dictionary(pages));

// Creating document catalog.
// There are many more entries allowed in the catalog dictionary.
let catalog_id = doc.add_object(dictionary! {
    "Type" => "Catalog",
    "Pages" => pages_id,
});

// The "Root" key in trailer is set to the ID of the document catalog,
// the remainder of the trailer is set during `doc.save()`.
doc.trailer.set("Root", catalog_id);
doc.compress();

// Store file in current working directory.
// Note: Line is excluded when running tests
if false {
    doc.save("example.pdf").unwrap();
}
```

* Merge PDF documents

```rust
use lopdf::dictionary;

use std::collections::BTreeMap;

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, ObjectId, Stream, Bookmark};

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

fn main() -> std::io::Result<()> {
    // Generate a stack of Documents to merge.
    let documents = vec![
        generate_fake_document(),
        generate_fake_document(),
        generate_fake_document(),
        generate_fake_document(),
    ];

    // Define a starting `max_id` (will be used as start index for object_ids).
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
                            let bookmark = Bookmark::new(String::from(format!("Page_{}", pagenum)), [0.0, 0.0, 1.0], 0, object_id);
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

    // "Catalog" and "Pages" are mandatory.
    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    // Process all objects except "Page" type
    for (object_id, object) in documents_objects.iter() {
        // We have to ignore "Page" (as are processed later), "Outlines" and "Outline" objects.
        // All other objects should be collected and inserted into the main Document.
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                // Collect a first "Catalog" object and use it for the future "Pages".
                catalog_object = Some((
                    if let Some((id, _)) = catalog_object {
                        id
                    } else {
                        *object_id
                    },
                    object.clone(),
                ));
            }
            b"Pages" => {
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
            b"Page" => {}     // Ignored, processed later and separately
            b"Outlines" => {} // Ignored, not supported yet
            b"Outline" => {}  // Ignored, not supported yet
            _ => {
                document.objects.insert(*object_id, object.clone());
            }
        }
    }

    // If no "Pages" object found, abort.
    if pages_object.is_none() {
        println!("Pages root not found.");

        return Ok(());
    }

    // Iterate over all "Page" objects and collect into the parent "Pages" created before
    for (object_id, object) in documents_pages.iter() {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_object.as_ref().unwrap().0);

            document
                    .objects
                    .insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    // If no "Catalog" found, abort.
    if catalog_object.is_none() {
        println!("Catalog root not found.");

        return Ok(());
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

    // Set any Bookmarks to the First child if they are not set to a page
    document.adjust_zero_pages();

    // Set all bookmarks to the PDF Object tree then set the Outlines to the Bookmark content map.
    if let Some(n) = document.build_outline() {
        if let Ok(Object::Dictionary(dict)) = document.get_object_mut(catalog_object.0) {
            dict.set("Outlines", Object::Reference(n));
        }
    }

    document.compress();

    // Save the merged PDF.
    // Store file in current working directory.
    // Note: Line is excluded when running doc tests
    if false {
        document.save("merged.pdf").unwrap();
    }

    Ok(())
}
```

* Modify PDF document

```rust
use lopdf::Document;

// For this example to work a parser feature needs to be enabled
#[cfg(not(feature = "async"))]
#[cfg(feature = "nom_parser")]
{
    let mut doc = Document::load("assets/example.pdf").unwrap();

    doc.version = "1.4".to_string();
    doc.replace_text(1, "Hello World!", "Modified text!");
    // Store file in current working directory.
    // Note: Line is excluded when running tests
    if false {
        doc.save("modified.pdf").unwrap();
    }
}

#[cfg(feature = "async")]
#[cfg(feature = "nom_parser")]
{
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("Failed to create runtime")
        .block_on(async move {
            let mut doc = Document::load("assets/example.pdf").await.unwrap();
            
            doc.version = "1.4".to_string();
            doc.replace_text(1, "Hello World!", "Modified text!");
            // Store file in current working directory.
            // Note: Line is excluded when running tests
            if false {
                doc.save("modified.pdf").unwrap();
            }
    });
}
```

## FAQ

* Why does the library keep everything in memory as high-level objects until finally serializing the entire document?

    Normally, a PDF document won't be very large, ranging from tens of KB to hundreds of MB. Memory size is not a bottle neck for today's computer.
    By keeping the whole document in memory, the stream length can be pre-calculated, no need to use a reference object for the Length entry.
    The resulting PDF file is smaller for distribution and faster for PDF consumers to process.

    Producing is a one-time effort, while consuming is many more.
