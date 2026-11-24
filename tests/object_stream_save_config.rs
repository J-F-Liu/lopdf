use lopdf::{Document, Object, ObjectStreamConfig, SaveOptions, dictionary};

/// A `SaveOptions` filled in by hand can ask for 0 objects per object stream, which the builder
/// reads as the default: saving must keep every object then, too.
#[test]
fn zero_objects_per_stream_keeps_every_object() {
    let mut document = Document::with_version("1.5");
    let pages = document.new_object_id();
    let page = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages });
    let kids = vec![Object::Reference(page)];
    document.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 1 }.into(),
    );
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    document.trailer.set("Root", catalog);
    let extras: Vec<_> = (0..5)
        .map(|number| document.add_object(dictionary! { "Number" => number }))
        .collect();

    let options = SaveOptions {
        use_object_streams: true,
        use_xref_streams: true,
        object_stream_config: ObjectStreamConfig {
            max_objects_per_stream: 0,
            ..ObjectStreamConfig::default()
        },
        ..SaveOptions::default()
    };
    let mut saved = Vec::new();
    document.save_with_options(&mut saved, options).unwrap();

    let reloaded = Document::load_mem(&saved).unwrap();
    assert_eq!(reloaded.get_pages().len(), 1);
    for (number, id) in extras.into_iter().enumerate() {
        let extra = reloaded.get_dictionary(id).unwrap();
        assert_eq!(extra.get(b"Number").unwrap().as_i64().unwrap(), number as i64);
    }
}
