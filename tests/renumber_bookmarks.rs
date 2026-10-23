//! Renumbering moves each bookmark to its page's new object number once.

use lopdf::{Bookmark, Document, Object, ObjectId, dictionary};

/// Two pages whose object numbers descend in page order, each with a bookmark.
fn two_pages_in_descending_order() -> Document {
    let mut doc = Document::with_version("1.7");
    let pages: ObjectId = (1, 0);
    let (first, second): (ObjectId, ObjectId) = ((3, 0), (2, 0));
    for page in [first, second] {
        doc.objects.insert(
            page,
            Object::Dictionary(dictionary! { "Type" => "Page", "Parent" => pages }),
        );
    }
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![first.into(), second.into()], "Count" => 2,
        }),
    );
    doc.objects.insert(
        (4, 0),
        Object::Dictionary(dictionary! { "Type" => "Catalog", "Pages" => pages }),
    );
    doc.trailer.set("Root", (4, 0));
    doc.max_id = 4;
    doc.add_bookmark(Bookmark::new("first".into(), [0.0; 3], 0, first), None);
    doc.add_bookmark(Bookmark::new("second".into(), [0.0; 3], 0, second), None);
    doc
}

#[test]
fn renumbering_keeps_each_bookmark_on_its_page() {
    let mut doc = two_pages_in_descending_order();
    doc.renumber_objects();
    let pages = doc.get_pages();
    for (page_number, title) in [(1, "first"), (2, "second")] {
        let bookmark = doc.bookmark_table.values().find(|b| b.title == title).unwrap();
        assert_eq!(bookmark.page, pages[&page_number], "{title}");
    }
}

#[test]
fn renumbering_moves_each_bookmark_once() {
    // Starting at 3 maps 2 to 4 and 4 to 6: the bookmark of the page that becomes 2 must end on
    // 4, not follow the chain to 6.
    let mut doc = two_pages_in_descending_order();
    let pages_before = doc.get_pages();
    doc.renumber_objects_with(3);
    let pages = doc.get_pages();
    assert_eq!(pages.len(), pages_before.len());
    for (page_number, title) in [(1, "first"), (2, "second")] {
        let bookmark = doc.bookmark_table.values().find(|b| b.title == title).unwrap();
        assert_eq!(bookmark.page, pages[&page_number], "{title}");
    }
}
