use std::collections::BTreeMap;

use super::{Dictionary, Document, Object, Result};
use indexmap::IndexMap;
#[derive(Debug, Clone)]
pub struct Destination {
    map: BTreeMap<Vec<u8>, Object>,
}

impl Destination {
    pub fn new(title: Object, page: Object, typ: Object) -> Self {
        let mut map = BTreeMap::new();
        map.insert(b"Title".to_vec(), title);
        map.insert(b"Page".to_vec(), page);
        map.insert(b"Type".to_vec(), typ);
        Destination { map }
    }

    pub fn set(&mut self, key: Vec<u8>, value: Object) {
        self.map.insert(key, value);
    }

    #[allow(dead_code)]
    pub fn title(&self) -> Option<&Object> {
        self.map.get(b"Title".as_slice())
    }

    #[allow(dead_code)]
    pub fn page(&self) -> Option<&Object> {
        self.map.get(b"Page".as_slice())
    }
}

impl Document {
    pub fn get_named_destinations(
        &self, tree: &Dictionary, named_destinations: &mut IndexMap<Vec<u8>, Destination>,
    ) -> Result<()> {
        if let Ok(kids) = tree.get(b"Kids") {
            for kid in kids.as_array()? {
                if let Ok(kid) = kid.as_reference().and_then(move |id| self.get_dictionary(id)) {
                    self.get_named_destinations(kid, named_destinations)?;
                }
            }
        }
        if let Ok(names) = tree.get(b"Names") {
            let mut names = names.as_array()?.iter();
            loop {
                let key = names.next();
                if key.is_none() {
                    break;
                }
                let val = names.next();
                if val.is_none() {
                    break;
                }
                if let Ok(obj_ref) = val.unwrap().as_reference() {
                    if let Ok(dict) = self.get_dictionary(obj_ref) {
                        let val = dict.get(b"D").as_ref().unwrap().as_array()?;
                        let dest = Destination::new(key.unwrap().clone(), val[0].clone(), val[1].clone());
                        named_destinations.insert(key.unwrap().as_str().unwrap().to_vec(), dest);
                    } else if let Ok(Object::Array(val)) = self.get_object(obj_ref) {
                        let dest = Destination::new(key.unwrap().clone(), val[0].clone(), val[1].clone());
                        named_destinations.insert(key.unwrap().as_str().unwrap().to_vec(), dest);
                    }
                } else if let Ok(dict) = val.unwrap().as_dict() {
                    let val = dict.get(b"D").as_ref().unwrap().as_array()?;
                    let dest = Destination::new(key.unwrap().clone(), val[0].clone(), val[1].clone());
                    named_destinations.insert(key.unwrap().as_str().unwrap().to_vec(), dest);
                } else {
                    // TODO: Log error: Unpexpected node type
                }
            }
        }
        Ok(())
    }
}
