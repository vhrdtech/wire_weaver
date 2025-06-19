# WireWeaver

WireWeaver is a wire format and API code generator for resource constrained systems (for example microcontrollers).
It allows you to define data types, methods, properties and streams and generate code that uses no standard library or
memory allocation. Unsized types - `Vec<T>`, String and others are supported (even on no_std without allocator!).
Backwards and forwards compatibility is supported: devices with older format version can communicate with newer ones and
vice versa.

Currently only Rust language is supported, with the idea to handle device communications in Rust and provide higher
level bindings for C++, Python or other languages if needed.

Current state is - highly experimental.

## Prerequisites

All examples below assume that wire_weaver dependency is added in Cargo.toml and the following use statement:

```rust
use wire_weaver::prelude::*;
```

## Built-in types

* Boolean (one-bit alignment): `bool`
* Discrete numbers:
    * Signed (one-byte alignment): `i8`, `i16`, `i32`, `i64`, `i128`
    * Unsigned (one-byte alignment): `u8`, `u16`, `u32`, `u64`, `u128`
    * Unsigned (four-bit alignment): `u4`
    * Signed and unsigned (one-bit alignment): `iN` and `uN` (`U1`, `U2`, `U3`, ... `U64`, `I2` ... `I64`)
* Nibble-based variable length u32: `UNib32` (1 to 11 nibbles)
* Floating point numbers: `f32`, `f64`
* Textual:
    * UTF-8 string `String`
    * TODO: With max bounded length
* Sequences:
    * Arrays:
        * Arbitrary length array: `Vec<T>`
        * Byte array: `Vec<u8>`
        * Arbitrary length array (no alloc): `RefVec<'i, T>`
        * Byte array (no alloc): `RefVec<'i, u8>`
        * TODO: Max bounded
        * TODO: Fixed length array: `[T; N]`
* `Option<T>` and `Result<T, E>`
* User-defined:
    * Struct
    * Enum with or without data variants

* Not yet supported or not decided whether to support:
    * Tuple
    * Unicode character: `char` (4B)
    * ASCII character `c_char` (1B) (ASCII) and string: `c_str`
    * Map

### Library types

* ww_date_time crate
    * `DateTime`: ISO 8601 combined date and time with optional time zone and optional nanoseconds.
      Minimum size is 32 bits.
    * `NaiveDate`: ISO 8601 calendar date without timezone. Year stored as shifted by 2025, minimum size is 13 bits.
    * `NaiveTime`: ISO 8601 time without timezone. Size is 18 bits without nanoseconds and 49 bits with nanoseconds.
* ww_version crate
    * `Version`: SemVer version (including pre and build strings), no alloc
    * `VersionOwned`: SemVer version, same as `Version` but uses String's
    * `CompactVersion`: Global type id + major and minor version numbers, uses UNib32 for all three

### Alignment

Some types are one- or four-bit aligned and the rest are one-byte aligned. Dense packing is used to
save space, including in enum discriminants (which can even be U1).
Byte arrays, strings, and Unsized objects are
all one-byte
aligned to limit code complexity and computations required. Unused bits are set to zero and can be reclaimed when
evolving a
type.

For example:

```rust
fn version1() {
    let mut buf = [0u8; 8];
    let mut wr = BufWriter::new(&mut buf[..]);
    wr.write_bool(true).unwrap();
    wr.write_u8(0xAA).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, &[0x80, 0xAA]);
}
```

In a future version while older one is still in use, it was decided to add some more data:

```rust
fn version1_1() {
    let mut buf = [0u8; 8];
    let mut wr = BufWriter::new(&mut buf[..]);
    wr.write_bool(true).unwrap();
    wr.write(&Some(U6::new(5).unwrap())).unwrap();
    wr.write_u8(0xAA).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, &[0xC5, 0xAA]);
}
```

Older code can still read new data and will skip the Option, and newer code can read old data, yielding None.
All the while, the serialized size didn't even change (it could have though, it's just an example).

## Wire format

All serializing and deserializing operations are going through a wire format called `shrink_wrap`.
It is targeting both microcontroller and host usage.

Features:

* 1-bit, 4-bit and 1-byte alignment
* Support all the types described above
* `no_std` without allocator support (even with types like String and Vec, for both reading and writing)
* `std` support (standard Vec and String are used)
* Used in auto-generated serdes and API code
* Can be used stand-alone as well

### Automatic derive attribute macro

Writing out serializing and deserializing code by hand would be very tedious and error-prone. So a procedural macro
is provided that can create all the code.

#### Structs

Simple example on how to automatically get serializing and deserializing code generated for a struct:

```rust
#[derive_shrink_wrap]
#[derive(Debug, PartialEq)]
struct CoordV1 {
    x: u8,
    y: u8
}

fn derive_on_struct() {
    let mut buf = [0u8; 64];
    let coord = CoordV1 { x: 0xAA, y: 0xCC };
    let bytes = to_ww_bytes(&mut buf, &coord).unwrap();
    assert_eq!(bytes, &[0xAA, 0xCC]);
}
```

Let's evolve the type and try out the compatibility features:

```rust
#[derive_shrink_wrap]
#[derive(Debug, PartialEq)]
struct CoordV1_1 {
    x: u8,
    y: u8,
    #[default = None]
    z: Option<u8>
}

fn evolved_struct() {
    let mut buf = [0u8; 64];
    let coord = CoordV1_1 { x: 0xAA, y: 0xCC, z: Some(0xFF) };
    let bytes = to_ww_bytes(&mut buf, &coord).unwrap();
    assert_eq!(bytes, &[0xAA, 0xCC, 0x80, 0xFF]);
    // newer type from older data
    let coord: CoordV1_1 = from_ww_bytes(&[0xAA, 0xCC]).unwrap();
    assert_eq!(coord, CoordV1_1 { x: 0xAA, y: 0xCC, z: None });
    // older type from newer data
    let old_coord: CoordV1 = from_ww_bytes(bytes).unwrap();
    assert_eq!(old_coord, CoordV1 { x: 0xAA, y: 0xCC });
}
```

## API

Define a custom protocol as collections of resources - methods, properties or streams and generate server and client
side code.
Multiple levels are supported, each resource is identified via a number path from root, forming a tree. Efficient path
is used, consisting of UNib32 encoded numbers, taking as little as 4 bits.

Resources can be arranged into "ww-trait's" and then "implemented" at various points in the API tree. Accessing them is
possible in the same manner via resource paths, or through their globally unique ID (crate name + version or unique ID +
version). Many useful "ww-traits" are planned, implementing things like firmware update, event counters, logging, power
management, etc. That way code to handle them all can be reused greatly between very different projects. UI can also be
arranged into small reusable blocks.

Two models are planned: client-server and bus. Client-server model is functional (`ww_client_server` crate) and
supported in code generation for both server and client side, std and no_std. Bus model is still in development.

### Methods

#### Async and sync

#### Deferred and Immediate

### Streams

```rust
trait Log {
    fn defmt_bytes() -> Stream<u8>;
    fn sink(stream_in: Sink<u8>);
}
```

### Properties

#### Get/Set and value on change

### Traits

### Transport protocols

Several transport protocols are supported:

* USB (nusb on host side, embassy on embedded, no drivers needed on Windows/Mac/Linux)
* WebSocket (for reliable control access)
* UDP (for telemetry)
* TODO: CAN Bus (using CANOpen)

Others could be easily implemented, possibly reusing the same code.

USB and UDP transports support multiple events per packet/datagram. Many small messages can be accumulated over a time
window conserving bandwidth and allowing much higher message throughput per unit of time that would otherwise be
possible with one message per packet/datagram.

## Versioning

Each type and "ww-trait" version is it's crate version, same versioning rules apply.
Types and "ww-trait's" are globally identified by their crate name and version. `FullVersion` type is provided in
`ww_version` crate
that carries crate name in addition to version numbers.

### Compact ww-trait version

There is a possibility to make API calls on "ww-traits", without knowing the exact resource path. For example one could
make a "sleep"
call on all devices in a CAN Bus network, that support "PowerManagement" trait. Or "get_fw_version" on any device
supporting "FirmwareInfo" trait. In order to do so, instead of relying on resource path (a vector of numbers from API
root), `FullVersion` is sent instead.

Compared to resource paths that can only take a few bytes (numbers are UNib32 encoded, so the smallest path is 4 bits),
`FullVersion` is likely
to take about 8-16 bytes or more and vary with the crate name. This is unfortunate for constrained systems, or if one
want to pack many calls into one packet.

Solution to this is `CompactVersion`, which carries globally unique type id and major.minor version components only, all
UNib32 encoded.
The only downside is that guaranteeing globally unique IDs is not as simple as using crate's name anymore. IDs are
manually assigned and tracked via git instead.

## UI utility

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

## TODO: Subtypes (bounded numbers and array lengths)

Simple checked numbers where only a range of values is allowed:

* `u16<{1..=512}>`

Set of allowed values:

* `u8<{0..=8}, 12, 16, 20, 24, 32, 48, 64>`

Numbers are checked before serialization and after deserialization.

## TODO: SI support

Specify SI unit for any number:

* current: `f32<"A">`
* velocity: `f32<"m/s">`

Units are not transmitted over the wire, used as a hint for code generation and in UI tool.
