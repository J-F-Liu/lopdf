extern crate lopdf;
use lopdf::xobject;
use lopdf::Document;

fn main() {
	let mut doc = Document::load("assets/example.pdf").unwrap();
	let mm2pt = 2.834;
	let barcode = xobject::form(
		vec![0.0, 0.0, 595.0 - 12.44 * mm2pt * 2.0, 10.0 * mm2pt],
		vec![mm2pt, 0.0, 0.0, mm2pt, 12.44 * mm2pt, 842.0 - 14.53 * mm2pt],
		"0 0 0 rg
0 0 9 10 re
f
1 1 1 rg
9 0 9 10 re
f
0 0 0 rg
18 0 9 10 re
f
"
			.as_bytes()
			.to_vec(),
	);
	doc.insert_form_object(1, barcode);
	doc.save("add_barcode.pdf").unwrap();
}
