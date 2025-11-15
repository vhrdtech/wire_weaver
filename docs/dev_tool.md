# Dev GUI tool

Working features:

* Load and parse format definition file
* Show internal AST
* Show generated serdes code
* Show generated client-server code
* no_std switch to quickly view how embedded vs host code looks like

Planned features:

* Provide input and output widgets for various types (number with SI support as spinner / dial / slide, string,
  color, ...)
* Generate documentation like UI with the ability to interact with server code
* Generate server mockup UI with the ability to respond with user input, prerecorded answers or examples
* Support for bytecode loading to extract types and api information
* Support for source loading from external sources and compiling to bytecode (through Rust lib FFI or backend service)
