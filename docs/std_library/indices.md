---
level: advanced
---

# Indices and sequence numbers on the wire

A position in a table (`Index<Of, T, B>`, `IndexFrom1<Of, T, B>`) or a wrapping sequence number (`Seq<Of, T>`) has
the same algebra as [points](points.md): the table's start is the origin, the amounts are `Fixed`s of the same
thing in steps of one. See the book's [Indices and sequence numbers](https://exact.vhrd.tech/points/indices/).

## On the wire and in the model

All three are `transparent`: their bytes are the number's, the table is only in the type. An `Index<Sample, u32>` of
1000 next to a `u8` of 7 is `e8 03 00 00 07`; two `Seq<Frame, Nibble>`, 15 and the one after it, are one byte, `f0`.
`IndexFrom1` refuses 0 on decode, and every bounded one a number outside its bounds. The cases are
`stdlib.si.index.*`, `stdlib.si.index_from_1.*` and `stdlib.si.seq.*` in the [conformance corpus](../serdes/conformance.md).

In the API model they are instances of `Index` and `IndexFrom1` with `[Of, T, B]` and of `Seq` with `[Of, T]`, so a
client knows what a number counts: `ww` and the dynamic client show them as `#3 (Sample)`, `#2 (Item, from 1)` and
`seq 41 (Frame)`, and a number written into one is checked against its type, its bounds and an `IndexFrom1`'s 0 before
it is sent ([in clients](exact_units.md#in-ww-python-and-dynamic-clients)).
