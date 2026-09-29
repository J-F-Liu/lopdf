#[macro_use]
extern crate lopdf;
use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream};

fn main() {
    // with_version specifes the PDF version this document complies with.
    let mut doc = Document::with_version("1.5");

    // Object IDs cross-reference objects in a PDF; `lopdf` tracks them for us.

    // "Pages" is the root node of the page tree
    let pages_id = doc.new_object_id();

    // Fonts are dictionaries; the type, subtype and basefont tags are straight out of the PDF
    // reference manual. The `dictionary!` macro makes nested key/value pairs read like a match.
    let font_id = doc.add_object(dictionary! {
        // type of dictionary
        "Type" => "Font",
        // type of font, type1 is simple postscript font
        "Subtype" => "Type1",
        // basefont is postscript name of font for type1 font.
        // See PDF reference document for more details
        "BaseFont" => "Courier",
    });

    // Fonts must be reachable from a resource dictionary; only one is allowed per page tree root.
    let resources_id = doc.add_object(dictionary! {
        // Fonts are actually triplely nested dictionaries. Fun!
        "Font" => dictionary! {
            // F1 is the font name used when writing text; it must be unique in the document.
            "F1" => font_id,
        },
    });

    // Operations pair a PDF operator with its operands, listed in the reverse of file order.
    let content = Content {
        operations: vec![
            // BT begins a text element. it takes no operands
            Operation::new("BT", vec![]),
            // Tf sets the font and size; `into()` converts to a basic PDF object type.
            Operation::new("Tf", vec!["F1".into(), 48.into()]),
            // Td moves the text matrix. Right after BT it sets the initial position; Y=0 is the
            // page bottom, so 600 prints near the top.
            Operation::new("Td", vec![100.into(), 600.into()]),
            // Tj prints a filled black string literal; other operators vary the effect and color.
            Operation::new("Tj", vec![Object::string_literal("Hello World!")]),
            // ET ends the text element
            Operation::new("ET", vec![]),
        ],
    };

    // A stream is a dictionary plus bytes whose meaning depends on context. lopdf sets the
    // dictionary (Length, Filter, DecodeParams, ...) internally.
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));

    // Page is a dictionary that represents one page of a PDF file.
    // Its required fields are "Type", "Parent" and "Contents".
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
    });

    // The page tree root; its ID was created earlier so the page dictionary could point at it.
    // Many optional entries belong on the page dictionary rather than here, to avoid inheritance.
    let pages = dictionary! {
        // Type of dictionary
        "Type" => "Pages",
        // Vector of page IDs in document. Normally would contain more than one ID and be produced
        // using a loop of some kind
        "Kids" => vec![page_id.into()],
        // Page count
        "Count" => 1,
        // ID of resources dictionary, defined earlier
        "Resources" => resources_id,
        // a rectangle that defines the boundaries of the physical or digital media. This is the
        // "Page Size"
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    };

    // using insert() here, instead of add_object() since the id is already known.
    doc.objects.insert(pages_id, Object::Dictionary(pages));

    // Creating document catalog.
    // There are many more entries allowed in the catalog dictionary.
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });

    // Root key in trailer is set here to ID of document catalog,
    // remainder of trailer is set during doc.save().
    doc.trailer.set("Root", catalog_id);
    doc.compress();

    // Store file in current working directory.
    doc.save("example.pdf").unwrap();
}
