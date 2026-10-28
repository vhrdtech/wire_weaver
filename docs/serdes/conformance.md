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
| `explain` | What [`ww explain`](../cli_reference.md#ww-explain) is made of: the value written as a Rust expression (`ww_self::expr::to_expr`) and read back (`parse_value`) before it is encoded; the golden bytes decoded by `ww_self::explain`, whose map of the bits must hold: every bit in exactly one part (a value, a length, a discriminant, a flag, padding, a header, bytes skipped from a newer version). |
| `python` | `wire_weaver_py`'s `load_api(..).<type>.encode` / `.decode`, with the value as Python gives it. |
| `c` | The C runtime (`shrink_wrap/c`) for a single number, bool, string or byte list; the generated C readers of `fixed_layout` and `head_tail` types, compiled with `cc` and run on the golden bytes. Other types wait for C generated from the model (API-26). |

The [conformance matrix](conformance_matrix.md) lists every case and what each consumer does with it.

## Where it lives

- `tests/conformance/corpus.json`: the cases, the only copy. JSON, so every consumer reads it as it is.
- `tests/conformance_api`: the types. Each type of a case is a property of the `Conformance` trait
  (`property!(rw shape: Shape<'i>)`), which is how the dynamic consumers find it in the API model. Types that
  aren't in the model (a `head_tail` type, `TailBytes`) are defined there but not listed in the trait.
- `tests/conformance`: the runner. `src/derive.rs` has the derive's value of every case, `src/json.rs` turns a
  corpus value into a `ValueOwned`, `src/c.rs` writes and runs the C, `src/explain.rs` checks the bit map. One test per consumer in
  `tests/matrix.rs`, plus `corpus` (the file is well formed and `derive.rs` has every case), `report` (the
  matrix page is up to date), `frozen_bytes` ([frozen golden bytes](#frozen-golden-bytes), TEST-24,
  `src/frozen.rs`) and `coverage` ([coverage guard](#coverage-guard), TEST-25, `src/coverage.rs`).
- `wire_weaver_py/tests/test_conformance.py`: the Python consumer, the same rules.

`just test` runs all five. A failure names the case, the consumer and what differs, the first differing byte for
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
- `wire_change` (default none): `"EVO-n / reason"`, the escape the [frozen golden bytes guard](#frozen-golden-bytes)
  accepts for a deliberate change to this case's `bytes`, `direction` or `bytes_from`.

## A requantize case

The `requantize` list holds conversions instead of bytes (STD-63, [Rounding and overflow](../std_library/rounding.md)): a
stored number of one scale and storage converted to another with a named rounding and overflow policy, which every
consumer has to give bit for bit.

```json
{
  "name": "requantize.bam16_to_bam8.full_turn.wrap",
  "conversion": "BAM16 -> BAM8",
  "factor": {"pow10": 0, "rest": {"pow2": -16, "irrational": null, "factors": [null, null, null, null, null, null, null, null, null, null, null, null]}},
  "to": "u8",
  "value": 65408,
  "rounding": "nearest_even",
  "overflow": "wrap",
  "result": 0
}
```

- `conversion`: the typed conversion the derive runs, by its id in `tests/conformance/src/requantize.rs`
  (`Fixed::requantize::<P2, T2>`); the derive also checks that `factor` is the quotient of its scales.
- `factor`: the quotient of the scales, a `magnitude` value, which is all a dynamic client has: it reads the factor
  through the model and runs `exact_units::Requantizer` with the target storage `to`.
- `rounding` (`truncate`, `floor`, `ceil`, `nearest_even`, `nearest_away`), `overflow` (`checked`, `saturate`,
  `wrap`) and `result`, `null` for no value.
- `gaps`, `note` and `wire_change` as for a case: a `result` is as frozen as golden bytes.

Results are worked out independently of the Rust code (exact fractions, and 60 digits of π for scales with π).
Python and C have no unit conversions yet and carry a gap mark naming CLT-54 on every case.

## Frozen golden bytes

Once a case's `bytes` are in, they are the contract: nothing should change them, remove the case, or change its
`direction` or `bytes_from` without everyone noticing. `CONFORMANCE=update` on its own only refuses to *overwrite*
bytes already filled in (`corpus::fill_in_bytes` skips a case unless `to_fill()`); a plain text edit, or resetting
`bytes` back to `"?"` and running the update again, bypasses that and re-seals silently.

The `frozen_bytes` test (`tests/matrix.rs`, `conformance::frozen`, TEST-24) catches this independently: it reads
`tests/conformance/corpus.json` as it was at a baseline commit with `git show <sha>:tests/conformance/corpus.json`,
and compares every case that existed there against the corpus on disk now. A case whose `bytes`, `direction` or
`bytes_from` changed, or that disappeared, fails the test by name with the old and new values — so a session
branch that touches sealed bytes fails `just test` and the landing gate, which runs on the branch rebased onto
main.

The baseline is `CONFORMANCE_BASE=<sha>` when it names a real commit, otherwise `git merge-base HEAD
origin/master`, falling back to local `master`. The default is right for a pull request or a local run (where
`origin/master` still differs from `HEAD`), but not for CI running on a push to master: there, `origin/master` is
already `HEAD` right after the push, so the merge base would be `HEAD` itself and the guard would compare the
corpus with itself, never actually checking the pushed commits. CI's `test` and `coverage` jobs set
`CONFORMANCE_BASE` to the pushed range's prior commit (`github.event.before`) on a push, empty otherwise (which
falls through to the default lookup); `github.event.before` is an all-zero sentinel when a branch has no prior
commit (its first push), which the guard treats as nothing to compare against, same as not finding a merge base.

The baseline corpus is parsed with its own minimal, forward-compatible schema (`frozen::LenientCase`: `name`,
`bytes`, `direction`, `bytes_from`, `wire_change`, everything else ignored), not the current `Case` struct used
for the corpus on disk: a field added, renamed or made required since the baseline commit would otherwise make
the whole file fail to parse against today's stricter schema, turning this guard off silently right when the
corpus is being reshaped — exactly when a bytes change could hide in the same commit. A baseline JSON that still
fails this lenient parse (not JSON at all, or no `cases` array) isn't a schema difference, it's not a corpus at
all, so that fails the test loudly instead of being treated as nothing to check.

It depends only on the repository's own git history, never the network or anything outside the repo: without git,
without a baseline commit to resolve (a shallow clone with only the branch under test, say, or a `CONFORMANCE_BASE`
that doesn't resolve), or without the corpus existing yet at that commit (introducing TEST-24 itself, or the corpus
as a whole), the test prints why and passes rather than failing a check it can't actually run. CI's `test` and
`coverage` jobs fetch full history (`fetch-depth: 0`) so the check means something there; a shallower checkout
elsewhere only loses the check, not correctness.

A deliberate wire break is still possible: mark the changed case with `"wire_change": "EVO-n / reason"`, which the
guard accepts once — only when the mark is new or its text changed since the baseline. A case whose mark is
unchanged from the baseline is sealed again at its current bytes: reusing the same mark across unrelated later
edits would otherwise exempt that case from this guard forever instead of for one change, so a second deliberate
change needs its own new or reworded mark (a fresh `EVO-n`, or at least different wording). The mark must come
with the matching `EVO-n` item in `FEATURES.md` and a `### ⚠️ Breaking` entry in the affected crate's
`CHANGELOG.md` saying what changed and why — the mark on its own is not a justification, just an acknowledgment
that the matrix report also shows (a "Wire changes" section, `just conformance-update` writes it). Once the branch
carrying the change lands on main, the mark has done its job and the next guard run treats the new bytes as sealed
again; remove it once that is true (it is harmless to leave, but do not reuse it for a later, unrelated change).

## Coverage guard

A corpus can go stale the other way too: a wire feature lands without ever being exercised by a case, or a case
that used to be the only cover for a feature is deleted. The `coverage` test (`tests/matrix.rs`,
`conformance::coverage`, TEST-25) checks two things every case collectively must cover:

- Every variant of `ww_self::Type` (what a case's type resolves to in the API model: `Bool`, `Struct`, `Option`,
  ...) reachable from some case's type, walking `Vec`/`Array`/`Option`/`Result`/`Box`/`MaxLen`/`Series`/`Tuple`
  and following `OutOfLine`/`Instance` into the bundle's own `types` array (self-referential types are visited
  once). The walk is an exhaustive match: a new `Type` variant fails to compile there instead of being silently
  uncovered.
- Every wire-relevant `shrink_wrap_derive` directive (`final_structure`, `self_describing`, `sized`, `fixed_layout`,
  `head_tail`, `sparse`, `validate = ..`, `ww_repr = ..`, `series`, `#[series(opaque)]`, `no_future_version`,
  `#[flag]`, `#[default = ..]`) used by some type in `tests/conformance_api`. Directives that don't change
  serialized bytes (which Rust type is generated, derives, doc/display, naming) are left out rather than tracked.

A kind not exercised by any case, and not listed, fails the test by name. A kind that *is* listed (`TYPE_GAPS` /
`DIRECTIVE_GAPS` in `conformance::coverage`) needs a reason ending in a `FEATURES.md` id, same rules as a case's own
`gaps`; a listed kind that a new case now exercises also fails, so the mark gets removed once it no longer applies.
Add the missing case (`just conformance-update`, checked against [the wire format](wire_format.md)) to close a
gap, or add the kind with its reason if a case isn't possible yet.

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
