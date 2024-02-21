// Display a summary of the annotations in a PDF file to the terminal
//
//   Run with `cargo run --example print_annotations <pdf-file>`


use std::env;
use std::process;
use env_logger::Env;
use lopdf::{Document, Object};


fn logging() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init()
}

fn args() -> Vec<String> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <pdf-file>", &args[0]);
        process::exit(1);
    }

    args
}

fn handle_pdf_page(doc: Document) -> u32 {
    let mut page_counter = 1;

    for page in doc.page_iter() {
        for a in doc.get_page_annotations(page) {
            let subtype = a.get_deref(b"Subtype", &doc)
                .and_then(Object::as_name_str)
                .unwrap_or("");
            println!("Page {}, {} annotation at {:?}",
                     page_counter,
                     subtype,
                     a.get_deref(b"Rect", &doc).and_then(Object::as_array).unwrap());
            if let Ok(Object::String(c, _)) = a.get_deref(b"Contents", &doc) {
                println!("  Contents: {:.60}", String::from_utf8_lossy(c).lines().next().unwrap());
            }
            if subtype.eq("Link") {
                if let Ok(ahref) = a.get_deref(b"A", &doc).and_then(Object::as_dict) {
                    print!("  {} -> ", ahref.get_deref(b"S", &doc).and_then(Object::as_name_str).unwrap());
                    if let Ok(d) = ahref.get_deref(b"D", &doc).and_then(Object::as_array) {
                        println!("{:?}", d);
                    } else if let Ok(Object::String(u, _)) = ahref.get_deref(b"URI", &doc) {
                        println!("{}", String::from_utf8_lossy(u));
                    } else if let Ok(n) = ahref.get_deref(b"N", &doc).and_then(Object::as_name_str) {
                        println!("{}", n);
                    }
                }
            }
        }
        page_counter += 1;
    }

    page_counter
}

#[cfg(not(feature = "async"))]
fn main () {
    logging();

    let args: Vec<String> = args();

    match Document::load(&args[1]) {
        Ok(doc) => _ = handle_pdf_page(doc),
        Err(e) => eprintln!("Error opening {:?}: {:?}", &args[1], e),
    }
}

#[cfg(feature = "async")]
#[tokio::main]
async fn main() {
    logging();

    let args: Vec<String> = args();

    match Document::load(&args[1]).await {
        Ok(doc) => _ = handle_pdf_page(doc),
        Err(e) => eprintln!("Error opening {:?}: {:?}", &args[1], e),
    }
}