use super::{Dictionary, Document, Object, Result};
use indexmap::IndexMap;
#[derive(Debug, Clone)]
pub struct Destination(Dictionary);

impl Destination {
    pub fn new(title: Object, page: Object, typ: Object) -> Self {
        let mut dict = Dictionary::new();
        dict.set(b"Title", title);
        dict.set(b"Page", page);
        dict.set(b"Type", typ);
        Destination(dict)
    }

    pub fn set<K, V>(&mut self, key: K, value: V)
    where
        K: Into<Vec<u8>>,
        V: Into<Object>,
    {
        self.0.set(key, value);
    }

    pub fn title(&self) -> Result<&Object> {
        self.0.get(b"Title")
    }

    pub fn page(&self) -> Result<&Object> {
        self.0.get(b"Page")
    }
}

impl Document {
    pub fn get_named_destinations(
        &self, tree: &Dictionary, named_destinations: &mut IndexMap<Vec<u8>, Destination>,
    ) -> Result<()> {
        self.get_named_destinations_impl(tree, named_destinations, 0)
    }

    fn get_named_destinations_impl(
        &self, tree: &Dictionary, named_destinations: &mut IndexMap<Vec<u8>, Destination>, depth: usize,
    ) -> Result<()> {
        if depth >= crate::reader::MAX_NESTING_DEPTH {
            return Err(crate::Error::RecursionLimit);
        }
        if let Ok(kids) = tree.get(b"Kids") {
            for kid in kids.as_array()? {
                if let Ok(kid) = kid.as_reference().and_then(move |id| self.get_dictionary(id)) {
                    self.get_named_destinations_impl(kid, named_destinations, depth + 1)?;
                }
            }
        }
        if let Ok(names) = tree.get(b"Names") {
            for [key, val] in names.as_array()?.as_chunks::<2>().0 {
                // Skip entries that aren't a string key with a usable destination.
                let Ok(name) = key.as_str() else {
                    continue;
                };
                let val = match val {
                    Object::Reference(id) => match self.get_object(*id) {
                        Ok(val) => val,
                        Err(_) => continue,
                    },
                    val => val,
                };
                let array = match val {
                    Object::Array(array) => array,
                    Object::Dictionary(dict) => match dict.get(b"D").and_then(Object::as_array) {
                        Ok(array) => array,
                        Err(_) => continue,
                    },
                    _ => continue,
                };
                if let [page, typ, ..] = array.as_slice() {
                    let dest = Destination::new(key.clone(), page.clone(), typ.clone());
                    named_destinations.insert(name.to_vec(), dest);
                }
            }
        }
        Ok(())
    }
}
