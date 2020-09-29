use std::fs::File;
use std::io::{BufWriter, Result, Write};
use std::path::Path;

use super::Object::*;
use super::{Dictionary, Document, Object, Stream, StringFormat};
use crate::xref::*;

impl Document {
    /// Save PDF document to specified file path.
    #[inline]
    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<File> {
        let mut file = BufWriter::new(File::create(path)?);
        self.save_internal(&mut file)?;
        Ok(file.into_inner()?)
    }

    /// Save PDF to arbitrary target
    #[inline]
    pub fn save_to<W: Write>(&mut self, target: &mut W) -> Result<()> {
        self.save_internal(target)
    }

    fn save_internal<W: Write>(&mut self, target: &mut W) -> Result<()> {
        let mut target = CountingWrite {
            inner: target,
            bytes_written: 0,
        };
        let mut xref = Xref::new(self.max_id + 1);
        writeln!(target, "%PDF-{}", self.version)?;

        for (&(id, generation), object) in &self.objects {
            if object
                .type_name()
                .map(|name| ["ObjStm", "XRef", "Linearized"].contains(&name))
                .ok()
                != Some(true)
            {
                Writer::write_indirect_object(&mut target, id, generation, object, &mut xref)?;
            }
        }

        let xref_start = target.bytes_written;
        Writer::write_xref(&mut target, &xref)?;
        self.write_trailer(&mut target)?;
        write!(target, "\nstartxref\n{}\n%%EOF", xref_start)?;

        Ok(())
    }

    fn write_trailer(&mut self, file: &mut dyn Write) -> Result<()> {
        self.trailer.set("Size", i64::from(self.max_id + 1));
        file.write_all(b"trailer\n")?;
        Writer::write_dictionary(file, &self.trailer)?;
        Ok(())
    }
}

pub struct Writer;

impl Writer {
    fn need_separator(object: &Object) -> bool {
        match *object {
            Null => true,
            Boolean(_) => true,
            Integer(_) => true,
            Real(_) => true,
            Reference(_) => true,
            _ => false,
        }
    }

    fn need_end_separator(object: &Object) -> bool {
        match *object {
            Null => true,
            Boolean(_) => true,
            Integer(_) => true,
            Real(_) => true,
            Name(_) => true,
            Reference(_) => true,
            Object::Stream(_) => true,
            _ => false,
        }
    }

    fn write_xref(file: &mut dyn Write, xref: &Xref) -> Result<()> {
        writeln!(file, "xref\n0 {}", xref.size)?;

        let mut write_xref_entry =
            |offset: u32, generation: u16, kind: char| writeln!(file, "{:>010} {:>05} {} ", offset, generation, kind);
        write_xref_entry(0, 65535, 'f')?;

        let mut obj_id = 1;
        while obj_id < xref.size {
            if let Some(entry) = xref.get(obj_id) {
                if let XrefEntry::Normal { offset, generation } = *entry {
                    write_xref_entry(offset, generation, 'n')?;
                };
            } else {
                write_xref_entry(0, 65535, 'f')?;
            }
            obj_id += 1;
        }
        Ok(())
    }

    fn write_indirect_object<W: Write>(
        file: &mut CountingWrite<&mut W>, id: u32, generation: u16, object: &Object, xref: &mut Xref,
    ) -> Result<()> {
        let offset = file.bytes_written as u32;
        xref.insert(id, XrefEntry::Normal { offset, generation });
        write!(
            file,
            "{} {} obj{}",
            id,
            generation,
            if Writer::need_separator(object) { " " } else { "" }
        )?;
        Writer::write_object(file, object)?;
        writeln!(
            file,
            "{}endobj",
            if Writer::need_end_separator(object) { " " } else { "" }
        )?;
        Ok(())
    }

    pub fn write_object(file: &mut dyn Write, object: &Object) -> Result<()> {
        match *object {
            Null => file.write_all(b"null"),
            Boolean(ref value) => {
                if *value {
                    file.write_all(b"true")
                } else {
                    file.write_all(b"false")
                }
            }
            Integer(ref value) => {
                let _ = itoa::write(file, *value);
                Ok(())
            }
            Real(ref value) => file.write_all(format!("{:.02?}", *value).as_bytes()),
            Name(ref name) => Writer::write_name(file, name),
            String(ref text, ref format) => Writer::write_string(file, text, format),
            Array(ref array) => Writer::write_array(file, array),
            Object::Dictionary(ref dict) => Writer::write_dictionary(file, dict),
            Object::Stream(ref stream) => Writer::write_stream(file, stream),
            Reference(ref id) => write!(file, "{} {} R", id.0, id.1),
        }
    }

    fn write_name(file: &mut dyn Write, name: &[u8]) -> Result<()> {
        file.write_all(b"/")?;
        for &byte in name {
            // white-space and delimiter chars are encoded to # sequences
            // also encode bytes outside of the range 33 (!) to 126 (~)
            if b" \t\n\r\x0C()<>[]{}/%#".contains(&byte) || byte < 33 || byte > 126 {
                write!(file, "#{:02X}", byte)?;
            } else {
                file.write_all(&[byte])?;
            }
        }
        Ok(())
    }

    fn write_string(file: &mut dyn Write, text: &[u8], format: &StringFormat) -> Result<()> {
        match *format {
            // Within a Literal string, backslash (\) and unbalanced parentheses should be escaped.
            // This rule apply to each individual byte in a string object,
            // whether the string is interpreted as single-byte or multiple-byte character codes.
            // If an end-of-line marker appears within a literal string without a preceding backslash, the result is equivalent to \n.
            // So \r also need be escaped.
            StringFormat::Literal => {
                let mut escape_indice = Vec::new();
                let mut parentheses = Vec::new();
                for (index, &byte) in text.iter().enumerate() {
                    match byte {
                        b'(' => parentheses.push(index),
                        b')' => {
                            if !parentheses.is_empty() {
                                parentheses.pop();
                            } else {
                                escape_indice.push(index);
                            }
                        }
                        b'\\' | b'\r' => escape_indice.push(index),
                        _ => continue,
                    }
                }
                escape_indice.append(&mut parentheses);

                file.write_all(b"(")?;
                if !escape_indice.is_empty() {
                    for (index, &byte) in text.iter().enumerate() {
                        if escape_indice.contains(&index) {
                            file.write_all(b"\\")?;
                            file.write_all(&[if byte == b'\r' { b'r' } else { byte }])?;
                        } else {
                            file.write_all(&[byte])?;
                        }
                    }
                } else {
                    file.write_all(text)?;
                }
                file.write_all(b")")?;
            }
            StringFormat::Hexadecimal => {
                file.write_all(b"<")?;
                for &byte in text {
                    write!(file, "{:02X}", byte)?;
                }
                file.write_all(b">")?;
            }
        }
        Ok(())
    }

    fn write_array(file: &mut dyn Write, array: &[Object]) -> Result<()> {
        file.write_all(b"[")?;
        let mut first = true;
        for object in array {
            if first {
                first = false;
            } else if Writer::need_separator(object) {
                file.write_all(b" ")?;
            }
            Writer::write_object(file, object)?;
        }
        file.write_all(b"]")?;
        Ok(())
    }

    fn write_dictionary(file: &mut dyn Write, dictionary: &Dictionary) -> Result<()> {
        file.write_all(b"<<")?;
        for (key, value) in dictionary {
            Writer::write_name(file, key)?;
            if Writer::need_separator(value) {
                file.write_all(b" ")?;
            }
            Writer::write_object(file, value)?;
        }
        file.write_all(b">>")?;
        Ok(())
    }

    fn write_stream(file: &mut dyn Write, stream: &Stream) -> Result<()> {
        Writer::write_dictionary(file, &stream.dict)?;
        file.write_all(b"stream\n")?;
        file.write_all(&stream.content)?;
        file.write_all(b"endstream")?;
        Ok(())
    }
}

pub struct CountingWrite<W: Write> {
    inner: W,
    bytes_written: usize,
}

impl<W: Write> Write for CountingWrite<W> {
    #[inline]
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        let result = self.inner.write(buffer);
        if let Ok(bytes) = result {
            self.bytes_written += bytes;
        }
        result
    }

    #[inline]
    fn write_all(&mut self, buffer: &[u8]) -> Result<()> {
        self.bytes_written += buffer.len();
        // If this returns `Err` we can’t know how many bytes were actually written (if any)
        // but that doesn’t matter since we’re gonna abort the entire PDF generation anyway.
        self.inner.write_all(buffer)
    }

    #[inline]
    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

#[test]
fn save_document() {
    let mut doc = Document::with_version("1.5");
    doc.objects.insert((1, 0), Null);
    doc.objects.insert((2, 0), Boolean(true));
    doc.objects.insert((3, 0), Integer(3));
    doc.objects.insert((4, 0), Real(0.5));
    doc.objects
        .insert((5, 0), String("text((\r)".as_bytes().to_vec(), StringFormat::Literal));
    doc.objects.insert(
        (6, 0),
        String("text((\r)".as_bytes().to_vec(), StringFormat::Hexadecimal),
    );
    doc.objects.insert((7, 0), Name(b"name \t".to_vec()));
    doc.objects.insert((8, 0), Reference((1, 0)));
    doc.objects
        .insert((9, 2), Array(vec![Integer(1), Integer(2), Integer(3)]));
    doc.objects
        .insert((11, 0), Stream(Stream::new(Dictionary::new(), vec![0x41, 0x42, 0x43])));
    let mut dict = Dictionary::new();
    dict.set("A", Null);
    dict.set("B", false);
    dict.set("C", Name(b"name".to_vec()));
    doc.objects.insert((12, 0), Object::Dictionary(dict));
    doc.max_id = 12;

    doc.save("test_0_save.pdf").unwrap();
}
