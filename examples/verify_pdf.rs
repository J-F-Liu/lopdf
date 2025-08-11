use lopdf::{Document, Object};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <pdf_file>", args[0]);
        std::process::exit(1);
    }

    let pdf_path = &args[1];
    println!("Verifying PDF: {}", pdf_path);

    match Document::load(pdf_path) {
        Ok(doc) => {
            println!("✓ PDF loaded successfully");
            println!("  Version: {:?}", doc.version);
            println!("  Objects: {}", doc.objects.len());
            
            // Count pages
            let pages = doc.get_pages();
            println!("  Pages: {}", pages.len());
            
            // Check for object streams
            let mut obj_stream_count = 0;
            for (_id, obj) in &doc.objects {
                if let Object::Stream(stream) = obj {
                    if let Ok(type_obj) = stream.dict.get(b"Type") {
                        if let Ok(type_name) = type_obj.as_name() {
                            if type_name == b"ObjStm" {
                                obj_stream_count += 1;
                            }
                        }
                    }
                }
            }
            
            if obj_stream_count > 0 {
                println!("  Object streams: {}", obj_stream_count);
            }
            
            println!("\nPDF is valid and can be opened!");
        }
        Err(e) => {
            eprintln!("✗ Failed to load PDF: {}", e);
            std::process::exit(1);
        }
    }
}