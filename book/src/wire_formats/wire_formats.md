# Wire Formats
Common features:

* Platform independent.
* Backwards compatibility, ability to add new fields (to the end).
* Not self-describing, vhL description is used to give meaning to data. *Is there a need for self describing format?*.
* Generate serialisation and deserialisation code in various languages (Rust, Dart, C, C++, ...).
* Ability to configure and tweak the code generator - only code changes without impacting interoperability of the
  format.
* For now codegen to be implemented in Rust, maybe later by the language itself.
* Support/utils libraries in target languages written manually.
* Various cli tool commands to encode / decode / check / convert / etc.

## Binary dense - `wfd`

> Dense packing using nibble buffers and variable length numbers based on nibbles as well.
> Mainly targeted at constrained systems and low bandwidth channels.

Format features:

* Support for no_std environment without memory allocator.
* Zero-copy, in-place processing.
* All types are supported, including variable length arrays and strings.
* No type information, only data and necessary service information.
* Bounds checking.
* Optional additional sanity checks (cannot cover all errors though, use other means of checking for faulty channels,
  like CRC or FEC).

## Binary sparse - `wfs`
> Similar to vlu4 but uses byte buffers to increase processing speed where size is not such an issue.

## Binary padded - `wfp`
> Sparsely packed and properly padded for fast in-place processing.
> Mainly targeted for inter-process communication.

Format features:

* Little endian.

## vhL Text Form - `wflt` (literal-text)

> For ease of interaction with humans.

## JSON - `wfjson`

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