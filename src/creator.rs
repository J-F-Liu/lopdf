use crate::{Dictionary, Document, Object, ObjectId};
use crate::{Error, Result};

impl Document {
    /// Create new PDF document with version.
    pub fn with_version<S: Into<String>>(version: S) -> Document {
        let mut document = Self::new();
        document.version = version.into();
        document
    }

    /// Create an object ID.
    pub fn new_object_id(&mut self) -> ObjectId {
        self.max_id += 1;
        (self.max_id, 0)
    }

    /// Add PDF object into document's object list.
    pub fn add_object<T: Into<Object>>(&mut self, object: T) -> ObjectId {
        self.max_id += 1;
        let id = (self.max_id, 0);
        self.objects.insert(id, object.into());
        id
    }

    pub fn set_object<T: Into<Object>>(&mut self, id: ObjectId, object: T) {
        self.objects.insert(id, object.into());
    }

    /// Remove PDF object from document's object list.
    pub fn remove_object(&mut self, object_id: &ObjectId) -> Result<()> {
        for (_, page_id) in self.get_pages() {
            let page = self.get_object_mut(page_id)?.as_dict_mut()?;
            let annots = page.get_mut(b"Annots")?.as_array_mut()?;

            annots.retain(|object| {
                if let Ok(id) = object.as_reference() {
                    return id != *object_id;
                }

                true
            });
        }

        Ok(())
    }

    /// Get the pages resources. (Errors in case of reference)
    ///
    /// Get Object that has the key "Resources".
    fn get_or_create_resources_mut(&mut self, page_id: ObjectId) -> Result<&mut Object> {
        let page = self.get_object_mut(page_id).and_then(Object::as_dict_mut)?;
        if page.has(b"Resources") {
            if let Ok(_res_id) = page.get(b"Resources").and_then(Object::as_reference) {
                // Find and return referenced object.
                // Note: This returns an error because we can not have 2 mut barrow for `*self`.
                // self.get_object_mut(res_id)
                Err(Error::ObjectNotFound)
            } else {
                // It exists and is not a reference.
                page.get_mut(b"Resources")
            }
        } else {
            // "Resources" key does not exist, So create it.
            page.set("Resources", Dictionary::new());
            page.get_mut(b"Resources")
        }
    }

    /// Get the pages resources.
    ///
    /// Get Object that has the key "Resources".
    pub fn get_or_create_resources(&mut self, page_id: ObjectId) -> Result<&mut Object> {
        let resources_id = {
            let page = self.get_object(page_id).and_then(Object::as_dict)?;
            if page.has(b"Resources") {
                page.get(b"Resources").and_then(Object::as_reference).ok()
            } else {
                None
            }
        };
        match resources_id {
            Some(res_id) => self.get_object_mut(res_id),
            None => self.get_or_create_resources_mut(page_id),
        }
    }

    /// Add XObject to a page.
    ///
    /// Get Object that has the key `Resources -> XObject`.
    pub fn add_xobject<N: Into<Vec<u8>>>(
        &mut self, page_id: ObjectId, xobject_name: N, xobject_id: ObjectId,
    ) -> Result<()> {
        if let Ok(resources) = self.get_or_create_resources(page_id).and_then(Object::as_dict_mut) {
            if !resources.has(b"XObject") {
                resources.set("XObject", Dictionary::new());
            }
            let xobjects = resources.get_mut(b"XObject").and_then(Object::as_dict_mut)?;
            xobjects.set(xobject_name, Object::Reference(xobject_id));
        }
        Ok(())
    }

    /// Add Graphics State to a page.
    ///
    /// Get Object that has the key `Resources -> ExtGState`.
    pub fn add_graphics_state<N: Into<Vec<u8>>>(
        &mut self, page_id: ObjectId, gs_name: N, gs_id: ObjectId,
    ) -> Result<()> {
        if let Ok(resources) = self.get_or_create_resources(page_id).and_then(Object::as_dict_mut) {
            if !resources.has(b"ExtGState") {
                resources.set("ExtGState", Dictionary::new());
            }
            let states = resources.get_mut(b"ExtGState").and_then(Object::as_dict_mut)?;
            states.set(gs_name, Object::Reference(gs_id));
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use std::path::PathBuf;

    use crate::content::*;
    use crate::{Document, Object, Stream};

    /// Create and return a document for testing
    pub fn create_document() -> Document {
        let mut doc = Document::with_version("1.5");
        let info_id = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Create PDF document example"),
            "Creator" => Object::string_literal("https://crates.io/crates/lopdf"),
            "CreationDate" => time::OffsetDateTime::now_utc(),
        });
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
        });
        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.trailer.set("Info", info_id);
        doc.compress();

        doc
    }

    /// Save a document
    pub fn save_document(file_path: &PathBuf, doc: &mut Document) {
        let res = doc.save(file_path);

        assert!(match res {
            Ok(_file) => true,
            Err(_e) => false,
        });
    }

    #[test]
    fn save_created_document() {
        // Create temporary folder to store file.
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_1_create.pdf");

        let mut doc = create_document();
        // Save to file
        save_document(&file_path, &mut doc);
        assert!(file_path.exists());
    }
}
