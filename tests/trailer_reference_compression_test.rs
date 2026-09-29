//! Which trailer entries keep an object out of an object stream: only the
//! dictionary the trailer points at with `/Encrypt`.

use lopdf::{Document, Object, ObjectStream, StringFormat, dictionary};

/// Nothing about being referenced from the trailer — not the catalog, not the
/// info dictionary, not an object referenced several times or only reachable
/// through a chain of references — keeps an object out of an object stream.
#[test]
fn trailer_references_do_not_block_compression() {
    let mut doc = Document::with_version("1.5");

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference((10, 0)),
    });
    let info_id = doc.add_object(dictionary! {
        "Title" => "Test",
        "Author" => "Test Author",
        "CreationDate" => "D:20250807120000Z",
    });
    let metadata_id = doc.add_object(dictionary! {
        "Type" => "Metadata",
        "Subtype" => "XML",
    });
    let outlines_id = doc.add_object(dictionary! {
        "Type" => "Outlines",
        "Count" => 0,
    });
    let shared_id = doc.add_object(dictionary! { "Shared" => "Dictionary" });

    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);
    doc.trailer.set("Metadata", metadata_id);
    doc.trailer.set("Outlines", outlines_id);
    doc.trailer.set("Custom1", shared_id);
    doc.trailer.set("Custom2", shared_id);
    doc.trailer.set("Custom3", shared_id);

    // Only the head of the chain is referenced from the trailer; the object
    // behind it is reachable only through it.
    let chain_tail = doc.add_object(dictionary! { "Level" => 2 });
    let chain_head = doc.add_object(dictionary! { "Level" => 1, "Next" => chain_tail });
    doc.trailer.set("Chain", chain_head);

    for id in [
        catalog_id,
        info_id,
        metadata_id,
        outlines_id,
        shared_id,
        chain_head,
        chain_tail,
    ] {
        let object = doc.objects.get(&id).unwrap();
        assert!(
            ObjectStream::can_be_compressed(id, object, &doc),
            "{id:?} is referenced from the trailer but should still be compressible"
        );
    }
}

/// Trailer entries that are not references cannot name an object, so they leave
/// the compressibility of the objects alone — including a malformed `/Encrypt`
/// that holds a value instead of a reference.
#[test]
fn non_reference_trailer_entries_do_not_block_compression() {
    let mut doc = Document::with_version("1.5");
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog" });

    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Size", Object::Integer(100));
    doc.trailer.set("Prev", Object::Integer(1234));
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(vec![1, 2, 3, 4], StringFormat::Hexadecimal),
            Object::String(vec![5, 6, 7, 8], StringFormat::Hexadecimal),
        ]),
    );
    doc.trailer.set("Encrypt", Object::Null);

    let catalog = doc.objects.get(&catalog_id).unwrap();
    assert!(ObjectStream::can_be_compressed(catalog_id, catalog, &doc));
}

/// It is the reference itself, not the shape of the dictionary, that excludes
/// the encryption dictionary: a dictionary that looks like one and is not
/// referenced by `/Encrypt` is compressible like any other.
#[test]
fn only_the_encryption_dictionary_named_by_the_trailer_is_excluded() {
    let mut doc = Document::with_version("1.5");

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference((10, 0)),
    });
    let encrypt_id = doc.add_object(dictionary! {
        "Filter" => "Standard",
        "V" => 2,
        "R" => 3,
        "Length" => 128,
    });
    let encryption_lookalike_id = doc.add_object(dictionary! {
        "Filter" => "Standard",
        "V" => 2,
        "R" => 3,
    });

    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Encrypt", encrypt_id);

    for id in [catalog_id, encryption_lookalike_id] {
        let object = doc.objects.get(&id).unwrap();
        assert!(
            ObjectStream::can_be_compressed(id, object, &doc),
            "{id:?} should be compressible"
        );
    }
    let encrypt = doc.objects.get(&encrypt_id).unwrap();
    assert!(!ObjectStream::can_be_compressed(encrypt_id, encrypt, &doc));
}

/// Linearization excludes the catalog only; the other dictionaries the trailer
/// points at stay compressible.
#[test]
fn linearized_catalog_is_excluded_but_other_trailer_references_are_not() {
    let mut doc = Document::with_version("1.5");
    doc.add_object(dictionary! {
        "Linearized" => 1,
        "L" => 12345,
        "H" => vec![Object::Integer(100), Object::Integer(200)],
    });

    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog" });
    let info_id = doc.add_object(dictionary! { "Title" => "Linearized PDF" });
    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);

    let catalog = doc.objects.get(&catalog_id).unwrap();
    assert!(!ObjectStream::can_be_compressed(catalog_id, catalog, &doc));
    let info = doc.objects.get(&info_id).unwrap();
    assert!(ObjectStream::can_be_compressed(info_id, info, &doc));
}
