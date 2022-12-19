use std::fmt::Debug;
use std::fs::File;
use std::io::{Error, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;
use lopdf::{Document, Object};
use serde_json;
use shellexpand;

static IGNORE: &[&str] = &[
    "Length",
    "BBox",
    "FormType",
    "Matrix",
    "Resources",
    "Type",
    "XObject",
    "Subtype",
    "Filter",
    "ColorSpace",
    "Width",
    "Height",
    "BitsPerComponent",
    "Length1",
    "Length2",
    "Length3",
    "PTEX.FileName",
    "PTEX.PageNumber",
    "PTEX.InfoDict",
    "FontDescriptor",
    "ExtGState",
    "Font",
    "MediaBox",
    "Annot",
];

#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about,
    long_about = "Extract TOC and write to file.",
    arg_required_else_help = true
)]
pub struct Args {
    pub pdf_path: PathBuf,

    /// Optional output directory. If omitted the directory of the PDF file will be used.
    #[clap(short, long)]
    pub output: Option<PathBuf>,

    /// Optional pretty print output.
    #[clap(short, long)]
    pub pretty: bool,
}

impl Args {
    pub fn parse_args() -> Self {
        Args::parse()
    }
}

fn filter_func(object_id: (u32, u16), object: &mut Object) -> Option<((u32, u16), Object)> {
    if IGNORE.contains(&object.type_name().unwrap_or_default()) {
        return None;
    }
    if let Ok(d) = object.as_dict_mut() {
        d.remove(b"Font");
        d.remove(b"Resources");
        d.remove(b"Producer");
        d.remove(b"ModDate");
        d.remove(b"Creator");
        d.remove(b"ProcSet");
        d.remove(b"XObject");
        d.remove(b"MediaBox");
        d.remove(b"Annots");
        if d.is_empty() {
            return None;
        }
    }
    Some((object_id, object.to_owned()))
}

fn load_pdf<P: AsRef<Path>>(path: P) -> Result<Document, Error> {
    Document::load_filtered(path, filter_func).map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
}

fn pdf2toc<P: AsRef<Path> + Debug>(path: P, output: P, pretty: bool) -> Result<(), Error> {
    println!("Load {path:?}");
    let doc = load_pdf(&path)?;
    if doc.is_encrypted() {
        return Err(Error::new(ErrorKind::InvalidInput, "Password missing!"));
    }
    let toc = doc.get_toc().map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    if !toc.errors.is_empty() {
        eprintln!("{path:?} has {} errors:", toc.errors.len());
        for error in &toc.errors[..10] {
            eprintln!("{error:?}");
        }
    }
    let data = match pretty {
        true => serde_json::to_string_pretty(&toc).unwrap(),
        false => serde_json::to_string(&toc).unwrap(),
    };
    println!("Write {output:?}");
    let mut f = File::create(output)?;
    f.write_all(data.as_bytes())?;
    Ok(())
}

fn main() -> Result<(), Error> {
    let args = Args::parse_args();

    let start_time = Instant::now();
    let pdf_path = PathBuf::from(shellexpand::full(args.pdf_path.to_str().unwrap()).unwrap().to_string());
    let output = match args.output {
        Some(o) => o.join(pdf_path.file_name().unwrap()),
        None => args.pdf_path,
    };
    let mut output = PathBuf::from(shellexpand::full(output.to_str().unwrap()).unwrap().to_string());
    output.set_extension("toc");
    pdf2toc(&pdf_path, &output, args.pretty)?;
    println!(
        "Done after {:.1} seconds.",
        Instant::now().duration_since(start_time).as_secs_f64()
    );
    Ok(())
}
