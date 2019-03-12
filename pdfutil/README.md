```
USAGE:
    RUST_LOG=info pdfutil [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -i, --input <input file>
    -o, --output <output file>

SUBCOMMANDS:
    compress                      Compress PDF document
    decompress                    Decompress PDF document
    delete_objects                Delete objects
    delete_pages                  Delete pages
    delete_zero_length_streams    Delete zero length stream objects
    help                          Prints this message or the help of the given subcommand(s)
    process                       Process PDF document with specified operations
    prune_objects                 Prune unused objects
    renumber_objects              Renumber objects
```
