//! Identity is predefined for V4/V5 and is also the default when StmF/StrF are absent.

use std::{collections::BTreeMap, sync::Arc};

use lopdf::{
    Document, EncryptionState, EncryptionVersion, Object, Permissions, Stream, dictionary,
    encryption::{
        crypt_filters::{Aes128CryptFilter, Aes256CryptFilter, CryptFilter},
        decrypt_object, encrypt_object,
    },
};

fn document() -> Document {
    let mut doc = Document::with_version("1.7");
    let pages = doc.add_object(dictionary! { "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0 });
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    doc.trailer
        .set("ID", vec![Object::string_literal("0123456789abcdef"); 2]);
    doc
}

fn state(doc: &Document, version: u8, name: &[u8]) -> EncryptionState {
    let filter: Arc<dyn CryptFilter> = match version {
        4 => Arc::new(Aes128CryptFilter),
        5 => Arc::new(Aes256CryptFilter),
        _ => unreachable!(),
    };
    let filters = BTreeMap::from([(b"StdCF".to_vec(), filter)]);
    EncryptionState::try_from(match version {
        4 => EncryptionVersion::V4 {
            document: doc,
            encrypt_metadata: true,
            crypt_filters: filters,
            stream_filter: name.to_vec(),
            string_filter: name.to_vec(),
            owner_password: "owner",
            user_password: "",
            permissions: Permissions::all(),
        },
        5 => EncryptionVersion::V5 {
            encrypt_metadata: true,
            crypt_filters: filters,
            file_encryption_key: &[9; 32],
            stream_filter: name.to_vec(),
            string_filter: name.to_vec(),
            owner_password: "owner",
            user_password: "",
            permissions: Permissions::all(),
        },
        _ => unreachable!(),
    })
    .unwrap()
}

fn payload() -> Object {
    Stream::new(
        dictionary! { "Label" => Object::string_literal("locked.txt") },
        b"plain stream".to_vec(),
    )
    .into()
}

fn assert_plain(object: &Object) {
    let stream = object.as_stream().unwrap();
    assert_eq!(stream.content, b"plain stream");
    assert_eq!(stream.dict.get(b"Label").unwrap().as_str().unwrap(), b"locked.txt");
}

fn writing_preserves_identity(name: &[u8]) {
    for version in [4, 5] {
        let mut doc = document();
        let state = state(&doc, version, name);
        let id = doc.add_object(payload());
        doc.encrypt(&state).unwrap();
        assert_plain(doc.get_object(id).unwrap());
    }
}

#[test]
fn explicit_identity_preserves_plaintext_when_encrypting() {
    writing_preserves_identity(b"Identity");
}

#[test]
fn default_identity_preserves_plaintext_when_encrypting() {
    writing_preserves_identity(b"");
}

fn reading_preserves_identity(omit_names: bool) {
    for version in [4, 5] {
        let mut doc = document();
        let state = state(&doc, version, b"Identity");
        let id = doc.add_object(payload());
        // Build a plaintext payload under the encryption dictionary independently of encrypt().
        let mut encryption = state.encode().unwrap();
        if omit_names {
            encryption.remove(b"StmF");
            encryption.remove(b"StrF");
        }
        let encryption_id = doc.add_object(encryption);
        doc.trailer.set("Encrypt", encryption_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let loaded = Document::load_mem(&bytes).unwrap();
        assert_plain(loaded.get_object(id).unwrap());
    }
}

#[test]
fn explicit_identity_preserves_plaintext_when_loading() {
    reading_preserves_identity(false);
}

#[test]
fn omitted_filter_names_preserve_plaintext_when_loading() {
    reading_preserves_identity(true);
}

#[test]
fn named_aes_filters_still_encrypt_and_decrypt() {
    for version in [4, 5] {
        let doc = document();
        let state = state(&doc, version, b"StdCF");
        let mut object = payload();
        encrypt_object(&state, (10, 0), &mut object).unwrap();
        assert_ne!(object.as_stream().unwrap().content, b"plain stream");
        assert_ne!(
            object
                .as_stream()
                .unwrap()
                .dict
                .get(b"Label")
                .unwrap()
                .as_str()
                .unwrap(),
            b"locked.txt"
        );
        decrypt_object(&state, (10, 0), &mut object).unwrap();
        assert_plain(&object);
    }
}

#[test]
fn legacy_rc4_defaults_still_encrypt_and_decrypt() {
    for version in [1, 2] {
        let doc = document();
        let state = EncryptionState::try_from(match version {
            1 => EncryptionVersion::V1 {
                document: &doc,
                owner_password: "owner",
                user_password: "",
                permissions: Permissions::all(),
            },
            2 => EncryptionVersion::V2 {
                document: &doc,
                owner_password: "owner",
                user_password: "",
                key_length: 128,
                permissions: Permissions::all(),
            },
            _ => unreachable!(),
        })
        .unwrap();
        let mut object = payload();
        encrypt_object(&state, (10, 0), &mut object).unwrap();
        assert_ne!(object.as_stream().unwrap().content, b"plain stream");
        decrypt_object(&state, (10, 0), &mut object).unwrap();
        assert_plain(&object);
    }
}
