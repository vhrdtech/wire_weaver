# Alignment

Some types are one- or four-bit aligned and the rest are one-byte aligned. Dense packing is used to
save space, including in enum discriminants (which can even be U1).
Byte arrays, strings, and Unsized objects are
all one-byte
aligned to limit code complexity and computations required. Unused bits are set to zero and can be reclaimed when
evolving a
type.

For example:

```rust
fn version1() {
    let mut buf = [0u8; 8];
    let mut wr = BufWriter::new(&mut buf[..]);
    wr.write_bool(true).unwrap();
    wr.write_u8(0xAA).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, &[0x80, 0xAA]);
}
```

In a future version while older one is still in use, it was decided to add some more data:

```rust
fn version1_1() {
    let mut buf = [0u8; 8];
    let mut wr = BufWriter::new(&mut buf[..]);
    wr.write_bool(true).unwrap();
    wr.write(&Some(U6::new(5).unwrap())).unwrap();
    wr.write_u8(0xAA).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, &[0xC5, 0xAA]);
}
```

Older code can still read new data and will skip the Option, and newer code can read old data, yielding None.
All the while, the serialized size didn't even change (it could have though, it's just an example).

