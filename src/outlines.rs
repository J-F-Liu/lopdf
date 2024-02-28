use indexmap::IndexMap;

use super::{Destination, Dictionary, Document, Error, Object, Result};

pub enum Outline {
    Destination(Destination),
    SubOutlines(Vec<Outline>),
}

fn build_outline_result(
    dest: &Object, title: &Object, named_destinations: &mut IndexMap<Vec<u8>, Destination>,
) -> Result<Option<Outline>> {
    return Ok(Some(match dest {
        Object::Array(ref obj_array) => Outline::Destination(Destination::new(
            title.to_owned(),
            obj_array[0].clone(),
            obj_array[1].clone(),
        )),
        Object::String(ref key, ref _fmt) => {
            if let Some(destination) = named_destinations.get_mut(key) {
                destination.set(b"Title".to_vec(), title.to_owned());
                Outline::Destination(destination.clone())
            } else {
                return Ok(None);
            }
        }
        _ => return Err(Error::Invalid(format!("Unexpected destination {:?}", dest))),
    }));
}

impl Document {
    pub fn get_outline(
        &self, node: &Dictionary, named_destinations: &mut IndexMap<Vec<u8>, Destination>,
    ) -> Result<Option<Outline>> {
        let action = match self.get_dict_in_dict(node, b"A") {
            Ok(a) => a,
            Err(_) => {
                return build_outline_result(node.get(b"Dest")?, node.get(b"Title")?, named_destinations);
            }
        };
        let command = action.get(b"S")?.as_name_str()?;
        if command != "GoTo" && command != "GoToR" {
            return Err(Error::Invalid("Expected GoTo or GoToR".to_string()));
        }
        let title_obj = node.get(b"Title")?;
        let title_ref = match title_obj.as_reference() {
            Ok(o) => o,
            Err(_) => match title_obj.as_str() {
                Ok(_) => return build_outline_result(action.get(b"D")?, title_obj, named_destinations),
                Err(err) => return Err(err),
            },
        };
        build_outline_result(action.get(b"D")?, self.get_object(title_ref)?, named_destinations)
    }

    pub fn get_outlines(
        &self, mut node: Option<Object>, mut outlines: Option<Vec<Outline>>,
        named_destinations: &mut IndexMap<Vec<u8>, Destination>,
    ) -> Result<Option<Vec<Outline>>> {
        if outlines.is_none() {
            outlines = Some(Vec::new());
            let catalog = self.catalog()?;
            let mut dict_node = self.get_dict_in_dict(catalog, b"Outlines")?;
            let first = self.get_dict_in_dict(dict_node, b"First");
            if let Ok(first) = first {
                dict_node = first;
            }
            let mut tree = self.get_dict_in_dict(catalog, b"Dests");
            if tree.is_err() {
                let names = self.get_dict_in_dict(catalog, b"Names");
                if let Ok(names) = names {
                    let dests = self.get_dict_in_dict(names, b"Dests");
                    if dests.is_ok() {
                        tree = dests;
                    }
                }
            }
            if let Ok(tree) = tree {
                self.get_named_destinations(tree, named_destinations)?;
            }
            node = Some(Object::Dictionary(dict_node.clone()));
        }
        if node.is_none() {
            return Ok(outlines);
        }
        let node = node.unwrap();
        let mut node = match node.as_dict() {
            Ok(n) => n,
            Err(_) => self.get_object(node.as_reference()?)?.as_dict()?,
        };
        loop {
            if let Ok(Some(outline)) = self.get_outline(node, named_destinations) {
                if let Some(ref mut outlines) = outlines {
                    outlines.push(outline);
                }
            }
            if let Ok(first) = node.get(b"First") {
                let sub_outlines = Vec::new();
                let sub_outlines = self.get_outlines(Some(first.clone()), Some(sub_outlines), named_destinations)?;
                if let Some(sub_outlines) = sub_outlines {
                    if !sub_outlines.is_empty() {
                        if let Some(ref mut outlines) = outlines {
                            outlines.push(Outline::SubOutlines(sub_outlines));
                        }
                    }
                }
            }
            node = match self.get_dict_in_dict(node, b"Next") {
                Ok(n) => n,
                Err(_) => break,
            };
        }
        Ok(outlines)
    }
}
