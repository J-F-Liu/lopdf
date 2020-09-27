use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Xref {
    /// Entries for indirect object.
    pub entries: BTreeMap<u32, XrefEntry>,

    /// Total number of entries (including free entries), equal to the highest object number plus 1.
    pub size: u32,
}

#[derive(Debug, Clone)]
pub enum XrefEntry {
    Free,
    Normal { offset: u32, generation: u16 },
    Compressed { container: u32, index: u16 },
}

impl Xref {
    pub fn new(size: u32) -> Xref {
        Xref {
            entries: BTreeMap::new(),
            size,
        }
    }

    pub fn get(&self, id: u32) -> Option<&XrefEntry> {
        self.entries.get(&id)
    }

    pub fn insert(&mut self, id: u32, entry: XrefEntry) {
        self.entries.insert(id, entry);
    }

    pub fn extend(&mut self, xref: Xref) {
        for (id, entry) in xref.entries {
            self.entries.entry(id).or_insert(entry);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear()
    }

    pub fn max_id(&self) -> u32 {
        match self.entries.keys().max() {
            Some(&id) => id,
            None => 0,
        }
    }
}

use self::XrefEntry::*;
impl XrefEntry {
    pub fn is_normal(&self) -> bool {
        match *self {
            Normal { .. } => true,
            _ => false,
        }
    }

    pub fn is_compressed(&self) -> bool {
        match *self {
            Compressed { .. } => true,
            _ => false,
        }
    }
}

#[cfg(any(feature = "pom_parser", feature = "nom_parser"))]
pub use crate::parser_aux::decode_xref_stream;
