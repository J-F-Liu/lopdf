use std::collections::BTreeMap;
use std::io::{Result, Write};

#[derive(Debug, Clone)]
pub struct Xref {
    /// Type of Cross-Reference used in the last incremental version.
    /// This method of cross-referencing will also be used when saving the file.
    /// PDFs with Incremental Updates should alway use the same cross-reference type.
    pub cross_reference_type: XrefType,

    /// Entries for indirect object.
    pub entries: BTreeMap<u32, XrefEntry>,

    /// Total number of entries (including free entries), equal to the highest object number plus 1.
    pub size: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum XrefType {
    /// Cross-Reference Streams are supported beginning with PDF 1.5.
    CrossReferenceStream,
    /// Cross-Reference Table is older but still frequently used.
    CrossReferenceTable,
}

#[derive(Debug, Clone)]
pub enum XrefEntry {
    Free, // TODO add generation number
    UnusableFree,
    Normal { offset: u32, generation: u16 },
    Compressed { container: u32, index: u16 },
}

#[derive(Debug, Clone)]
pub struct XrefSection {
    pub starting_id: u32,
    pub entries: Vec<XrefEntry>,
}

impl Xref {
    pub fn new(size: u32, xref_type: XrefType) -> Xref {
        Xref {
            cross_reference_type: xref_type,
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

    /// Combine Xref entries. Only add them if they do not exists already.
    /// Do not replace existing entries.
    pub fn merge(&mut self, xref: Xref) {
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

impl XrefEntry {
    pub fn is_normal(&self) -> bool {
        matches!(*self, XrefEntry::Normal { .. })
    }

    pub fn is_compressed(&self) -> bool {
        matches!(*self, XrefEntry::Compressed { .. })
    }

    /// Write Entry in Cross Reference Table.
    pub fn write_xref_entry(&self, file: &mut dyn Write) -> Result<()> {
        match self {
            XrefEntry::Normal { offset, generation } => {
                writeln!(file, "{:>010} {:>05} n ", offset, generation)?;
            }
            XrefEntry::Compressed { container: _, index: _ } => {
                writeln!(file, "{:>010} {:>05} f ", 0, 65535)?;
            }
            XrefEntry::Free => {
                writeln!(file, "{:>010} {:>05} f ", 0, 0)?;
            }
            XrefEntry::UnusableFree => {
                writeln!(file, "{:>010} {:>05} f ", 0, 65535)?;
            }
        }
        Ok(())
    }
}

impl XrefSection {
    pub fn new(starting_id: u32) -> Self {
        XrefSection {
            starting_id,
            entries: Vec::new(),
        }
    }

    pub fn add_entry(&mut self, entry: XrefEntry) {
        self.entries.push(entry);
    }

    pub fn add_unusable_free_entry(&mut self) {
        self.add_entry(XrefEntry::UnusableFree);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Write Section in Cross Reference Table.
    pub fn write_xref_section(&self, file: &mut dyn Write) -> Result<()> {
        if !self.is_empty() {
            // Write section range
            writeln!(file, "{} {}", self.starting_id, self.entries.len())?;
            // Write entries
            for entry in &self.entries {
                entry.write_xref_entry(file)?;
            }
        }
        Ok(())
    }
}

#[cfg(feature = "nom_parser")]
pub use crate::parser_aux::decode_xref_stream;
