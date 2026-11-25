---
level: advanced
---

# Wire sizes compared

How many bytes does the same message take in `shrink_wrap` and in eight other pure-Rust formats? This page answers
that for eight different messages. The first is a protocol header that suits `shrink_wrap` well, and the others come
from fields where it has less to gain. The numbers come from the `compare_wire_formats` example and are checked
against it, so they cannot drift from the code. A comparison like this depends on the chosen messages and values, so
change them in `src/msgs/` to measure your own.

The other formats are [postcard](https://crates.io/crates/postcard), MessagePack, JSON, CBOR (Concise Binary Object
Representation), [wincode](https://crates.io/crates/wincode), protobuf, FlatBuffers and ASN.1 UPER (the Unaligned
Packed Encoding Rules of Abstract Syntax Notation One).

## How the comparison is made

Every message exists as one plain Rust type with no layout choices. The formats that work through `serde` encode
that type. Protobuf, FlatBuffers and ASN.1 describe the same fields in their own schema. Each format uses the
settings that a project which cares about size would pick, and every table row names its setting:

| Row | Setting |
|---|---|
| WireWeaver (plain) | `#[derive_shrink_wrap]` on the plain type, without layout directives |
| WireWeaver (tuned) | The same fields with [layout directives](../serdes/derive_reference.md): `#[wire(..)]` for narrower or variable-length numbers, smaller discriminants, `#[flag]`, `sparse`, and [size assumptions](../serdes/size_assumptions.md) on nested types |
| Postcard | The default: integers have a variable length |
| Postcard (tuned) | `postcard::fixint` on the firmware checksum and on latitude and longitude; the same bytes as Postcard elsewhere |
| MessagePack (array) | `rmp_serde::to_vec`: fields by position, without names |
| MessagePack (names) | `rmp_serde::to_vec_named`: every field carries its name |
| JSON | `serde_json::to_vec`, compact |
| CBOR (names) | `ciborium`: each struct is a map with field names |
| wincode (fixed) | The default configuration: integers have a fixed width |
| wincode (varint) | `with_varint_encoding()` |
| Protobuf | Hand-written `prost` types; repeated numbers are packed; `sint32`, `fixed32` and `sfixed32` where the values call for them |
| FlatBuffers | `planus` types generated from the schemas in `schema/` |
| ASN.1 UPER (closed) | `rasn` types that declare value ranges, list sizes and variable-length integers, as described below |
| ASN.1 UPER (extensible) | The same, with an extension marker on the message, so a later revision can append fields |

The ASN.1 types declare what an ASN.1 module declares. A narrow number has a value range, and a list of known length
has a size. A 32-bit or 64-bit number that usually holds a small value is an `INTEGER (0..MAX)`, which has a variable
length. The `rasn` UPER codec cannot encode a floating-point number, so each `f32` travels as an
`OCTET STRING (SIZE(4))`.

Byte arrays use `serde_bytes`, so the `serde` formats write them as byte strings instead of lists of numbers. JSON
has no byte string and still writes a list. MessagePack, CBOR and JSON write the names of enum variants, also in the
MessagePack array form. This is why they grow so much on enum-heavy messages.

The `evolution` column says how a format lets a later revision add a field without breaking readers of the old one.
There are four groups:

- `schema_rules`: field numbers or layout rules of a schema decide which fields exist, so fields can be added or dropped.
- `self_describing`: field names travel with the data, so a reader skips what it does not know.
- `extension_markers`: a marker in the type reserves room for fields added later.
- `none`: the encoding is positional, and any schema change breaks old readers.

A format that is smaller because it gives up evolution is not smaller for free, so read both columns together. The
same applies inside the tuned `shrink_wrap` rows. The message itself can still grow. A nested type marked `sized`,
`self_describing` or `final_structure` gives up that ability in exchange for its size. The extensible ASN.1 row is
similar: only the message has the marker.

## Summary

The first table lists the bytes per format (rows) and per message (columns):

```text
--8<-- "docs/generated/compare_wire_formats/summary.csv"
```

The second table names the smallest encodings of each message and shows the two `shrink_wrap` rows next to them:

```text
--8<-- "docs/generated/compare_wire_formats/winners.csv"
```

With layout directives, `shrink_wrap` is the smallest or tied for smallest on all eight messages. It is alone on the
RPC (remote procedure call) request and on the state report, which consist of small numbers, enums and flags. On the
other six messages, postcard, ASN.1 UPER or both reach the same size.

Without layout directives, `shrink_wrap` is never the smallest. It is larger than postcard on seven of the eight
messages. The largest gap is on the CAN frame batch, where the plain type writes every identifier and time gap at
full width and postcard writes them with a variable length. The directives make the difference, and you choose them
field by field.

The extension marker of ASN.1 UPER costs nothing on seven of the eight messages, because it fits into bits that are
padding otherwise. On the GPS fix it costs one byte. FlatBuffers is the largest of the schema formats, because it is
built for reading in place, with offsets and tables of field positions instead of packed values.

## The messages

### An RPC request

The header of a call in WireWeaver's own protocol: an address (`Absolute` path, or one of two global forms), a request kind with optional bytes, a sequence number, five flags, three optional bytes and two short strings. It is the one message here designed around `shrink_wrap`'s strengths: small enums, nibble-sized numbers and flags packed into bits.

The tuned type uses `ww_repr = nib` discriminants, `UNib32` path elements and `#[flag]` to move the three presence bits next to the five flags. The plain type uses byte discriminants and `u32` path elements.

```text
--8<-- "docs/generated/compare_wire_formats/rpc_request.csv"
```

The first 48 bytes of each encoding:

```text
--8<-- "docs/generated/compare_wire_formats/rpc_request.txt"
```

### A sensor sample

An inertial sensor sample: a sequence number, a microsecond timestamp, three accelerations, three angular rates, a temperature and a battery voltage. Seven of its values are `f32`, which no format can make smaller than four bytes.

The tuned type writes the timestamp as `UVlq64` and the battery voltage in 12 bits with `U12`. The sequence number stays a `u16`, because a counter uses its whole range. The floats stay `f32` in every format.

```text
--8<-- "docs/generated/compare_wire_formats/telemetry.csv"
```

The first 48 bytes of each encoding:

```text
--8<-- "docs/generated/compare_wire_formats/telemetry.txt"
```

### A log record

A log record: a timestamp, a level, a target path, a message of 37 characters and a line number. Text dominates, and no format compresses it.

The tuned type writes the timestamp as `UVlq64`, the line number as `UNib32` and the level in 3 bits (`ww_repr = u3`, `sized`).

```text
--8<-- "docs/generated/compare_wire_formats/log_record.csv"
```

The first 48 bytes of each encoding:

```text
--8<-- "docs/generated/compare_wire_formats/log_record.txt"
```

### A firmware chunk

One chunk of a firmware update: an offset, the total size, a checksum, a last-chunk flag and a 256-byte blob. The blob dominates, so the interesting number is the overhead each format adds around it.

The tuned type writes the offset and the total as `UVlq32`. The checksum stays a fixed `u32`, because its values are uniformly distributed and a variable-length number adds a byte. Protobuf (`fixed32`) and the tuned postcard type do the same.

```text
--8<-- "docs/generated/compare_wire_formats/firmware_chunk.csv"
```

The first 48 bytes of each encoding:

```text
--8<-- "docs/generated/compare_wire_formats/firmware_chunk.txt"
```

### A device configuration

A configuration with eight optional fields, four of them set: a name, a baud rate, a timeout and an enable flag. This is where formats that store absent fields as a bit or not at all do well.

The tuned type is `sparse`: a presence header, then only the fields that are set. The plain type is a struct of `Option` fields, each with its own presence bit.

```text
--8<-- "docs/generated/compare_wire_formats/device_config.csv"
```

The first 48 bytes of each encoding:

```text
--8<-- "docs/generated/compare_wire_formats/device_config.txt"
```

### A GPS fix

A position fix: a millisecond timestamp, latitude and longitude in degrees times 10 million, an altitude in millimetres, a speed, a horizontal dilution of precision, a satellite count and a four-way fix kind. Latitude and longitude use most of their 32 bits, so a variable-length integer can take five bytes where a fixed one takes four.

The tuned type writes the timestamp as `UVlq64`, the altitude as `IVlq32`, the speed as `UVlq32`, the dilution as `UNib32`, the satellite count in 6 bits (`U6`) and the fix kind in 2 bits (`ww_repr = u2`, `sized`). Protobuf (`sfixed32`) and the tuned postcard type keep latitude and longitude at a fixed width.

```text
--8<-- "docs/generated/compare_wire_formats/gps_fix.csv"
```

The first 48 bytes of each encoding:

```text
--8<-- "docs/generated/compare_wire_formats/gps_fix.txt"
```

### A CAN frame batch

A bus number and six CAN frames, each with an identifier (standard or extended), a gap to the previous frame and up to eight bytes of data. It is a list of small structs, the case where per-element overhead decides.

The tuned type writes each identifier in 29 bits (`U29`) and the gap as `UNib32`. Each frame is a `final_structure`, which drops the per-frame size marker.

```text
--8<-- "docs/generated/compare_wire_formats/can_batch.csv"
```

The first 48 bytes of each encoding:

```text
--8<-- "docs/generated/compare_wire_formats/can_batch.txt"
```

### A state report

A report made almost entirely of enums: a mode with five values, a link state with a payload in two of its three variants, a list of two faults out of eight, a battery percentage, a charging flag and an uptime.

The tuned types use discriminants of 2 and 3 bits (`ww_repr = u2`, `u3`), a 7-bit percentage (`U7`), and a `UNib32` uptime and link speed. The mode and the faults are `sized` enums, and the link state is `self_describing`, so none of them stores a size.

```text
--8<-- "docs/generated/compare_wire_formats/state_report.csv"
```

The first 48 bytes of each encoding:

```text
--8<-- "docs/generated/compare_wire_formats/state_report.txt"
```

## Running it

From the repository root, `just test-wire-sizes` runs the example and fails when a table is stale, and
`just book-outputs` writes the tables again. The example lives in `examples/compare_wire_formats`, which is excluded
from the main Cargo workspace so that the other formats' crates stay out of every other build's lockfile. The
FlatBuffers bindings are generated once with `planus rust` from `schema/request.fbs` and `schema/messages.fbs`, and
committed.
