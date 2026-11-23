---
level: advanced
---

# JSON and TOML for any API

`ww_json` turns the values of any API into JSON and back, and describes both with a JSON Schema. The same walk reads and writes [TOML](#toml), for settings files. It works from the API model (introspection data or an API crate), so nothing is written per command: an MCP server, a web client or a script gets arguments as a JSON object, results as JSON, and a schema per method, property and stream item.

```rust
let codec = ww_json::Codec::new(&bundle).root("args");
let values = codec.args_from_json(&serde_json::json!({"delay": "2 s"}), args)?; // Vec<ValueOwned>
let json = codec.args_to_json(&values, args)?;                                   // {"delay": "2000000 µs"}
```

`bundle` is a [prepared](../std_library/ww_self.md) `ApiBundleOwned`, the same one the dynamic client and Python use. The codec is `ValueOwned` ⇄ `serde_json::Value`: serializing to bytes is the dynamic client's.

## The model in a program

A host program that talks to a device has the device's introspection data. A program that only reads and writes values of its own types (settings files, an MCP server of its commands) takes the model from the API crate: `bundle` in a `ww_codegen!` call there emits the whole model as a byte const (API-73).

```rust
// in the API crate, next to the root: emits `SETTINGS_BUNDLE`
ww_codegen!(crate :: Settings, bundle = "no_docs"); // or "with_docs"
```

```rust
use shrink_wrap::DeserializeShrinkWrapOwned;
let mut bundle = ww_self::ApiBundleOwned::from_ww_bytes_owned(&settings_api::SETTINGS_BUNDLE)?;
ww_self::generics::prepare(&mut bundle)?;
let codec = ww_json::Codec::new(&bundle).root("settings");
```

Unlike a device's `WW_SELF_BYTES`, nothing is left out: dependency types (`ww_si`'s quantities, a standard library trait) are in the bytes, so no snapshot is looked up. The API crate stays free of the runtime: the call needs no server, client or `for <struct>`. With `"with_docs"` the bytes are those of the bundle `wire_weaver_core::load` gives from source, so tests and the installed program use the same model.

## The forms

| WireWeaver | JSON |
|---|---|
| `bool` | `true`, `false` |
| integers, `UNib32`, `u4`, nibbles | number; beyond ±2⁵³ a decimal string |
| `f32`, `f64` | number; `"NaN"`, `"inf"`, `"-inf"` as strings |
| `String` | string |
| `Vec<u8>`, `[u8; N]` | lowercase hex string; a list of numbers is accepted |
| `Vec<T>`, `[T; N]`, `Series<T>`, tuple | list |
| struct with named fields | object, in the order of the definition |
| tuple struct | list; with one field its value |
| unit struct | `null` |
| enum unit variant | `"Variant"` |
| enum variant with fields | `{"Variant": fields}` |
| enum variant of a newer version | `"<unknown N>"` |
| `Option<T>` | `null` or `T` |
| `Result<T, E>` | `{"Ok": T}` or `{"Err": E}` |
| `Range`, `RangeInclusive` | `[start, end]` |
| `Quantity`, `AnyQuantity`, `Fixed`, `Point`, `Coded` | text with its unit, `"5 ms"` |
| `Index`, `IndexFrom1`, `Seq` | the number |

Output is deterministic: the same value is the same text. Input is strict: an unknown field, a value of another kind and a number that does not fit are errors.

### Big numbers

A JSON number is a double in most parsers, exact up to 2⁵³. An integer beyond it is written as a decimal string, and read back from a string or a number:

```json
"18446744073709551615"
```

The same goes for `i128`, `u128` and a `Fixed` of `u64` (a timestamp in µs). Floats are numbers, written by their shortest text, so an `f32` `0.1` is `0.1`. NaN and the infinities are strings.

### Quantities

A quantity is written as text in the field's own unit and scale, exactly: `"5 ms"` for a `Fixed<Second, Milli, u16>`, `"4 KiB"`, `"3300 mV"`. A scale without a symbol is its factor, `"2048·33/40960 V"`. A `Point` is shown in its origin, `"25 °C"`, `"90.25 °"`.

Input takes the same text, or the text in any unit that converts exactly:

```json
"0.005 s"     → "5 ms"
"5000 µs"     → "5 ms"
"298.15 K"    → "25 °C"   (board_temp, raw / 256 + 25 °C)
"1.5 µs"      → error: 1.5 µs can't be written into Fixed<Second, Milli, u16, Unbounded>: not a whole number of ms
"5 V"         → error: `5 V`: expected a quantity in Second (s), e.g. `1 s` or with a prefix
"6 s"         → error: 6 s can't be written into Fixed<Second, Milli, u16, Range<1, 5000>>: 6000 is out of 1..=5000
```

A plain number is the stored number of a `Fixed`, `Index` or `Seq` (`5` into `Fixed<Second, Milli, u16>` is 5 ms), as Python accepts it; of a `ww_si::Quantity<U>` it is the number in the unit itself. A `Point` takes no plain number (raw or shown?), an `AnyQuantity` has no unit to read one in.

A `ww_si::Quantity<U>` stores any number type: text is read as an `i32` or `f32`, so a value read back is the same quantity, possibly with another number type than the device sent.

### Arguments

A method's or stream function's arguments are one object by name, in the order of the definition (a `#[flatten]` argument's fields in its place). An argument with a default (`#[default]`, a trailing `Option`, `Vec` or `String`) may be left out:

```json
{"delay": "2 s"}     → {"delay": "2000000 µs"}   (wait)
```

## Errors

An error names the JSON path of the failing value and what was expected:

```text
args.delay: 6 s can't be written into Fixed<Second, Micro, u32, Range<0, 5000000>>: 6000000 is out of 0..=5000000
value.small: 99 is out of range for ub4
value: 256 is out of range for u8
```

`ww_json::JsonError` has the `JsonPath` (`args.timeout`, `items[2].name`) and an `ErrorKind`: `WrongType`, `OutOfRange`, `UnknownField`, `MissingField`, `UnknownVariant`, `Length`, `Quantity`, `Unsupported`, `Model`, `Mismatch`, `TooDeep`.

## Structural form

`Codec::raw()` writes the structs quantities are on the wire (`{"value": 5000}`, `{"prefix": "Milli", "value": {"U32": 3300}}`) and byte lists as lists. It is what the conformance corpus is written in, so every consumer reads the same values ([conformance](../serdes/conformance.md)).

## TOML

`ww_json::toml` is the same codec for TOML files, such as an application's settings levels, on `toml_edit` (re-exported as `ww_json::toml_edit`, without serde). It follows the JSON form's rules, with what TOML holds natively:

| WireWeaver | TOML |
|---|---|
| integers | integer; beyond `i64` a decimal string, so a `u64` tick count stays an integer up to 2⁶³ |
| `f32`, `f64` | float, `nan` and `inf` included |
| struct with named fields | table; a nested struct is a sub-table `[audio.limits]` |
| unit struct | empty table `{}` |
| enum unit variant | snake_case string, `"fast_start"`; the name as defined is read too |
| enum variant with fields | `{fast_start = fields}` |
| `Option<T>` | the key left out for `None`; a `None` inside a list has no form |
| quantities | text with the unit, `"300 ms"`, `"16 kHz"`, read in any unit that converts exactly |
| lists, tuples, `Result`, bytes, ranges | as in JSON |

A date or time of TOML has no value in the model and is refused.

```rust
use ww_json::toml_edit::DocumentMut;
let codec = ww_json::Codec::new(&bundle).root("streamer");
let base = codec.from_toml(&built_in_doc.as_item(), ty)?;   // the type's Default, encoded once
let level: DocumentMut = text.parse()?;
let settings = codec.decode_onto(&base, &level, ty)?;       // each level onto the one before
```

`decode_onto(base, table, ty)` sets only the keys the table holds and keeps every other field of `base`. A key naming a nested struct (or a `Some` option of one) goes onto that field key by key; any other key replaces the field. A caller's built-in level is the type's `Default` encoded to a value, and each level's table goes onto the result of the one before, so field defaults need not enter the API model. `to_toml` gives the full table.

`write_onto(&mut level, &base, &settings, ty)` is the inverse, for write-back: it edits the parsed document so that `decode_onto` of it gives `settings`. A key whose value already decodes to the wanted one stays as typed (`"0.4 s"` stays, although the encoder writes `"400 ms"`), a changed key gets its new value with the old one's comment, and a field equal to `base` that the level doesn't name is not added. Untouched lines and comments stay:

```toml
# the streamer's level
[audio]
gain = 3.0 # louder        ← was 2.0
window = "0.4 s"   # as typed

[audio.limits]             ← added: strict differs from the base
strict = true
```

### Markers

`decode_onto` reads two markers, a one-key inline table in place of a field's ordinary value:

| Marker | Means |
|---|---|
| `{ unset_to_none = true }` | an `Option` field is `None` over any base value, where a left-out key would keep it (Q22) |
| `{ use_parent = true }` | the field keeps the base's value outright, the same as leaving the key out (Q23) |

```toml
[audio.limits]
max_clip_length = { unset_to_none = true }   # the channel clears the streamer's limit
window = { use_parent = true }               # explicit: this level keeps the channel's value
```

`write_onto` writes `{ unset_to_none = true }` when it must: `None` set over a base that has a value, where TOML's own way of saying so, leaving the key out, would read back as the base's value instead. It never writes `{ use_parent = true }` itself, since a field equal to the base is left out as usual, but keeps an existing line of either marker alone when the value it stands for still matches, same as any other untouched key.

No struct field may be named `unset_to_none` or `use_parent`: a one-key table of either name is read as the marker, never as a struct's own value, so the two names stay unambiguous.

Errors are the JSON form's, with the path from the root: an unknown key is `UnknownField` with the names valid there (so a caller can offer the nearest), a value of another kind `WrongType`, a value that doesn't parse in its unit `Quantity`:

```text
streamer.audio: no field `span`, expected one of: window, rate, duty, gain, mode, limits
streamer.audio.window: `3 V`: expected a quantity in Second (s), e.g. `1 s` or with a prefix
```

## JSON Schema

`ww_json::schema` writes a JSON Schema (draft 2020-12) of the same forms: `item_schemas(item, bundle)` for one API item (the arguments of a method as an object, its result, the value of a property, the item of a stream), `type_schema` and `args_schema` for any type or argument list. Each schema is a complete document with the `$defs` it refers to; doc comments are `description`s. A test validates every value the codec writes for the conformance corpus against the schema of its type.

A struct is an object with `properties` in the order of the definition, `required` listing the fields without a default that are not `Option`, and `additionalProperties: false`. An enum is a `oneOf`: `{"const": "Unit"}` or `{"Variant": fields}`. A quantity is a string that names its unit and bounds:

```json
{
  "type": "string",
  "x-min": "1 ms",
  "x-max": "5000 ms",
  "description": "A quantity of Second, written as text with its unit: `5 ms`, in whole ms (stored as u16), from 1 ms to 5000 ms. Any unit and prefix that converts exactly is accepted too. A plain number is taken as ms.",
  "x-quantity": "fixed",
  "x-unit": "ms",
  "x-storage": "u16"
}
```

An integer that a JSON number does not hold exactly is a number up to ±2⁵³ or a decimal string, with its range in `description` and `x-range`:

```json
{"anyOf": [{"type": "integer", "minimum": 0, "maximum": 9007199254740992}, {"type": "string", "pattern": "^-?[0-9]+$"}],
 "description": "An integer from 0 to 18446744073709551615: a number up to ±2^53, a decimal string beyond.",
 "x-range": ["0", "18446744073709551615"]}
```

Bytes are a hex string or a list of numbers, a string is `maxLength`-limited for `MaxLen`, a list has `minItems` and `maxItems` for an array, a tuple is `prefixItems`. A recursive type is one `$defs` entry referring to itself. The schema is never looser than the codec: it names the forms the codec writes, and leaves out a whole float for an integer or a plain number for a quantity, which the codec also takes.
