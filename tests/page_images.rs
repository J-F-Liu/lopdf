use lopdf::{Document, Error, Object, ObjectId, Stream, dictionary};

struct PageTree {
    doc: Document,
    root_id: ObjectId,
    middle_id: ObjectId,
    page_id: ObjectId,
    image_id: ObjectId,
}

/// A page under two levels of `/Pages` nodes (root -> middle -> page) and a 1x1 image
/// XObject, with no `/Resources` anywhere yet.
fn page_tree() -> PageTree {
    let mut doc = Document::with_version("1.7");
    let image_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 1,
            "Height" => 1,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        vec![0x80],
    ));
    let root_id = doc.new_object_id();
    let middle_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => middle_id });
    doc.objects.insert(
        middle_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Parent" => root_id,
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    doc.objects.insert(
        root_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![middle_id.into()],
            "Count" => 1,
        }),
    );
    PageTree {
        doc,
        root_id,
        middle_id,
        page_id,
        image_id,
    }
}

fn image_ids(doc: &Document, page_id: ObjectId) -> Vec<ObjectId> {
    doc.get_page_images(page_id)
        .unwrap()
        .iter()
        .map(|image| image.id)
        .collect()
}

/// `/Resources` is inheritable, so a page without its own uses the one of the nearest
/// ancestor in the page tree, whether that one is a reference or written directly.
#[test]
fn page_images_from_inherited_resources() {
    let mut tree = page_tree();
    let resources_id = tree.doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im1" => tree.image_id },
    });
    tree.doc
        .get_dictionary_mut(tree.root_id)
        .unwrap()
        .set("Resources", resources_id);
    assert_eq!(image_ids(&tree.doc, tree.page_id), vec![tree.image_id]);

    let mut tree = page_tree();
    tree.doc.get_dictionary_mut(tree.middle_id).unwrap().set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Im1" => tree.image_id } },
    );
    assert_eq!(image_ids(&tree.doc, tree.page_id), vec![tree.image_id]);
}

/// Inherited resources are replaced, not merged: a page with its own `/Resources` doesn't
/// see the images of its ancestors. A page with no resources at all has no images.
#[test]
fn page_images_from_nearest_resources_only() {
    let mut tree = page_tree();
    tree.doc.get_dictionary_mut(tree.root_id).unwrap().set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Im1" => tree.image_id } },
    );
    tree.doc
        .get_dictionary_mut(tree.page_id)
        .unwrap()
        .set("Resources", dictionary! {});
    assert_eq!(image_ids(&tree.doc, tree.page_id), vec![]);

    let tree = page_tree();
    assert_eq!(image_ids(&tree.doc, tree.page_id), vec![]);
}

/// A `/Parent` cycle is reported instead of being followed forever.
#[test]
fn page_images_with_parent_cycle() {
    let mut tree = page_tree();
    tree.doc
        .get_dictionary_mut(tree.root_id)
        .unwrap()
        .set("Parent", tree.middle_id);
    assert!(matches!(
        tree.doc.get_page_images(tree.page_id),
        Err(Error::ReferenceCycle(_))
    ));
}
