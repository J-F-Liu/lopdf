use std::collections::BTreeMap;

pub struct Xref {
	pub entries: BTreeMap<u32, XrefEntry>
}

pub enum XrefEntry {
	Free,
	Normal { offset: u32, generation: u16 },
	Compressed { container: u32, index: u16 },
}

impl Xref {
	pub fn new() -> Xref {
		Xref { entries: BTreeMap::new() }
	}

	pub fn get(&self, id: u32) -> Option<&XrefEntry> {
		self.entries.get(&id)
	}

	pub fn insert(&mut self, id: u32, entry: XrefEntry) {
		self.entries.insert(id, entry);
	}

	pub fn clear(&mut self) {
		self.entries.clear()
	}
}
