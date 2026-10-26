use crate::ObjectStreamConfig;

/// Options for saving PDF documents
#[derive(Debug, Clone, Default)]
pub struct SaveOptions {
    /// Enable object streams for compressing non-stream objects
    pub use_object_streams: bool,

    /// Enable cross-reference streams instead of traditional xref tables
    pub use_xref_streams: bool,

    /// Enable linearization (fast web view)
    pub linearize: bool,

    /// Configuration for object streams
    pub object_stream_config: ObjectStreamConfig,

    /// Write only 7-bit ASCII: tab, line feed, carriage return and the printable characters.
    ///
    /// The comment line of high bytes after the header is left out, literal strings write
    /// other bytes as octal escapes, and a stream whose data holds any other byte, the
    /// cross-reference stream and object streams included, is written with `/ASCIIHexDecode`
    /// added in front of its filters. A reader decodes every stream to the same data. Saving
    /// an encrypted document this way fails, because a reader decrypts a stream before it
    /// applies the stream's filters, so the ciphertext would have to be written as it is.
    pub ascii: bool,

    /// Write each dictionary entry on a line of its own, indented by two spaces per level of
    /// nesting, and each element of an array that holds a dictionary or an array likewise, so
    /// that a saved file reads well in a text editor. Stream data is written as it is.
    pub pretty: bool,
}

impl SaveOptions {
    /// Create a builder for SaveOptions
    pub fn builder() -> SaveOptionsBuilder {
        SaveOptionsBuilder::default()
    }
}

/// Builder for SaveOptions
#[derive(Default)]
pub struct SaveOptionsBuilder {
    use_object_streams: bool,
    use_xref_streams: bool,
    linearize: bool,
    max_objects_per_stream: usize,
    compression_level: u32,
    ascii: bool,
    pretty: bool,
}

impl SaveOptionsBuilder {
    /// Enable or disable object streams
    pub fn use_object_streams(mut self, value: bool) -> Self {
        self.use_object_streams = value;
        self
    }

    /// Enable or disable cross-reference streams
    pub fn use_xref_streams(mut self, value: bool) -> Self {
        self.use_xref_streams = value;
        self
    }

    /// Enable or disable linearization
    pub fn linearize(mut self, value: bool) -> Self {
        self.linearize = value;
        self
    }

    /// Set maximum objects per stream
    pub fn max_objects_per_stream(mut self, value: usize) -> Self {
        self.max_objects_per_stream = value;
        self
    }

    /// Set compression level (0-9)
    pub fn compression_level(mut self, value: u32) -> Self {
        self.compression_level = value;
        self
    }

    /// Write only 7-bit ASCII, see [`SaveOptions::ascii`]
    pub fn ascii(mut self, value: bool) -> Self {
        self.ascii = value;
        self
    }

    /// Indent dictionaries and arrays, see [`SaveOptions::pretty`]
    pub fn pretty(mut self, value: bool) -> Self {
        self.pretty = value;
        self
    }

    /// Build the SaveOptions
    pub fn build(self) -> SaveOptions {
        SaveOptions {
            use_object_streams: self.use_object_streams,
            use_xref_streams: self.use_xref_streams,
            linearize: self.linearize,
            object_stream_config: ObjectStreamConfig {
                max_objects_per_stream: if self.max_objects_per_stream == 0 {
                    100
                } else {
                    self.max_objects_per_stream
                },
                compression_level: self.compression_level,
            },
            ascii: self.ascii,
            pretty: self.pretty,
        }
    }
}
