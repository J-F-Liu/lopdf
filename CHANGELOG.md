
<a name="v0.28.0"></a>

* Added new field to `Document`, `xref_start`. Default value is `0`.
* Added new function to `Document`: `new_from_prev`, `has_object`, `add_page_contents`, `set_object`
* New structure added `IncrementalDocument`.
* Support writing of PDF file with "Cross Reference Stream".
* Cross Reference Tables now write abbreviated tables when non-consecutive object are written.

<a name="v0.26.0"></a>
## [v0.26.0](https://github.com/J-F-Liu/lopdf/compare/v0.25.0...v0.26.0) (2020-09-29)

### Add

* Add `as_str`, `as_str_mut` methods to `Object` ([#107](https://github.com/J-F-Liu/lopdf/issues/107))

### Dtoa

* dtoa may write real number in exponential format which is not allowed in PDF

### Genericize

* Genericize Content to allow `AsRef<[Operation]>` ([#111](https://github.com/J-F-Liu/lopdf/issues/111))

### Make

* Make pom dependency optional (but default) ([#112](https://github.com/J-F-Liu/lopdf/issues/112))
* Make rayon dependency optional ([#108](https://github.com/J-F-Liu/lopdf/issues/108))

### Merge

* Merge document PDF logic with some fixes ([#117](https://github.com/J-F-Liu/lopdf/issues/117))

### Various

* Various improvements, updated libraries and image features ([#118](https://github.com/J-F-Liu/lopdf/issues/118))


<a name="v0.25.0"></a>
## [v0.25.0](https://github.com/J-F-Liu/lopdf/compare/v0.24.0...v0.25.0) (2020-06-25)

### Add

* Add indexing checks ([#98](https://github.com/J-F-Liu/lopdf/issues/98))

### Add

* Add a test for [#93](https://github.com/J-F-Liu/lopdf/issues/93) ([#95](https://github.com/J-F-Liu/lopdf/issues/95))

### Bugfix

* Bugfix for xref_start. ([#105](https://github.com/J-F-Liu/lopdf/issues/105))

### Check

* check  that the buffer is big enough for startxref ([#93](https://github.com/J-F-Liu/lopdf/issues/93))

### Create

* Create rust.yml ([#104](https://github.com/J-F-Liu/lopdf/issues/104))

### Extend

* extend recursion limit to non-local references ([#100](https://github.com/J-F-Liu/lopdf/issues/100))

### Fix

* Fix compilation error&test error ([#102](https://github.com/J-F-Liu/lopdf/issues/102))

### Keep

* keep looking for the last pattern ([#94](https://github.com/J-F-Liu/lopdf/issues/94))

### Limit

* limit recursion to the number of objects ([#92](https://github.com/J-F-Liu/lopdf/issues/92))

### Limit

* Limit allowed bracket depth. ([#97](https://github.com/J-F-Liu/lopdf/issues/97))

### Move

* Move bracket depth checking into parsers. ([#101](https://github.com/J-F-Liu/lopdf/issues/101))

### Release

* Release 0.25

### Return

* Return Result from as_array_mut() ([#106](https://github.com/J-F-Liu/lopdf/issues/106))

### Update

* Update itoa and linked-hash-map ([#91](https://github.com/J-F-Liu/lopdf/issues/91))


<a name="v0.24.0"></a>
## [v0.24.0](https://github.com/J-F-Liu/lopdf/compare/v0.23.0...v0.24.0) (2020-02-17)

### Compute

* Compute an accurate iterator size when the page tree is sane.

### Fix

* Fix datetime parser ([#89](https://github.com/J-F-Liu/lopdf/issues/89))

### More

* More permissive datetime parsing ([#90](https://github.com/J-F-Liu/lopdf/issues/90))

### Release

* Release 0.24

### Validate

* Validate expected id in pom parser.
* Validate the expected id when reading indirect objects.


<a name="v0.23.0"></a>
## [v0.23.0](https://github.com/J-F-Liu/lopdf/compare/v0.22.0...v0.23.0) (2019-07-14)

### Adapt

* Adapt pom parser.

### Add

* Add error descriptions.
* Add a proper error type and remove some more panics.

### Allow

* Allow loading a document from a memory slice.

### Avoid

* Avoid allocating an intermediate collection for iteration.
* Avoid unwraps when already returning an Option for failure.

### Error

* Error signaling around compression and image handling.

### Escape

* Escape fix ([#68](https://github.com/J-F-Liu/lopdf/issues/68))

### Export

* Export dereference function as it is useful for PDF consumers.
* Export filters module.

### Get_font_encoding

* get_font_encoding seems more at home with Dictionary.

### Handle

* Handle stream filter chains ([#66](https://github.com/J-F-Liu/lopdf/issues/66))

### Hex

* Hex fix ([#67](https://github.com/J-F-Liu/lopdf/issues/67))

### Implement

* Implement LZW decompression.

### Improve

* Improve hex parsing performance.

### Make

* Make a page iterator.
* Make Reader::read consume the Reader.
* Make content operations faillible.

### Protect

* Protect against reference loops.
* Protect against a corrupted page tree.

### Refactor

* Refactor a bit to allow a utility function.

### Release

* Release 0.23.0

### Remove

* Remove intermediate assignation.
* Remove unsafe code around FilterType.
* Remove unsafe code on get_object_mut.
* Remove some 'if let' for readability.
* Remove more panic paths in xref parsing.

### Replace

* Replace unwraps in processor.rs.

### Return

* Return results when appropriate.

### Separate

* Separate decompression into two functions.

### Take

* Take care of panic that I actually hit on the pom side.
* Take care of creator.rs.

### Unify

* Unify buffer creation.

### Use

* Use lifetime ellision.
* Use TryInto.
* Use writeln where appropriate.
* Use error enum in reader.
* Use stable cloned.


<a name="v0.22.0"></a>
## [v0.22.0](https://github.com/J-F-Liu/lopdf/compare/v0.21.0...v0.22.0) (2019-05-13)

### Add

* Add parsing benchmark.
* Add nom dependency.

### Also

* Also test with nom parsing feature enabled.

### Array

* Array and dictionary parsing.

### Avoid

* Avoid using format! when writing.

### Be

* Be explicit about trait objects.

### Boolean

* Boolean and null parsing.

### Content

* Content parsing.

### Duplicate

* Duplicate pom parser for incremental replacement with nom 5.

### Ease

* Ease off on rayon a bit.

### Escape

* Escape sequence parsing.

### Extern

* extern crate is not required anymore with 2018 edition.

### Fix

* Fix last ugly parser.
* Fix octal parser.
* Fix pdfutil build

### Float

* Float parsing.

### Header

* Header parsing.

### Hex

* Hex string parsing.

### Indirect

* Indirect object and stream parsing.

### Literal

* Literal string syntax.

### Make

* Make sure Stream.start_position is relative to the whole file.

### Merge

* Merge remote-tracking branch 'upstream/master' into nom5
* Merge remote-tracking branch 'upstream/master' into nom5

### More

* More 2018 edition lints.
* More cleanup.
* More cleanup.
* More simplifications.

### Object

* Object id and reference parsing.

### Octal

* Octal and hexadecimal parsing.

### Parallel

* Parallel object stream parsing.

### Rayon

* Rayon usage proof of concept.

### Release

* Release 0.22.0

### Remove

* Remove pom dependency in tests.

### Replace

* Replace name parser.

### Resolve

* Resolve name collisions.

### Simplify

* Simplify lifetime annotations.

### Slowly

* Slowly replace the cute pom parser with nom.

### Trailer

* Trailer and xref start.

### Turns

* Turns out "contained" already exists in nom.

### Unify

* Unify both variants of the parsing functions.

### Use

* Use a BufWriter when saving to path.
* Use parse_at(&self.buffer, offset) to read indirect_object
* Use nom digit testing functions.
* Use lifetime ellision.
* Use nom sequence operators.

### Useless

* Useless move.

### Xref

* Xref stream and trailer parsing.
* Xref parsing.


<a name="v0.21.0"></a>
## [v0.21.0](https://github.com/J-F-Liu/lopdf/compare/v0.20.0...v0.21.0) (2019-04-26)

### Avoid

* Avoid allocating a String.

### Check

* Check offsets read from file to avoid panics
* Check and correct Size entry of trailer dictionary

### Clean

* Clean up bytes_to_string, string_to_bytes iterators

### Fix

* Fix clippy warnings
* Fix .editorconfig

### Fixed

* fixed finally

### Redundant

* Redundant imports with 2018 edition.

### Release

* Release 0.21.0

### Update

* Update example
* Update Cargo.toml

### Use

* Use env_logger in pdfutil


<a name="v0.20.0"></a>
## [v0.20.0](https://github.com/J-F-Liu/lopdf/compare/v0.19.0...v0.20.0) (2019-03-07)

### Release

* Release 0.20.0

### Replace

* Replace println with log macros

### Use

* Use Rust 2018
* Use pom 3.0


<a name="v0.19.0"></a>
## [v0.19.0](https://github.com/J-F-Liu/lopdf/compare/v0.18.0...v0.19.0) (2018-10-24)

### Allow

* Allow xref section has zero entries

### Dictionary

* Dictionary key type changed to Vec<u8>

### Format

* Format code with rustfmt

### Improve

* Improve codestyle (simplify loops, remove closures, use is_empty() etc.)

### Move

* Move image dependency to embed_image feature

### Release

* Release 0.19.0

### Skip

* Skip corrupt deflate stream


<a name="v0.18.0"></a>
## [v0.18.0](https://github.com/J-F-Liu/lopdf/compare/v0.17.0...v0.18.0) (2018-10-05)

### Able

* Able to read stream when it's length is in object stream

### Adress

* Adress timezone formatting problem from [#34](https://github.com/J-F-Liu/lopdf/issues/34)

### Insert

* insert image on page


<a name="v0.17.0"></a>
## [v0.17.0](https://github.com/J-F-Liu/lopdf/compare/v0.16.0...v0.17.0) (2018-09-19)

### Make

* Make chrono crate optional

### Release

* Release 0.17.0

### Update

* Update add_barcode example


<a name="v0.16.0"></a>
## [v0.16.0](https://github.com/J-F-Liu/lopdf/compare/v0.15.3...v0.16.0) (2018-09-18)

### Add

* Add form xobject to page
* Add extract_stream subcommand

### Compress

* Compress created Form xobject

### Compress

* compress page content after change

### Fix

* Fix collect_fonts_from_resources for referenced resources
* Fix add xobject to page resources as direct object


<a name="v0.15.3"></a>
## [v0.15.3](https://github.com/J-F-Liu/lopdf/compare/v0.15.0...v0.15.3) (2018-09-14)

### Decompress

* Decompress Form XObject

### Disable

* Disable auto format markdown

### Fix

* Fix bug in reading incremental updated document
* Fix build warning
* Fix string_to_bytes method

### Hexadecimal

* Hexadecimal strings can contain white space.

### Remove

* Remove println in extract_text

### Update

* Update example code
* Update example


<a name="v0.15.0"></a>
## [v0.15.0](https://github.com/J-F-Liu/lopdf/compare/v0.14.1...v0.15.0) (2018-02-04)

### Add

* add `get_object_mut`
* add method as_array_mut

### Extract

* Extract text from specified pages

### Replace

* Replace text of specified page


<a name="v0.14.1"></a>
## [v0.14.1](https://github.com/J-F-Liu/lopdf/compare/v0.13.0...v0.14.1) (2017-11-03)

### Add

* Add `impl From<_> for Object` for more numeric types
* Add an Object::string_literal constructor
* Add a `dictionary!` macro that creates a Dictionary
* Add `impl From<ObjectId> for Object` creating Object::Reference

### Derive

* Derive Clone for lopdf::Document

### Release

* Release 0.14.0

### Remove

* Remove the Seek bound on Document::save_to


<a name="v0.13.0"></a>
## [v0.13.0](https://github.com/J-F-Liu/lopdf/compare/v0.11.0...v0.13.0) (2017-10-02)

### Avoid

* Avoid decompress flate stream which has Subtype

### Debug

* Debug with lldb

### Fix

* Fix get_object for created document

### Ignore

* Ignore invalid objects when reading all object in xref table

### Impl

* impl fmt::Debug for Object

### Pdfutil

* pdfutil add extract_pages command

### Read

* Read optional space at the end of xref subsection header line

### Release

* Release 0.13.0
* Release 0.12.0

### Store

* Store compressed stream objects and normal objects together


<a name="v0.11.0"></a>
## [v0.11.0](https://github.com/J-F-Liu/lopdf/compare/v0.10.0...v0.11.0) (2017-08-21)

### Release

* Release 0.11.0

### Use

* Use itoa and dtoa to improve writing performance


<a name="v0.10.0"></a>
## [v0.10.0](https://github.com/J-F-Liu/lopdf/compare/v0.9.0...v0.10.0) (2017-07-20)

### Added

* Added optional allows_compression for Stream object

### Release

* Release 0.10.0


<a name="v0.9.0"></a>
## [v0.9.0](https://github.com/J-F-Liu/lopdf/compare/v0.8.0...v0.9.0) (2017-05-24)

### Add

* Add pdfutil readme

### Added

* Added unit test for load_from() and save_to()
* Added Document::with_version + refactored save() and load()
* Added Debug trait for lopdf::Document

### Apply

* Apply multiple operations in one command

### Build

* Build with Rust stable
* Build with Rust beta

### Fix

* Fix delete_zero_length_streams

### Fixed

* Fixed unit tests
* Fixed breaking API changes

### Release

* Release 0.9.0


<a name="v0.8.0"></a>
## [v0.8.0](https://github.com/J-F-Liu/lopdf/compare/v0.7.0...v0.8.0) (2017-03-16)

### Change

* Change Name(String) to Name(Vec<u8>)

### Delete_object

* delete_object and delete_unused_objects

### Get_pages

* get_pages and delete_pages

### Handle

* Handle zero length stream

### Release

* Release 0.8.0

### Traverse

* Traverse objects from trailer recursively


<a name="v0.7.0"></a>
## [v0.7.0](https://github.com/J-F-Liu/lopdf/compare/v0.6.0...v0.7.0) (2017-03-07)

### Add

* Add Content::decode() function

### Build

* Build on Rust 1.17

### Create

* Create String object for DateTime

### Parse

* Parse PDF datetime value

### Read

* Read xref stream in hybrid-reference file

### Update

* Update create_document example
* Update README


<a name="v0.6.0"></a>
## [v0.6.0](https://github.com/J-F-Liu/lopdf/compare/v0.5.0...v0.6.0) (2017-02-16)

### Add

* Add Stream::decompressed_content() method

### Read

* Read previous Xrefs of linearized or incremental updated document


<a name="v0.5.0"></a>
## [v0.5.0](https://github.com/J-F-Liu/lopdf/compare/v0.4.0...v0.5.0) (2017-02-10)

### Add

* Add size field to Xref
* Add Xref struct

### Decode

* Decode PNG frame after FlateDecode

### Read

* Read compressed objects from object stream
* Read xref stream

### Update

* Update README

### Use

* Use pom 0.9.0

### XrefEntry

* XrefEntry as enum type


<a name="v0.4.0"></a>
## [v0.4.0](https://github.com/J-F-Liu/lopdf/compare/v0.3.0...v0.4.0) (2017-01-29)

### Add

* Add Operation constructor
* Add modify_text test
* Add travis-ci build status
* Add FAQ in Readme
* Add print_xref_size() for debuging

### Decode

* Decode content stream

### Encode

* Encode content operations

### Fix

* Fix load_document test
* Fix https://github.com/rust-lang/rust/issues/39177

### Optimize

* Optimize parser code

### Solve

* Solve mutual reference problem between Pages and Page objects

### Trigger

* Trigger new release to pass build on docs.rs

### Update

* Update create PDF example


<a name="v0.3.0"></a>
## [v0.3.0](https://github.com/J-F-Liu/lopdf/compare/v0.2.0...v0.3.0) (2017-01-18)

### Add

* Add compress/decompress subcommands to pdfutil

### Create

* create PDF parser using pom instead of nom

### Dictionary

* Dictionary preserve key insert order

### Update

* Update README
* Update parser to use pom 0.6.0

### Use

* Use reader to get stream length if it is a reference object


<a name="v0.2.0"></a>
## [v0.2.0](https://github.com/J-F-Liu/lopdf/compare/v0.1.0...v0.2.0) (2017-01-05)

### Add

* Add pdfutil program

### Fix

* Fix parsing PDF array error

### Improve

* Improve documentation


<a name="v0.1.0"></a>
## v0.1.0 (2016-12-23)

### Editor

* Editor config

### Impl

* impl Document add_object method

### Improve

* Improve Document::save functional type

### Initial

* Initial commit

### PDF

* PDF objects and document definition

### Parse

* Parse and load PDF document

### Read

* Read objects from xref table instead of sequentially from file stream

### Save

* Save PDF document to file

### Store

* Store max_id as a field of document

