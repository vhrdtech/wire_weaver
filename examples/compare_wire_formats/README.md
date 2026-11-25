# Compare wire formats

Encodes one example value with `shrink_wrap` (WireWeaver's own wire format) and eight other pure-Rust formats
(postcard, MessagePack, JSON, CBOR, wincode, protobuf, FlatBuffers, ASN.1 UPER), and reports each one's byte count
and whether its schema can evolve (a field can be added later without breaking old readers or writers). `cargo run`
prints a transcript; `docs/generated/compare_wire_formats/output.txt` and `sizes.csv` are the same numbers, kept
byte-for-byte by `book_output::check` in `src/main.rs` (stale: `WW_BOOK_OUTPUTS=write cargo run`, or `just
book-outputs` from the repository root). FlatBuffers' Rust bindings are generated once from `schema/request.fbs`
(`planus rust -o src/request_generated.rs schema/request.fbs`, `cargo install planus-cli`) and committed; the rest
need no code generation. This crate is excluded from the main workspace (its own `Cargo.lock`) so these formats
never reach every other build's lockfile; see `just`'s `book-outputs` and `test-all` recipes for how it is still
run and checked.

This example also showcases some of the `shrink_wrap` features that result in dense wire representation of the
following data structures:

```rust
struct Request {
    pub seq: u16,
    pub path_kind: PathKind,
    pub kind: RequestKind,
    pub flags: [bool; 5],
    pub dummy_a: Option<u8>,
    pub dummy_b: Option<u8>,
    pub dummy_c: Option<u8>,
    pub strs: Vec<String>,
}

enum PathKind {
    Absolute { path: Vec<u32> },
    GlobalCompact,
    GlobalFull,
}

enum RequestKind {
    Call { args: Vec<u8> },
    Read,
}
```

With the following example value:

```rust
fn main() {
    let req = RequestSerde {
        seq: 1234,
        path_kind: PathKindSerde::Absolute { path: vec![0, 1, 2] },
        kind: RequestKindSerde::Call { args: vec![0xAA, 0xBB, 0xCC] },
        flags: [false, true, false, true, false],
        dummy_a: Some(0xCC),
        dummy_b: None,
        dummy_c: None,
        strs: vec!["ab".to_string(), "c".to_string()],
    };
}
```

In particular the following features are resulting in a considerably smaller size:

* UNib32 encoding using nibbles, for path array and for all lengths:
    * `D2 04` is u16 in LE
    * `00 12` first nibble is PathKind discriminant, then `0, 1, 2` is path array, nibbles are 4-bit aligned, so the two
      stick together
    * `00` first nibbles is RequestKind discriminant, second one is padding before u8 args array
    * From the back: `01 22 53` are the lengths of the objects, string array and string themselves - 3, 5, 2, 2, 1,
      padding
* Boolean values stored as bits - `0b0010_1010 = 0x2A`
* Option's flag grouping - see the source on how the flags for Option's are relocated to be grouped with the bool array.

Note that ShrinkWrap format while being smaller, also supports backwards and forwards compatibility.

Results (`docs/generated/compare_wire_formats/sizes.csv` from the repository root has the exact, checked numbers;
`output.txt` has every format's bytes):

| Format | Bytes | Evolvable |
|---|---|---|
| WireWeaver | 18 | yes |
| Postcard | 27 | no |
| MessagePack | 49 | no |
| JSON | 193 | yes |
| CBOR | 113 | no |
| wincode | 77 | no |
| Protobuf | 34 | yes |
| FlatBuffers | 144 | yes |
| ASN.1 UPER (closed) | 29 | no |
| ASN.1 UPER (extensible) | 29 | yes |

ASN.1 UPER costs the same whether or not it is extensible here: the extension bit fits in a byte boundary that is
otherwise padding, so this message gets forward compatibility for free. FlatBuffers pays the most for being
zero-copy on the read side (a vtable per table, offsets instead of inline values); MessagePack and CBOR pay for
self-describing field names per nested variant while still laying out the outer struct's own fields positionally,
so, as encoded here (the common case for structs with serde), neither can gain a field without breaking old
readers.