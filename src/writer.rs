use std::fs::File;
use std::io::{BufWriter, Result, Write};
use std::path::Path;
use std::vec;

use super::Object::*;
use super::{Dictionary, Document, Object, Stream, StringFormat};
use crate::{xref::*, IncrementalDocument};

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

        let mut xref = Xref::new(self.max_id + 1, self.reference_table.cross_reference_type);
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

        // Pick right cross reference stream.
        match xref.cross_reference_type {
            XrefType::CrossReferenceTable => {
                Writer::write_xref(&mut target, &xref)?;
                self.write_trailer(&mut target)?;
            }
            XrefType::CrossReferenceStream => {
                // Cross Reference Stream instead of XRef and Trailer
                self.write_cross_reference_stream(&mut target, &mut xref, xref_start as u32)?;
            }
        }
        // Write `startxref` part of trailer
        write!(target, "\nstartxref\n{}\n%%EOF", xref_start)?;

        Ok(())
    }

    /// Write the Cross Reference Stream.
    ///
    /// Insert an `Object` to the end of the PDF (not visible when inspecting `Document`).
    /// Note: This is different from the "Cross Reference Table".
    fn write_cross_reference_stream<W: Write>(
        &mut self, file: &mut CountingWrite<&mut W>, xref: &mut Xref, xref_start: u32,
    ) -> Result<()> {
        // Increment max_id to account for CRS.
        self.max_id += 1;
        let new_obj_id_for_crs = self.max_id;
        xref.insert(
            new_obj_id_for_crs,
            XrefEntry::Normal {
                offset: xref_start,
                generation: 0,
            },
        );
        self.trailer.set("Type", Name(b"XRef".to_vec()));
        // Update `max_id` in trailer
        self.trailer.set("Size", i64::from(self.max_id + 1));
        // Set the size of each entry in bytes (default for PDFs is `[1 2 1]`)
        // In our case we use `[u8, u32, u16]` for each entry
        // to keep things simple and working at all times.
        self.trailer.set("W", Array(vec![Integer(1), Integer(4), Integer(2)]));
        // Note that `ASCIIHexDecode` does not work correctly,
        // but is still useful for debugging sometimes.
        let filter = XRefStreamFilter::None;
        let (stream, stream_length, indexes) = Writer::create_xref_steam(xref, filter)?;
        self.trailer.set("Index", indexes);

        if filter == XRefStreamFilter::ASCIIHexDecode {
            self.trailer.set("Filter", Name(b"ASCIIHexDecode".to_vec()));
        } else {
            self.trailer.remove(b"Filter");
        }

        self.trailer.set("Length", stream_length as i64);

        let trailer = &self.trailer;
        let cross_reference_stream = Stream(Stream {
            dict: trailer.clone(),
            allows_compression: true,
            content: stream,
            start_position: None,
        });
        // Insert Cross Reference Stream as an `Object` to the end of the PDF.
        // The `Object` is not added to `Document` because it is generated every time you save.
        Writer::write_indirect_object(file, new_obj_id_for_crs, 0, &cross_reference_stream, xref)?;

        Ok(())
    }

    fn write_trailer(&mut self, file: &mut dyn Write) -> Result<()> {
        self.trailer.set("Size", i64::from(self.max_id + 1));
        file.write_all(b"trailer\n")?;
        Writer::write_dictionary(file, &self.trailer)?;
        Ok(())
    }
}

impl IncrementalDocument {
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

        // Write previous document versions.
        let prev_document_bytes = self.get_prev_documents_bytes();
        target.inner.write_all(prev_document_bytes)?;
        target.bytes_written += prev_document_bytes.len();

        // Write/Append new document version.
        let mut xref = Xref::new(
            self.new_document.max_id + 1,
            self.get_prev_documents().reference_table.cross_reference_type,
        );

        if let Some(last_byte) = prev_document_bytes.last() {
            if *last_byte != b'\n' {
                // Add a newline if it was not already present
                writeln!(target)?;
            }
        }
        writeln!(target, "%PDF-{}", self.new_document.version)?;

        for (&(id, generation), object) in &self.new_document.objects {
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

        // Pick right cross reference stream.
        match xref.cross_reference_type {
            XrefType::CrossReferenceTable => {
                Writer::write_xref(&mut target, &xref)?;
                self.new_document.write_trailer(&mut target)?;
            }
            XrefType::CrossReferenceStream => {
                // Cross Reference Stream instead of XRef and Trailer
                self.new_document
                    .write_cross_reference_stream(&mut target, &mut xref, xref_start as u32)?;
            }
        }
        // Write `startxref` part of trailer
        write!(target, "\nstartxref\n{}\n%%EOF", xref_start)?;

        Ok(())
    }
}

pub struct Writer;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum XRefStreamFilter {
    ASCIIHexDecode,
    _FlateDecode, //this is generally a Zlib compressed Stream.
    None,
}

impl Writer {
    fn need_separator(object: &Object) -> bool {
        matches!(*object, Null | Boolean(_) | Integer(_) | Real(_) | Reference(_))
    }

    fn need_end_separator(object: &Object) -> bool {
        matches!(
            *object,
            Null | Boolean(_) | Integer(_) | Real(_) | Name(_) | Reference(_) | Object::Stream(_)
        )
    }

    /// Write Cross Reference Table.
    ///
    /// Note: This is different from a "Cross Reference Stream".
    fn write_xref(file: &mut dyn Write, xref: &Xref) -> Result<()> {
        writeln!(file, "xref")?;

        let mut xref_section = XrefSection::new(0);
        // Add first (0) entry
        xref_section.add_unusable_free_entry();

        for obj_id in 1..xref.size {
            // If section is empty change number of starting id.
            if xref_section.is_empty() {
                xref_section = XrefSection::new(obj_id);
            }
            if let Some(entry) = xref.get(obj_id) {
                match *entry {
                    XrefEntry::Normal { offset, generation } => {
                        // Add entry
                        xref_section.add_entry(XrefEntry::Normal { offset, generation });
                    }
                    XrefEntry::Compressed { container: _, index: _ } => {
                        xref_section.add_unusable_free_entry();
                    }
                    XrefEntry::Free => {
                        xref_section.add_entry(XrefEntry::Free);
                    }
                    XrefEntry::UnusableFree => {
                        xref_section.add_unusable_free_entry();
                    }
                }
            } else {
                // Skip over `obj_id`, but finish section if not empty.
                if !xref_section.is_empty() {
                    xref_section.write_xref_section(file)?;
                    xref_section = XrefSection::new(obj_id);
                }
            }
        }
        // Print last section
        if !xref_section.is_empty() {
            xref_section.write_xref_section(file)?;
        }
        Ok(())
    }

    /// Create stream for Cross reference stream.
    fn create_xref_steam(xref: &Xref, filter: XRefStreamFilter) -> Result<(Vec<u8>, usize, Object)> {
        let mut xref_sections = Vec::new();
        let mut xref_section = XrefSection::new(0);

        for obj_id in 1..xref.size + 1 {
            // If section is empty change number of starting id.
            if xref_section.is_empty() {
                xref_section = XrefSection::new(obj_id);
            }
            if let Some(entry) = xref.get(obj_id) {
                xref_section.add_entry(entry.clone());
            } else {
                // Skip over but finish section if not empty
                if !xref_section.is_empty() {
                    xref_sections.push(xref_section);
                    xref_section = XrefSection::new(obj_id);
                }
            }
        }
        // Print last section
        if !xref_section.is_empty() {
            xref_sections.push(xref_section);
        }

        let mut xref_stream = Vec::new();
        let mut xref_index = Vec::new();

        for section in xref_sections {
            // Add indexes to list
            xref_index.push(Integer(section.starting_id as i64));
            xref_index.push(Integer(section.entries.len() as i64));
            // Add entries to stream
            let mut obj_id = section.starting_id;
            for entry in section.entries {
                match entry {
                    XrefEntry::Free => {
                        // Type 0
                        xref_stream.push(0); // Type 0
                        xref_stream.extend(obj_id.to_be_bytes());
                        xref_stream.extend(vec![0, 0]); // TODO add generation number
                    }
                    XrefEntry::UnusableFree => {
                        // Type 0
                        xref_stream.push(0); // Type 0
                        xref_stream.extend(obj_id.to_be_bytes());
                        xref_stream.extend(65535_u16.to_be_bytes());
                    }
                    XrefEntry::Normal { offset, generation } => {
                        // Type 1
                        xref_stream.push(1); // Type 1
                        xref_stream.extend(offset.to_be_bytes());
                        xref_stream.extend(generation.to_be_bytes());
                    }
                    XrefEntry::Compressed { container, index } => {
                        // Type 2
                        xref_stream.push(2); // Type 2
                        xref_stream.extend(container.to_be_bytes());
                        xref_stream.extend(index.to_be_bytes());
                    }
                }
                obj_id += 1;
            }
        }

        // The end of line character should not be counted, added later.
        let stream_length = xref_stream.len();

        if filter == XRefStreamFilter::ASCIIHexDecode {
            xref_stream = xref_stream
                .iter()
                .flat_map(|c| format!("{:02X}", c).as_bytes().to_vec())
                .collect::<Vec<u8>>();
        }

        Ok((xref_stream, stream_length, Array(xref_index)))
    }

    fn write_indirect_object<W: Write>(
        file: &mut CountingWrite<&mut W>, id: u32, generation: u16, object: &Object, xref: &mut Xref,
    ) -> Result<()> {
        let offset = file.bytes_written as u32;
        xref.insert(id, XrefEntry::Normal { offset, generation });
        write!(
            file,
            "{} {} obj\n{}",
            id,
            generation,
            if Writer::need_separator(object) { " " } else { "" }
        )?;
        Writer::write_object(file, object)?;
        writeln!(
            file,
            "{}\nendobj",
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
                let mut buf = itoa::Buffer::new();
                file.write_all(buf.format(*value).as_bytes())
            }
            Real(ref value) => write!(file, "{}", value),
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
            if b" \t\n\r\x0C()<>[]{}/%#".contains(&byte) || !(33..=126).contains(&byte) {
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
        file.write_all(b"\nendstream")?;
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

    // Create temporary folder to store file.
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test_0_save.pdf");
    doc.save(&file_path).unwrap();
    // Check if file was created.
    assert!(file_path.exists());
    // Check if path is file
    assert!(file_path.is_file());
    // Check if the file is above 400 bytes (should be about 610 bytes)
    assert!(file_path.metadata().unwrap().len() > 400);
}
