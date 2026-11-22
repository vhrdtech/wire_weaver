---
level: advanced
---

# JSON for any API

`ww_json` turns the values of any API into JSON and back. It works from the API model (introspection data or an API crate), so nothing is written per command: an MCP server, a web client or a script gets arguments as a JSON object and results as JSON.

```rust
let codec = ww_json::Codec::new(&bundle).root("args");
let values = codec.args_from_json(&serde_json::json!({"delay": "2 s"}), args)?; // Vec<ValueOwned>
let json = codec.args_to_json(&values, args)?;                                   // {"delay": "2000000 µs"}
```

`bundle` is a [prepared](../std_library/ww_self.md) `ApiBundleOwned`, the same one the dynamic client and Python use. The codec is `ValueOwned` ⇄ `serde_json::Value`: serializing to bytes is the dynamic client's.

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
