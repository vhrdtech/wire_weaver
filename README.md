# WireWeaver
WireWeaver is a wire format and API code generator for resource constrained systems (for example microcontrollers).
It allows you to define data types, methods, properties or streams and generate code that uses no standard library or memory allocation.
Backwards and forwards compatibility is supported: devices with older format version can communicate with newer ones and vice versa.

Currently only Rust language is supported, with the idea to handle device communications in Rust and provide higher level bindings for C++, Python or other languages if needed.

Current state is - highly experimental.

## Supported types
* Boolean: `bool`
* Discrete numbers:
  * Signed: `i8`, `i16`, `i32`, `i64`, `i128`
  * Unsigned: `u4`, `u8`, `u16`, `u32`, `u64`, `u128`
* Variable length encoded numbers: `leb<T>`, `nib<u32>`
* Floating point numbers: `f32`, `f64`
* Textual:
  * UTF-8 string `str`, or with max bounded length: `str<N>` (N in bytes)
* Sequences:
  * Arrays:
    * Fixed length array: `[T; N]`
    * Arbitrary length array: `vec<T>` or max bounded: `vec<T, N>`
* `Option<T>` and `Result<T, E>`
* User-defined:
  * Struct
  * Enum with data variants


* Not yet supported or not decided whether to support:
  * Tuple
  * Unicode character: `char` (4B)
  * ASCII character `c_char` (1B) (ASCII) and string: `c_str`
  * Map
  * Bitfield

## Bounded numbers
Simple checked numbers where only a range of values is allowed:
* `u16<{1..=512}>`

Set of allowed values:
* `u8<{0..=8}, 12, 16, 20, 24, 32, 48, 64>`

Numbers are checked before serialization and after deserialization.

## SI support
Specify SI unit for any number:
* current: `f32<"A">`
* velocity: `f32<"m/s">`

Units are not transmitted over the wire, used as a hint for code generation and in UI tool.

## Syntax
Rust syntax is reused with addition of several attributes.

## Wire format definition
Struct fields are laid out in order, as defined or according to provided id.

Only one wire format is currently being worked on targeted at microcontroller usage: wfdb.
Features:
* 1 byte alignment
* Support all types described above
* Booleans can take 1 bit, 4 bit or 1B of space, see pre-conditions below.
* u4 / nibble based variable length numbers used for array length


## API
Define a custom protocol as collections of methods, properties or streams and generate server and client side code.
Event based communication model is used.
Generated code will perform protocol compatibility checks.

Under the hood API code generator uses a WireWeaver definition of Event. Custom Event type can also be provided?

## Versioning
Each file containing WireWeaver code must be saved with a version appended after it's name. Before code generation,
compatibility check is performed to ensure backwards and forward compatibility.

## UI utility
Features:
* Support for bytecode loading in order to extract types and api information
* Support for source loading from external sources and compiling to bytecode (through Rust lib FFI or backend service)
* Provide input and output widgets for various types (number with SI support as spinner / dial / slide, string, color, ...)
* Generate documentation like UI with the ability to interact with server code
* Generate server mockup UI with ability to respond with user input, prerecorded answers or examples

## CLI utility
Utility for generating code, documentation, publishing to the repository.
Main way to invoke WireWeaver in Rust is through procedural macros, no CLI tool calls are required.

## Repository
Public repository for common dependencies and unique protocol IDs.