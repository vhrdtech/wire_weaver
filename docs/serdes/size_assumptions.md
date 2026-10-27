---
level: advanced
---

# Size assumptions: `final_structure`, `self_describing`, `sized`

By default every derived type is evolvable (`Unsized`): its serialized size isn't known up front, so when it is
nested inside another struct or enum, the parent stores a length for it (a nibble-based reverse length, like
`UNib32`). That length is what lets fields be added later without breaking anyone (see
[evolving types](derive.md#evolving-types)).

Three directives opt a type out of that flexibility in exchange for a smaller wire size. They are mutually
exclusive, and they are part of the type's wire contract: **once released, a choice can't be undone.** (Why the
lengths exist at all: [the FIFO of lengths](wire_format.md#the-fifo-of-lengths).)

## The options

| Directive         | `ElementSize`           | Meaning                                                                                                                       | Trade-off                                                                                                       |
|-------------------|-------------------------|-------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------|
| _(none)_          | `Unsized`               | Size unknown, stored as a reverse length in the parent.                                                                       | Fully evolvable.                                                                                                |
| `final_structure` | `UnsizedFinalStructure` | Size still unknown, but the type is flattened onto its parent: no length is stored for it; its own lists still have theirs.  | Can't evolve. Only the first version of a type can be `final_structure`, and it can't be removed later.          |
| `self_describing` | `SelfDescribing`        | No size is stored; a reader can tell where the value ends from the encoding itself.                                          | No overhead, but only for types whose encoding ends by itself (variable-length numbers, enums of unequal variants). |
| `sized`           | `Sized { size_bits }`   | Size is fixed and known at compile time, computed from the fields. Nothing is stored for it, ever.                           | Can't evolve at all: adding, removing or resizing a field changes the layout for every reader. Most compact.    |

A struct of one field can also take its field's size class with [`transparent`](derive_reference.md#transparent)
instead of one of these: a `Count(u64)` is `Sized { size_bits: 64 }`, a wrapped `UNib32` `SelfDescribing`, a wrapped
`&str` `UnsizedFinalStructure`, with the bytes of the plain field. It excludes all three.

!!! tip "Free bits can still be used"
    A `final_structure` or `sized` type can still grow into the gaps it already has, which are zero by default. If
    a `bool` is followed by a `u8`, 7 bits are unused and skipped by readers; a later version can put a `bool` or an
    `Option` flag there, and old data reads as `false` / `None`.

## What it looks like on the wire

`ww_repr` alone does **not** make an enum compact: without one of these directives it stays `Unsized`, exactly like
a struct.

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2)]
enum SpeedUnsized { Slow, Medium, Fast, Turbo }

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2, sized)]
enum SpeedSized { Slow, Medium, Fast, Turbo }

// A `Sized` field: its 2 bits are known up front, so the next field packs into the same byte.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct PayloadWithSized {
    speed: SpeedSized,
    after: bool,
}

// An `Unsized` field: the struct stores a length for it, and the next field starts on a fresh byte.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct PayloadWithUnsized {
    speed: SpeedUnsized,
    after: bool,
}

fn compact_vs_not() {
    let mut buf = [0u8; 4];
    // Sized, so (Slow, Turbo) packs into 2+2 = 4 bits of a single byte:
    let bytes = (SpeedSized::Slow, SpeedSized::Turbo).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, &[0x30]);

    // Unsized, standalone: no length needed, since nothing follows it.
    let mut buf2 = [0u8; 4];
    let bytes2 = SpeedUnsized::Turbo.to_ww_bytes(&mut buf2).unwrap();
    assert_eq!(bytes2, &[0xC0]);

    // 0b11_1_00000: Turbo (11), then `after` (1), then padding.
    let mut buf3 = [0u8; 4];
    let sized_payload = PayloadWithSized { speed: SpeedSized::Turbo, after: true };
    let bytes3 = sized_payload.to_ww_bytes(&mut buf3).unwrap();
    assert_eq!(bytes3, &[0xE0]);

    // `speed` is padded to its own byte; `after` starts a new byte, and the length of
    // `speed` (1) shares its low bits:
    let mut buf4 = [0u8; 4];
    let unsized_payload = PayloadWithUnsized { speed: SpeedUnsized::Turbo, after: true };
    let bytes4 = unsized_payload.to_ww_bytes(&mut buf4).unwrap();
    assert_eq!(bytes4, &[0xC0, 0x81]);
    // bool and reverse length end up in the same byte, and there are even 3 bits left
}
```

## Enums

Only one variant of an enum is on the wire, so its size is the discriminant (see
[`ww_repr`](derive_reference.md#ww_repr-repr-enums-only)) plus the fields of _one_ variant:

- `sized` needs every variant to take the same number of bits (unit variants take 0), and a fixed-size
  discriminant (anything but `unib32`). `ELEMENT_SIZE` is then `Sized { size_bits: discriminant + variant }`.
- Variants of different sizes make the enum `self_describing`: the discriminant tells how long the value is. Asking
  for `sized` is a compile error that names the variants and their sizes:

    ```text
    sized enum Reading: variants differ in size (Short: 8 bits, Long: 32 bits); use self_describing instead of
    sized, or pad the variants to one size
    ```

`sized` and `self_describing` give the same bytes, for the type and for whatever contains it, so switching between
the two is not a wire change.

## Checked at compile time

Whichever assumption you pick, the macro inserts a compile-time assertion that the type's _actual_ `ELEMENT_SIZE`
matches the one asked for. If a field turns out to make the type `Unsized` after all, the build fails instead of
silently producing an incompatible layout.

User types are counted wherever they appear in a field (`Point`, `[Point; 3]`, `(Point, u8)`, `Option<Point>`,
`Range<Point>`): the macro can't see their size, so it emits a const expression over their `ELEMENT_SIZE` and the
compiler folds it. Array lengths can be any const expression; a `sized` type's `ELEMENT_SIZE` carries them
symbolically.

`size_bits` is the sum of the fields' own bits. Alignment padding between fields depends on where the value starts
in the buffer and is not part of it: `struct { a: bool, b: u8 }` is `Sized { size_bits: 9 }` and takes 2 bytes on
its own. How sizes combine in detail: [traits and `ElementSize`](wire_format.md#traits-and-elementsize).
