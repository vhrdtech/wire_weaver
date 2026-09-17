# Showcase

A tour of `shrink_wrap` tricks, from plain bit-packing to self-referential types, illustrated with real code from
`wire_weaver` itself and from the `ww_stdlib` crates (`ww_version`, `ww_date_time`, `ww_client_server`, `ww_self`).
Every byte sequence shown below was produced by actually running the snippet - none of it is hand-waved.

Read [wire format](shrink_wrap.md) and [derive](derive.md) first if you haven't; this page assumes you know what
`Unsized`/`final_structure`/`self_describing`/`sized` mean and how `#[derive_shrink_wrap(..)]` decides between
borrowed and owned types.

## Sub-byte packing

`bool` is 1-bit aligned, `nib` (the `Nibble` type) is 4-bit aligned, and `u1`..`u64`/`i2`..`i64` (`U1`, `U2`, ...,
`I2`, ...) are 1-bit aligned regardless of width - they just cost exactly as many bits as you ask for, packed
back-to-back with no padding in between:

```rust
let mut buf = [0u8; 8];
let mut wr = BufWriter::new(&mut buf);
wr.write_bool(true).unwrap();          // 1 bit
wr.write(&U2::new(3).unwrap()).unwrap(); // 2 bits
wr.write(&U1::new(1).unwrap()).unwrap(); // 1 bit
let bytes = wr.finish().unwrap();
assert_eq!(bytes, &[0b1111_0000]); // 1 + 11 + 1 = 4 bits used, rest padded with zeros
```

A whole tuple of 8 bools packs into a single byte, no framing at all:

```rust
let bytes = (true, false, true, false, true, true, false, false)
    .to_ww_bytes(&mut buf).unwrap();
assert_eq!(bytes, &[0b1010_1100]);
```

This is what the `ElementSize::Sized { size_bits }` case (see [derive.md](derive.md#size-assumptions-final_structure-self_describing-sized))
is built on: a `sized` struct/enum's fields are just laid out bit after bit, so putting a `bool` field or a small
enum next to another `Sized` field is free - they share the same byte instead of each getting its own.

## `UNib32`: variable-length numbers by the nibble

`UNib32` encodes a `u32` as 1-or-more nibbles: 1 continuation bit + 3 value bits per nibble.

```rust
assert_eq!(UNib32(0).to_ww_bytes(&mut buf).unwrap(), &[0x00]);   // 1 nibble
assert_eq!(UNib32(7).to_ww_bytes(&mut buf).unwrap(), &[0x70]);   // 1 nibble, max for that size
assert_eq!(UNib32(8).to_ww_bytes(&mut buf).unwrap(), &[0x81]);   // 2 nibbles: needs a continuation
assert_eq!(UNib32(0o777).to_ww_bytes(&mut buf).unwrap(), &[0xff, 0x70]); // 3 nibbles
```

Small numbers (the overwhelming majority in practice - lengths, indices, enum discriminants) cost a single nibble.
It's `self_describing`: a reader always knows where it ends from the continuation bits alone, no length prefix
needed. This is why `ww_version::Version` uses `UNib32` for `major`/`minor`/`patch` instead of `u8`/`u32` - most
real-world version numbers are small, so `Version::new(0, 1, 2)` serializes to just **2 bytes**:

```rust
let version = Version::new(0, 1, 2);
assert_eq!(version.to_ww_bytes(&mut buf).unwrap(), hex!("01 20"));
```

`UNib32` is also what a struct/enum's own reverse-length marker is built from when it's `Unsized` - see
[the FIFO-of-lengths section](#the-fifo-of-lengths-why-strings-can-come-before-their-length) below.

## `Option<T>` and `Result<T, E>`: a flag, not a tag

Both are `SelfDescribing`: one `bool` flag followed by the payload (for `Result`, `true` picks `Ok`, `false` picks
`Err` - both variants exist on the wire, unlike a length-prefixed encoding that would need to know the size of
whichever branch to skip):

```rust
impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Option<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Some(val) => { wr.write_bool(true)?; wr.write(val) }
            None => wr.write_bool(false),
        }
    }
}
```

No size marker is stored for the `Option`/`Result` itself - the flag bit _is_ the delimiter. That's why stacking
several `Option`s in a struct costs almost nothing extra: each is just one more bit plus its payload's bits, all
packed contiguously:

```rust
// from docs/types.md - v1 struct, then evolved with an inserted Option<U6> field
wr.write_bool(true)?;                        // 1 bit
wr.write(&Some(U6::new(5).unwrap()))?;       // 1 (flag) + 6 (value) bits
wr.write_u8(0xAA)?;                          // byte-aligned, starts a fresh byte
// -> [0xC5, 0xAA], same total size as before the field was added
```

## Picking an enum's discriminant width - and when `Option<bool>` beats it

`ww_repr = <width>` sets exactly how many bits an enum's discriminant costs (see
[derive.md](derive.md#ww_repr-repr-enums-only)). Pick the width for the variant count you'll ever need - a 3- or
4-variant enum needs `ww_repr = u2`:

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2, sized)]
enum Speed3 { Slow, Medium, Fast }

assert_eq!(Speed3::Slow.to_ww_bytes(&mut buf).unwrap(), &[0b00_000000]);
assert_eq!(Speed3::Fast.to_ww_bytes(&mut buf).unwrap(), &[0b10_000000]);
```

A `sized` enum like this **always** costs exactly its declared width, no matter which variant. But if your "enum"
is really just "a common default state, plus one bit of extra information when it isn't the default", `Option<bool>`
can be smaller on average, because it's `SelfDescribing` rather than fixed-width:

```rust
assert_eq!(None::<bool>.to_ww_bytes(&mut buf).unwrap(),       &[0b0_0000000]); // 1 bit used
assert_eq!(Some(false).to_ww_bytes(&mut buf).unwrap(),        &[0b10_000000]); // 2 bits used
assert_eq!(Some(true).to_ww_bytes(&mut buf).unwrap(),         &[0b11_000000]); // 2 bits used
```

Same worst case (2 bits, tying the `u2` enum), but the common/default case (`None`) costs only **1 bit** instead of 2. `ww_date_time::Timezone` uses exactly this shape for the same reason - UTC is the overwhelmingly common case:

```rust
#[derive_shrink_wrap(borrowed, derive(Copy, Clone, Debug, ..), sized, ww_repr = u1)]
pub enum Timezone {
    UTC,              // costs 1 bit
    Other(OtherTimezone), // costs 1 bit + OtherTimezone's own bits
}
```

`Timezone` itself _is_ the sized-enum side of this trade-off (`ww_repr = u1`, so `UTC` and `Other(..)` both cost at
least 1 bit) - but notice it's shaped exactly like `Option<OtherTimezone>` would be, and was written as an explicit
enum instead so that `Other`'s payload (`OtherTimezone`, itself `ww_repr = u3, sized`) could be added as a real,
named variant rather than a bare tuple. Same bit-level trick, better ergonomics for a type with more than two
states down the line.

## Relocating presence flags with `#[flag]`

By default, an `Option<T>`/`Result<T, E>` field gets its presence flag written immediately in front of it. `#[flag]`
lets you park that flag bit somewhere else entirely - most usefully, right next to other small fields so they all
land in the same byte instead of the flag re-opening a new bit run right before its `Option`. `ww_version::Version`
does exactly this, grouping both presence flags together right after the three `UNib32` numbers, ahead of the two
`Option<&str>` fields they belong to:

```rust
pub struct Version<'i> {
    pub major: UNib32,
    pub minor: UNib32,
    pub patch: UNib32,
    #[flag]
    build: bool,          // presence flag for the `build` field below, relocated here
    pub pre: Option<&'i str>,
    pub build: Option<&'i str>,
}
```

Flags are tracked in LIFO order - `#[flag]` fields are pushed in declaration order and popped by the `Option`s
that consume them, so `#[flag] build` (pushed first) must be consumed after `pre`'s auto-generated flag (pushed
second, popped first) - the macro checks this ordering at compile time and rejects anything that isn't valid LIFO.
The payoff: `major`/`minor`/`patch`/`build`-flag/`pre`-flag all land in the same 14 bits, so a version with no
pre-release or build metadata (the common case) still fits in **2 bytes**, and a fully-populated one packs its two
strings back to back with no gap for their flags:

```rust
let version = Version::full(0, 1, 2, Some("pre"), Some("build"));
// major/minor/patch nibbles + 2 flag bits, then "pre" bytes, then "build" bytes,
// then (per the FIFO-of-lengths rule below) build's reverse length, then pre's.
assert_eq!(version.to_ww_bytes(&mut buf).unwrap(), hex!("01 2C 707265 6275696C64 5 3"));
```

## The FIFO-of-lengths: why strings can come before their length

An `Unsized` value's length isn't written where the value starts - it's appended to the _back_ of the buffer as a
reversed `UNib32`, so the writer never needs to know a value's size before it has finished writing it (see
[the high-level overview](shrink_wrap.md#high-level-overview)). Multiple lengths stack up in reverse order, forming
a FIFO read from the back:

```rust
let bytes = ("abc", "de").to_ww_bytes(&mut buf).unwrap();
assert_eq!(bytes, hex!("61 62 63 64 65 23"));
//                      a  b  c  d  e  ^^ reversed nibble lengths: 2 ("de"), then 3 ("abc")
```

The last byte, `0x23`, holds both strings' lengths as two reversed nibbles - `"de"` (written second, so its length
is closest to where the FIFO starts) then `"abc"`. Reading walks the same FIFO from the back to know exactly where
each string ends, without ever touching a byte before it needs to.

## Zero-copy vectors, strings and pre-serialized payloads

`RefVec<'i, T>` is the no-alloc, zero-copy stand-in for `Vec<T>` - it stores a reference into the original buffer
(or a native `&[T]` slice) and only decodes elements as you `.iter()` over them:

```rust
let v = RefVec::new_bytes(&[0xAA, 0xBB, 0xCC]); // RefVec<'_, u8>, zero-copy byte array
```

When `owned(feature = "std")` is requested, the exact same field becomes a `Vec<T>` on the owned side - see the
[type mapping table](derive.md#type-mapping).

`TailBytes<'i>` is a more specialized trick used by `ww_client_server`: a byte slice that is _not_ length-prefixed
at all, because it's guaranteed to be the last thing in an already-`Unsized` message (the framer/transport already
knows the whole message's length, so re-encoding the tail's length would be pure waste):

```rust
pub struct Request<'i> {
    pub seq: u16,
    pub path_kind: PathKind<'i>,
    pub kind: RequestKind<'i>, // eventually bottoms out in `Call { args: TailBytes<'i> }`
}
```

Method arguments and property values are serialized once into their own buffer, then handed to the outer message
as `TailBytes` - one buffer, no copying to reframe it, no length to write since it's already implied.

## Self-referential types with `RefBox`

Rust needs a compile-time-known size for any type; a struct or enum can't directly contain itself. `RefBox<'i, T>`
breaks the cycle the same way `Box<T>` does on the heap, but for `no_std`/no-alloc: it stores either a plain `&'i T`
reference or an unparsed `BufReader` slice, and only actually deserializes `T` when you call `.read()`. Serializing
a `RefBox` is nearly free (it forwards straight to `T`'s own serialization, no boxing overhead on the wire), and
deserializing one is a plain buffer-position save - reading the boxed value is deferred until you ask for it:

```rust
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq))]
struct Linked<'i> {
    a: u8,
    next: Option<RefBox<'i, Linked<'i>>>,
}

let linked = Linked {
    a: 1,
    next: Some(RefBox::new(&Linked { a: 2, next: None })),
};
assert_eq!(linked.to_ww_bytes(&mut buf).unwrap(), hex!("01 80 02 00 02"));
//                                                       a1 fl a2 fl len
// a=1, Option-flag=1, a=2, Option-flag=0 (nested Linked has no `next`), reverse length of the boxed Linked
```

`RefBox` composes with `Option`/`Result`/enum variants exactly like any other field, which is how you build actual
recursive data. `ww_self` (the introspection/AST crate) uses this to describe _types themselves_ recursively -
`Type::Vec(RefBox<'i, Type<'i>>)` means "a `Vec` whose element type is this boxed `Type`", `Type::Option(RefBox<..>)`
likewise for `Option<T>`, and so on all the way down. Stripped to the essentials, the same idea looks like this:

```rust
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = nib)]
enum Ty<'i> {
    U8,
    U32,
    Str,
    Vec(RefBox<'i, Ty<'i>>),      // Vec<T>, T described recursively
    Option(RefBox<'i, Ty<'i>>),   // Option<T>
    Tuple(RefVec<'i, Ty<'i>>),    // (T0, T1, ...) - a real collection this time, no RefBox needed
}

// describes the Rust type `Vec<Option<u8>>`
let inner = Ty::U8;
let opt = Ty::Option(RefBox::new(&inner));
let vec_of_opt = Ty::Vec(RefBox::new(&opt));
let bytes = vec_of_opt.to_ww_bytes(&mut buf).unwrap(); // [0x30, 0x40, 0x00, 0x01, 0x03]

let des = Ty::from_ww_bytes(bytes).unwrap();
// des == Vec(RefBox(..)); nothing past the outer variant is decoded yet
if let Ty::Vec(elem_ty) = &des {
    assert_eq!(elem_ty.read().unwrap(), opt); // .read() decodes lazily, one level at a time
}
```

Note `RefVec<'i, Ty<'i>>` needs no `RefBox` even though `Ty` is self-referential - a `RefVec` never holds `T`
inline, only a reference to where its elements live in the buffer, so there's no Rust-level sizing problem to
begin with. `RefBox` is only needed for a _single_ recursive field (`Option<T>`, a boxed enum payload, etc.), never
for a collection of them.

## Patching a discriminant after you've already started writing its payload

Sometimes you don't know which variant you're building until you've already started writing it - a response
builder that reserves space for a result flag or an event kind before it knows whether the call will succeed, for
example. `discriminants` (see [derive.md](derive.md#discriminants-enums-only)) generates a fieldless
`{Name}Discriminants` enum for exactly this, and `BufWriter::save_state`/`restore_state` let you go back and patch
bits you already reserved:

```rust
// ww_client_server's EventKindBuilder, trimmed
pub struct EventKindBuilder { discriminant: BufWriterState }

impl EventKindBuilder {
    pub fn new(wr: &mut BufWriter<'_>) -> Result<Self, Error> {
        let discriminant = wr.save_state();   // remember where the discriminant nibble goes
        wr.write_nib_masked(0)?;              // write a placeholder
        Ok(Self { discriminant })
    }

    pub fn finish_with_kind(self, kind: EventKindDiscriminants, wr: &mut BufWriter<'_>) {
        let after = wr.save_state();          // remember where we ended up
        wr.restore_state(self.discriminant);  // rewind to the placeholder
        _ = wr.write_nib_masked(kind.discriminant()); // patch it in
        wr.restore_state(after);              // resume from where we left off
    }
}
```

Between `new()` and `finish_with_kind()`, the caller writes the variant's actual payload with no idea yet which
`EventKindDiscriminants` it'll turn out to be - `ErrorBuilder` in the same module does the same trick one level
deeper, wrapping the whole payload in an `UnsizedBuilder` so its reverse-length still comes out correct even though
the discriminant in front of it was written last. `ww_client_server`'s tests confirm the builder's output is
byte-for-byte identical to building the equivalent value up front and calling `.to_ww_bytes()` on it directly - the
builder is purely a streaming-friendly way to produce the exact same bytes.

## Putting it together: real wire sizes

A few real types from `ww_stdlib`, each combining several of the tricks above:

- **`ww_version::Version`** - `UNib32` for major/minor/patch, `final_structure` (flattened into its parent, no
  reverse-length of its own), a relocated `#[flag]` for `build`. Result: **2 bytes** for a `0.1.2` with no
  pre-release/build metadata, growing gracefully only when those fields are actually used.
- **`ww_date_time::DateTime`** - `self_describing`, a `Year` newtype storing years as `UNib32` shifted by 2025 (so
  "now" costs a handful of bits, ancient or far-future dates cost more), and a `Timezone` that's `UTC` in 1 bit for
  the common case. A full UTC timestamp with second-level precision packs into **4 bytes**:
  ```rust
  let dt = DateTime::from_ymd_hms_utc_opt(2025, 5, 30, 16, 20, 0, 0).unwrap();
  assert_eq!(dt.to_ww_bytes(&mut buf).unwrap(), hex!("05 F3 96 C0"));
  ```
- **`ww_client_server::PathKind`** - `ww_repr = nib` picking between three addressing modes (`Absolute`,
  `GlobalCompact`, `GlobalFull`), each shaped so the cheapest, most common case (a short absolute path) costs a
  single nibble discriminant plus the path itself, while the most expensive case (routing to an arbitrary
  crates.io trait by name) only costs more when you actually use it.

None of these types hand-roll any bit-twiddling - they're all plain `#[derive_shrink_wrap(..)]` structs/enums built
from the pieces on this page.
