use super::Object::*;
use super::{Dictionary, Stream};

#[cfg(feature = "embed_image")]
use image::{self, ColorType, GenericImageView, ImageFormat};
#[cfg(feature = "embed_image")]
use std::path::Path;

pub fn form(boundingbox: Vec<f64>, matrix: Vec<f64>, content: Vec<u8>) -> Stream {
	let mut dict = Dictionary::new();
	dict.set("Type", Name(b"XObject".to_vec()));
	dict.set("Subtype", Name(b"Form".to_vec()));
	dict.set("BBox", Array(boundingbox.into_iter().map(Real).collect()));
	dict.set("Matrix", Array(matrix.into_iter().map(Real).collect()));
	let mut xobject = Stream::new(dict, content);
	// Ignore any compression error.
	let _ = xobject.compress();
	xobject
}

#[cfg(feature = "embed_image")]
pub fn image<P: AsRef<Path>>(path: P) -> Result<Stream> {
	use std::fs::File;
	use std::io::prelude::*;
	let img = image::open(&path)?;
	let (width, height) = img.dimensions();
	let (color_space, bits) = match img.color() {
		ColorType::Gray(bits) => (b"DeviceGray".to_vec(), bits),
		ColorType::RGB(bits) => (b"DeviceRGB".to_vec(), bits),
		ColorType::Palette(bits) => (b"Indexed".to_vec(), bits),
		ColorType::GrayA(bits) => (b"DeviceN".to_vec(), bits),
		ColorType::RGBA(bits) => (b"DeviceN".to_vec(), bits),
		ColorType::BGR(bits) => (b"DeviceN".to_vec(), bits),
		ColorType::BGRA(bits) => (b"DeviceN".to_vec(), bits),
	};

	let mut dict = Dictionary::new();
	dict.set("Type", Name(b"XObject".to_vec()));
	dict.set("Subtype", Name(b"Image".to_vec()));
	dict.set("Width", width);
	dict.set("Height", height);
	dict.set("ColorSpace", Name(color_space));
	dict.set("BitsPerComponent", bits);

	let mut file = File::open(&path)?;
	let mut buffer = Vec::new();
	file.read_to_end(&mut buffer)?;

	let is_jpeg = match image::guess_format(&buffer) {
		Ok(format) => match format {
			ImageFormat::JPEG => true,
			_ => false,
		},
		Err(_) => false,
	};

	if is_jpeg {
		dict.set("Filter", Name(b"DCTDecode".to_vec()));
		Ok(Stream::new(dict, buffer))
	} else {
		let mut img_object = Stream::new(dict, img.raw_pixels());
		// Ignore any compression error.
		let _ = img_object.compress();
		Ok(img_object)
	}
}

#[cfg(feature = "embed_image")]
#[test]
fn insert_image() {
	use super::xobject;
	let mut doc = Document::load("assets/example.pdf").unwrap();
	let pages = doc.get_pages();
	let page_id = *pages.get(&1).expect(&format!("Page {} not exist.", 1));
	let img = xobject::image("assets/pdf_icon.jpg").unwrap();
	doc.insert_image(page_id, img, (100.0, 210.0), (400.0, 225.0)).unwrap();
	doc.save("test_5_image.pdf").unwrap();
}
