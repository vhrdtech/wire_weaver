---
level: advanced
---

# Indices and sequence numbers

A [`Fixed`](exact_units.md)`<Sample, One, u32>` is an amount: 50 samples, 512 bytes. A position in a table is
something else: sample number 100 of a capture, byte 0x40 of a register map, channel 3 of 4. A sequence number is a
position on a circle: frame 250, then 251, ..., 255, 0, 1. `exact_units` (feature `counting`, re-exported by `ww_si`)
has a type for each, with the same algebra as [points](points.md): the table's start is the origin, the amounts are
`Fixed`s of the same thing in steps of one.

| Type                    | What                                           | Refused                          |
|-------------------------|------------------------------------------------|----------------------------------|
| `Index<Of, T, B>`       | 0-based position in a table of `Of`            | outside `B`                      |
| `IndexFrom1<Of, T, B>`  | 1-based position, as people and datasheets count | 0, outside `B`                 |
| `Seq<Of, T>`            | wrapping sequence number of `Of`               | nothing, every `T` is one        |

`Of` names what is counted: a counted marker (`Item`, `Sample`, `Frame`, `Packet`, `Cycle`, `Block`, `Sector`,
`Chunk`, `Char`, `Code`, or an API's own from `unit!`), or any other unit (`Byte` for an offset). `T` is an unsigned
storage (`u8` .. `u64`, `Nibble`, `UNib32`, `UVlq64`). `B` are `Fixed`'s bounds markers, `Unbounded` by default.

## Index

```rust
use exact_units::{Fixed, Index, units::Sample};

let start = Index::<Sample, u32>::new(100);
let end = start + Fixed::new(50);
assert_eq!(end.get(), 150);
// two positions are apart by an amount of samples
assert_eq!(end - start, Fixed::new(50));
assert!(start < end);
```

| Expression              | Result                                   |
|-------------------------|------------------------------------------|
| index + amount          | index                                    |
| index − amount          | index                                    |
| index − index           | amount (`Fixed<Of, One, T>`, a distance) |
| index + index           | doesn't compile                          |
| index · x               | doesn't compile                          |
| index of `Sample` + amount of `Frame` | doesn't compile            |
| index < index           | compares the numbers                     |

The operators behave like the storage's own (a panic on overflow in debug builds); `checked_add`, `checked_sub` and
`checked_distance` return `None` instead. A table of 8 is `Range<0, 7>`, checked on construction (`try_new`) and on
every read from the wire; a bounded index moves only with the checked forms:

```rust
use exact_units::{Fixed, Index, Range, units::Item};

type Slot = Index<Item, u8, Range<0, 7>>;
let last = Slot::try_new(7).unwrap();
assert!(Slot::try_new(8).is_err());
assert_eq!(last.checked_add(Fixed::new(1)), None);
assert_eq!(last.checked_sub(Fixed::new(7)), Some(Slot::try_new(0).unwrap()));
```

## IndexFrom1

Channel 1 to 4, pin 1, motor 1 to 4: numbers as people count them, never 0. `B` bounds the number as shown
(`Range<1, 4>` for four channels); `to_index` and `from_index` go to and from the 0-based position.

```rust
use exact_units::{Index, IndexFrom1, Range, units::Item};

type Channel = IndexFrom1<Item, u8, Range<1, 4>>;
let ch = Channel::try_new(3).unwrap();
assert_eq!(ch.to_string(), "3");
assert_eq!(ch.to_index(), Index::new(2));
assert_eq!(Channel::from_index(Index::<Item, u8>::new(0)), Some(Channel::try_new(1).unwrap()));
assert!(Channel::try_new(0).is_err());
assert!(Channel::try_new(5).is_err());
```

## Seq

A request's, an envelope's or a frame's number: `Seq<Frame, u16>`, `Seq<Packet, Nibble>`. It moves on modulo 2^bits
of `T` (`next`, `wrapping_add`, `wrapping_sub`, `+` and `−` of an amount) and compares in serial number order
(RFC 1982): `a` is before `b` when `b` is ahead by more than 0 and less than half the range, so the order holds
across the wrap.

```rust
use exact_units::{Fixed, Seq, units::Frame};

let a = Seq::<Frame, u8>::new(250);
let b = a.wrapping_add(10);
assert_eq!(b.get(), 4);
assert!(a.is_before(b));
assert!(b.is_after(a));
assert_eq!(a.distance(b), Fixed::of(10));
assert_eq!(a.next().get(), 251);
```

`distance` is the steps forward from one to the other, a `Fixed<Of, One, T, Wrapping>`. `serial_cmp` gives the order
as an `Option<Ordering>`: two numbers exactly half the range apart have none. A `Seq` has no `<` and no `Ord`
(integer order is wrong across the wrap, and serial order isn't transitive), and two sequence numbers don't add.

## On the wire and in the model

All three are `transparent`: their bytes are the number's, the table is only in the type. An `Index<Sample, u32>` of
1000 next to a `u8` of 7 is `e8 03 00 00 07`; two `Seq<Frame, Nibble>`, 15 and the one after it, are one byte, `f0`.
`IndexFrom1` refuses 0 on decode, and every bounded one a number outside its bounds. The cases are
`stdlib.si.index.*`, `stdlib.si.index_from_1.*` and `stdlib.si.seq.*` in the [conformance corpus](../serdes/conformance.md).

In the API model they are instances of `Index` and `IndexFrom1` with `[Of, T, B]` and of `Seq` with `[Of, T]`, so a
client knows what a number counts; showing them as positions (`#3 (Sample)`, `seq 41`) and checking 0 and the bounds
in dynamic clients is CLT-54.
