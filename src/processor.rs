use super::{Document, Object};

impl Document {
	/// Compress PDF stream objects.
	pub fn compress(&mut self) {
		for (_, object) in self.objects.iter_mut() {
			match *object {
				Object::Stream(ref mut stream) => stream.compress(),
				_ => ()
			}
		}
	}

	/// Decompress PDF stream objects.
	pub fn decompress(&mut self) {
		for (_, object) in self.objects.iter_mut() {
			match *object {
				Object::Stream(ref mut stream) => stream.decompress(),
				_ => ()
			}
		}
	}
}
