use lopdf::{Document, Object, Result, dictionary};

mod utils;

/// One page whose `/Annots` mixes the two legal entry forms: a reference to
/// an annotation dictionary, and the dictionary written directly into the
/// array. Returns the document and the page's id.
fn page_with_mixed_annots(annots_indirect: bool) -> (Document, lopdf::ObjectId) {
    let mut doc = Document::with_version("1.7");
    let referenced = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Contents" => Object::string_literal("referenced"),
    });
    let entries = vec![
        Object::Reference(referenced),
        Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Contents" => Object::string_literal("direct"),
        }),
    ];
    // `/Annots` is itself allowed to be either an array or a reference to one.
    let annots = if annots_indirect {
        Object::Reference(doc.add_object(Object::Array(entries)))
    } else {
        Object::Array(entries)
    };
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Annots" => annots,
    });
    (doc, page_id)
}

fn contents_of(doc: &Document, page_id: lopdf::ObjectId) -> Vec<String> {
    doc.get_page_annotations(page_id)
        .unwrap()
        .iter()
        .map(|a| String::from_utf8_lossy(a.get(b"Contents").unwrap().as_str().unwrap()).into_owned())
        .collect()
}

/// A page whose `/Annots` holds one resolvable reference plus entries that do
/// not resolve. Returns the document and the page's id.
fn page_with_unresolvable_annots() -> (Document, lopdf::ObjectId) {
    let mut doc = Document::with_version("1.7");
    let good = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Contents" => Object::string_literal("kept"),
    });
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Annots" => vec![
            Object::Reference((9999, 0)),
            Object::Reference(good),
            Object::Null,
        ],
    });
    (doc, page_id)
}

/// An entry of `/Annots` may be the annotation dictionary itself rather than
/// a reference to one — ISO 32000-1, 12.5.2 requires an indirect object only
/// for an annotation that carries a `/Popup` or is an `/IRT` target. Direct
/// dictionaries used to be dropped without a word, so a page could report
/// fewer annotations than it has. Both entry forms must be handed out whether
/// `/Annots` is an array or a reference to one.
#[test]
fn page_annotations_include_direct_dictionaries() {
    for annots_indirect in [false, true] {
        let (doc, page_id) = page_with_mixed_annots(annots_indirect);
        assert_eq!(contents_of(&doc, page_id), vec!["referenced", "direct"]);
    }
}

/// The mutable accessor reaches both entry forms, so editing every returned
/// dictionary is visible afterwards, and whatever does not resolve is skipped
/// rather than taking the page down with it.
#[test]
fn page_annotations_mut_edits_every_entry() {
    for annots_indirect in [false, true] {
        let (mut doc, page_id) = page_with_mixed_annots(annots_indirect);
        for annotation in doc.get_page_annotations_mut(page_id).unwrap() {
            annotation.set("Contents", Object::string_literal("edited"));
        }
        assert_eq!(contents_of(&doc, page_id), vec!["edited", "edited"]);
    }

    // An entry that does not resolve costs its own annotation, not the page.
    let (mut doc, page_id) = page_with_unresolvable_annots();
    assert_eq!(contents_of(&doc, page_id), vec!["kept"]);

    let mut annotations = doc.get_page_annotations_mut(page_id).unwrap();
    assert_eq!(annotations.len(), 1);
    annotations[0].set("Contents", Object::string_literal("touched"));
    assert_eq!(contents_of(&doc, page_id), vec!["touched"]);
}

/// The same annotation referenced twice cannot be handed out as two aliasing
/// mutable references, so only the first occurrence comes back.
#[test]
fn page_annotations_mut_deduplicates_repeated_references() {
    let mut doc = Document::with_version("1.7");
    let annotation = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Contents" => Object::string_literal("once"),
    });
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Annots" => vec![Object::Reference(annotation), Object::Reference(annotation)],
    });
    assert_eq!(doc.get_page_annotations_mut(page_id).unwrap().len(), 1);
}

/// A page without `/Annots` yields nothing, and that is not an error.
#[test]
fn page_annotations_without_annots_is_empty() {
    let mut doc = Document::with_version("1.7");
    let page_id = doc.add_object(dictionary! { "Type" => "Page" });
    assert!(doc.get_page_annotations(page_id).unwrap().is_empty());
    assert!(doc.get_page_annotations_mut(page_id).unwrap().is_empty());
}

#[test]
fn annotation_count() -> Result<()> {
    // This test file from the pdfcpu repository,
    // https://github.com/pdfcpu/pdfcpu/blob/master/pkg/samples/basic/AnnotationDemo.pdf
    let doc = utils::load_document("assets/AnnotationDemo.pdf")?;
    assert_eq!(doc.version, "1.7".to_string());
    assert_eq!(doc.page_iter().count(), 1);
    assert_eq!(doc.get_page_annotations(doc.page_iter().next().unwrap())?.len(), 33);
    Ok(())
}
