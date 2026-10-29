use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, dictionary};

/// A page with no `/Resources` of its own under a `/Pages` node whose `/Resources`
/// (an indirect object) holds a font `F1`. Returns the document, the page and the font.
fn page_inheriting_font() -> (Document, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
            "Resources" => resources_id,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, page_id, font_id)
}

fn page_resources(doc: &Document, page_id: ObjectId) -> &Dictionary {
    let page = doc.get_dictionary(page_id).unwrap();
    doc.get_dict_in_dict(page, b"Resources").unwrap()
}

/// `/Resources` is inheritable (ISO 32000-1, 7.7.3.4). A page that has none must not get an
/// empty dictionary when a resource is added to it, as that would hide the inherited one.
#[test]
fn add_xobject_keeps_inherited_resources() {
    let (mut doc, page_id, font_id) = page_inheriting_font();
    let xobject_id = doc.add_object(dictionary! {});
    doc.add_xobject(page_id, "X1", xobject_id).unwrap();

    let resources = page_resources(&doc, page_id);
    let font = doc.get_dict_in_dict(resources, b"Font").unwrap();
    assert_eq!(font.get(b"F1").unwrap(), &Object::Reference(font_id));
    let xobjects = doc.get_dict_in_dict(resources, b"XObject").unwrap();
    assert_eq!(xobjects.get(b"X1").unwrap(), &Object::Reference(xobject_id));

    // The page gets its own copy, the inherited dictionary itself is left alone.
    let pages_id = doc.get_dictionary(page_id).unwrap().get(b"Parent").unwrap();
    let pages = doc.get_dictionary(pages_id.as_reference().unwrap()).unwrap();
    assert!(!doc.get_dict_in_dict(pages, b"Resources").unwrap().has(b"XObject"));
}

#[test]
fn add_graphics_state_keeps_inherited_resources() {
    let (mut doc, page_id, font_id) = page_inheriting_font();
    let gs_id = doc.add_object(dictionary! { "Type" => "ExtGState", "CA" => 0.5 });
    doc.add_graphics_state(page_id, "GS1", gs_id).unwrap();

    let resources = page_resources(&doc, page_id);
    let font = doc.get_dict_in_dict(resources, b"Font").unwrap();
    assert_eq!(font.get(b"F1").unwrap(), &Object::Reference(font_id));
    let states = doc.get_dict_in_dict(resources, b"ExtGState").unwrap();
    assert_eq!(states.get(b"GS1").unwrap(), &Object::Reference(gs_id));
}

#[test]
fn incremental_add_xobject_keeps_inherited_resources() {
    let (mut doc, page_id, font_id) = page_inheriting_font();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let prev = Document::load_mem(&bytes).unwrap();
    let mut incremental = IncrementalDocument::create_from(bytes, prev);

    let xobject_id = incremental.new_document.add_object(dictionary! {});
    incremental.add_xobject(page_id, "X1", xobject_id).unwrap();

    let doc = &incremental.new_document;
    let resources = doc.get_dictionary(page_id).unwrap().get(b"Resources").unwrap();
    let resources = resources.as_dict().unwrap();
    let font = incremental
        .get_prev_documents()
        .get_dict_in_dict(resources, b"Font")
        .unwrap();
    assert_eq!(font.get(b"F1").unwrap(), &Object::Reference(font_id));
    let xobjects = resources.get(b"XObject").and_then(Object::as_dict).unwrap();
    assert_eq!(xobjects.get(b"X1").unwrap(), &Object::Reference(xobject_id));
}

/// A page whose `/Resources` has `/ExtGState` as a reference to another object holding `GS0`.
/// Returns the document, the page, the `/ExtGState` dictionary and the `GS0` graphics state.
fn page_with_referenced_ext_g_state() -> (Document, ObjectId, ObjectId, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let gs0_id = doc.add_object(dictionary! { "Type" => "ExtGState", "CA" => 1 });
    let states_id = doc.add_object(dictionary! { "GS0" => gs0_id });
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => dictionary! { "ExtGState" => states_id },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, page_id, states_id, gs0_id)
}

#[test]
fn add_graphics_state_to_referenced_ext_g_state() {
    let (mut doc, page_id, states_id, gs0_id) = page_with_referenced_ext_g_state();
    let gs1_id = doc.add_object(dictionary! { "Type" => "ExtGState", "CA" => 0.5 });
    doc.add_graphics_state(page_id, "GS1", gs1_id).unwrap();

    let resources = page_resources(&doc, page_id);
    assert_eq!(resources.get(b"ExtGState").unwrap(), &Object::Reference(states_id));
    let states = doc.get_dictionary(states_id).unwrap();
    assert_eq!(states.get(b"GS0").unwrap(), &Object::Reference(gs0_id));
    assert_eq!(states.get(b"GS1").unwrap(), &Object::Reference(gs1_id));
}

#[test]
fn incremental_add_graphics_state_to_referenced_ext_g_state() {
    let (mut doc, page_id, states_id, gs0_id) = page_with_referenced_ext_g_state();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let prev = Document::load_mem(&bytes).unwrap();
    let mut incremental = IncrementalDocument::create_from(bytes, prev);

    let gs1_id = incremental
        .new_document
        .add_object(dictionary! { "Type" => "ExtGState", "CA" => 0.5 });
    incremental.add_graphics_state(page_id, "GS1", gs1_id).unwrap();

    // The `/ExtGState` dictionary is copied into the update, the previous document is unchanged.
    let states = incremental.new_document.get_dictionary(states_id).unwrap();
    assert_eq!(states.get(b"GS0").unwrap(), &Object::Reference(gs0_id));
    assert_eq!(states.get(b"GS1").unwrap(), &Object::Reference(gs1_id));
    let prev_states = incremental.get_prev_documents().get_dictionary(states_id).unwrap();
    assert!(!prev_states.has(b"GS1"));
}
