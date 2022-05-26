# Wire Formats
Common features:
* Platform independent.
* Backwards compatibility, ability to add new fields (to the end).
* Not self-describing, vhL description is used to give meaning to data. *Is there a need for self describing format?*.
* Generate serialisation and deserialisation code in various languages (Rust, Dart, C, C++, ...).
* Ability to configure and tweak the code generator - only code changes without impacting interoperability of the format.
* For now codegen to be implemented in Rust, maybe later by the language itself.
* Support/utils libraries in target languages written manually.
* Various cli tool commands to encode / decode / check / convert / etc.

## Binary minimal
> Absolute minimum size, dense packing down to bit level.
> Mainly targeted at constrained systems and low bandwidth channels.

Format features:
* Support for no_std environment without memory allocator.
* No type information, only data and necessary service information.
* Bounds checking.
* Optional sanity checks (cannot cover all errors though, use other means of checking for faulty channels,
  like CRC or FEC).
* Little endian.

## Binary fast
> Sparsely packed and properly padded for fast in-place processing.
> Mainly targeted for inter-process communication.

Format features:
* Little endian.

## vhL Text Form
> For ease of interaction with humans.

## JSON
> For ease of interaction with humans and compatibility reasons.

## Format to format conversion
Ideally there should be a way to convert one wire format into another without losses.

### Short names for formats?
`vwbm`, `vwbfb` / `vwbfl`, `vwj` / `vwjt` + `vwjb`

### Format versions for future changes?
`vwbm-1.0`

---
Inspired by:

* [MessagePack](https://github.com/msgpack/msgpack/blob/master/spec.md)
* [Cap'n Proto wire format](https://capnproto.org/encoding.html)
* [FIDL wire format](https://fuchsia.dev/fuchsia-src/reference/fidl/language/wire-format)