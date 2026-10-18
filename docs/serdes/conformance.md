---
level: nomicon
---

# Conformance corpus

The ShrinkWrap wire format is computed in more than one place, on purpose. `#[derive_shrink_wrap]` lays out a
type from its own AST: the introspection model (`ww_self`) is itself encoded with the derive, so the derive can't
depend on it. Everything outside the derive works from the model instead: the dynamic client, Python, the `ww` CLI
and the GUI compute the layout again from introspection data, and generated C (and other languages later) is made
from it too. Instead of a shared intermediate form, one corpus of cases is the contract that keeps them byte for
byte the same.

Each case is a type, a value and the bytes that value has on the wire (golden bytes). Every consumer has to encode
the value to exactly those bytes and decode them back to the value:

| Consumer | How it runs a case |
|---|---|
| `derive` | The value as the Rust type `#[derive_shrink_wrap]` generates (owned), encoded and decoded; where the borrowed type differs, the golden bytes go through it and back too. The reference: it runs every case. |
| `dynamic` | `ww_self::ValueOwned::ser_shrink_wrap_dyn` / `des_shrink_wrap_dyn` with the type from the API model of `tests/conformance_api`, loaded from source as for a device's crate. |
| `python` | `wire_weaver_py`'s `load_api(..).<type>.encode` / `.decode`, with the value as Python gives it. |
| `c` | The C runtime (`shrink_wrap/c`) for a single number, bool, string or byte list; the generated C readers of `fixed_layout` and `head_tail` types, compiled with `cc` and run on the golden bytes. Other types wait for C generated from the model (API-26). |

The [conformance matrix](conformance_matrix.md) lists every case and what each consumer does with it.

## Where it lives

- `tests/conformance/corpus.json`: the cases, the only copy. JSON, so every consumer reads it as it is.
- `tests/conformance_api`: the types. Each type of a case is a property of the `Conformance` trait
  (`property!(rw shape: Shape<'i>)`), which is how the dynamic consumers find it in the API model. Types that
  aren't in the model (a `head_tail` type, `TailBytes`) are defined there but not listed in the trait.
- `tests/conformance`: the runner. `src/derive.rs` has the derive's value of every case, `src/json.rs` turns a
  corpus value into a `ValueOwned`, `src/c.rs` writes and runs the C. One test per consumer in
  `tests/matrix.rs`, plus `corpus` (the file is well formed and `derive.rs` has every case) and `report` (the
  matrix page is up to date).
- `wire_weaver_py/tests/test_conformance.py`: the Python consumer, the same rules.

`just test` runs all four. A failure names the case, the consumer and what differs, the first differing byte for
an encode:

```text
case 'fixed.numbers', consumer c: (the part that runs) decode: C check failed: t_fixed_numbers_a(p) == 18u
case 'fixed.numbers', consumer dynamic: encode: bytes differ at byte 0: got 125634debc9a7808... (46 bytes), golden 135634debc9a7808... (46 bytes)
```

## A case

```json
{
  "name": "struct.flags.relocated",
  "type": "flagged",
  "value": {"a": 5, "early": "hi", "res": {"Ok": 7}, "tagged": {"Pair": {"first": 1, "second": null}}, "late": 300},
  "bytes": "bc686907a0012c0122",
  "gaps": {"c": "no generated C for this type yet: C from the ww_self model (API-26)"}
}
```

- `name`: unique, `area.what`.
- `type`: the property of `Conformance` with the case's type, or the Rust type name of one outside the model.
- `value`: in the form Python uses (`wire_weaver_py/src/convert.rs`): numbers, `true` / `false`, strings, lists
  for lists, arrays, tuples, ranges and bytes, an object of fields for a struct, a list of fields for a tuple
  struct (the value itself for one field), `null` for a unit struct and `None`, `"Variant"` or
  `{"Variant": fields}` for an enum, `{"Ok": ..}` / `{"Err": ..}` for a `Result`, `"<unknown N>"` for a variant
  of a newer version. Every field is given. A number beyond 64 bits is a string, JSON parsers lose it.
- `bytes`: the golden bytes in hex, spaces allowed.
- `direction` (default `both`): `decode` when the bytes decode to the value but the value encodes to something
  else (old data read by a newer type, a newer variant read by an older one), `reject` when the bytes must not
  decode (`value` is `null`), `reject_value` when the value must not encode (no `bytes`).
- `bytes_from`: the bytes are another case's (old data for an evolved type), copied when they are filled in.
- `gaps`: consumer → why it can't run the case yet, ending with the `FEATURES.md` id that fixes it (or saying
  *by design*, or *reader only* for a C reader without a writer). A consumer that can't run an unmarked case, or
  passes a marked one, fails the test: a gap is never skipped silently, and a fixed one is noticed.
- `note`: what the case checks, when the name doesn't say it all.

## Adding a case

1. If the type is new, add it to `tests/conformance_api/src/lib.rs` and a property of that type to `Conformance`.
2. Add the case to `corpus.json` with `"bytes": "?"`, and its value as the Rust type to the table in
   `tests/conformance/src/derive.rs`, under the same name.
3. `just conformance-update`: fills in the golden bytes from the derive and writes the matrix page. Check the bytes
   against what the format docs ([wire format](wire_format.md)) say: once in, they are the contract.
4. `just test`. A consumer that fails gets fixed, or, when the fix is bigger, the case gets a gap mark naming a new
   `FEATURES.md` item for it.

A new wire feature lands with its cases and green across the consumers; a consumer that can't express it yet
has a gap mark and an item, as above. Golden bytes don't change once in: a case whose bytes change is a wire
format change (see [evolution rules](../evolution/rules.md)).
