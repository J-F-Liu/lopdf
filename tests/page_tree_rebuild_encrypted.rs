//! An encrypted document loaded without the password it needs has no objects yet, so the
//! catalog and page tree cannot load from it. That is not damage, and the load is not repeated
//! with the table rebuilt. A rebuild logs that it reconstructed the table, which is what this
//! counts; it has its own test binary because a logger is process-wide.

use std::cell::Cell;
use std::sync::Once;

use lopdf::{Document, EncryptionState, EncryptionVersion, LoadOptions, Object, Permissions, Stream, dictionary};

thread_local! {
    static RECONSTRUCTIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountReconstructions;

impl log::Log for CountReconstructions {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if record
            .args()
            .to_string()
            .starts_with("reconstructed cross-reference table")
        {
            RECONSTRUCTIONS.with(|count| count.set(count.get() + 1));
        }
    }

    fn flush(&self) {}
}

/// How many times `load` reconstructs a cross-reference table on this thread.
fn reconstructions<T>(load: impl FnOnce() -> T) -> (T, usize) {
    static LOGGER: Once = Once::new();
    LOGGER.call_once(|| {
        log::set_logger(&CountReconstructions).unwrap();
        log::set_max_level(log::LevelFilter::Warn);
    });
    RECONSTRUCTIONS.with(|count| count.set(0));
    let result = load();
    (result, RECONSTRUCTIONS.with(Cell::get))
}

/// A one-page document encrypted with the user password `user`.
fn encrypted_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::string_literal(b"0123456789abcdef"),
            Object::string_literal(b"fedcba9876543210"),
        ]),
    );
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT (Hello) Tj ET".to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
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

    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&state).unwrap();
    let mut pdf = Vec::new();
    doc.save_to(&mut pdf).unwrap();
    pdf
}

#[test]
fn an_encrypted_document_without_its_password_is_not_rebuilt() {
    let pdf = encrypted_pdf();

    // The counter sees a rebuild.
    let force = LoadOptions {
        force_reconstruct: true,
        ..Default::default()
    };
    let (_, count) = reconstructions(|| Document::load_mem_with_options(&pdf, force));
    assert_eq!(count, 1);

    let (document, count) = reconstructions(|| Document::load_mem(&pdf));
    let document = document.unwrap();
    assert!(document.is_encrypted());
    assert_eq!(count, 0, "the load was repeated with the table rebuilt");

    let (metadata, count) = reconstructions(|| Document::load_metadata_mem(&pdf));
    assert!(metadata.unwrap().encrypted);
    assert_eq!(count, 0, "the metadata probe was repeated with the table rebuilt");

    // With the password, the document loads and is not rebuilt either.
    let (document, count) =
        reconstructions(|| Document::load_mem_with_options(&pdf, LoadOptions::with_password("user")));
    assert_eq!(document.unwrap().get_pages().len(), 1);
    assert_eq!(count, 0);
}
