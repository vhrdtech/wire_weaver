# Wire format

<p align="center">
<img src="https://github.com/romixlab/shrink_wrap/blob/main/assets/logo-256.png?raw=true" alt="logo"/>
</p>

All serializing and deserializing operations are going through a wire format called `shrink_wrap`.
It is targeting both microcontroller and host usage.

Features:

* 1-bit, 4-bit and 1-byte alignment
* Support all the types listed on the [types page](../types.md)
* `no_std` without allocator support (even with types like String and Vec, for both reading and writing)
* `std` support (standard Vec and String are used)
* Zero-copy deserialization
* Self-referential types
* Built-in mechanism for backwards and forwards compatibility

Used in auto-generated serdes and API code and it can be used stand-alone as well.
Note that understanding how use serdes system manually is optional, as most of the code is automatically generated.
Feel free to continue to the [next step](#next-step).

## High-level overview

Main idea behind the wire format is a stack of sizes that is kept in the back of the buffer. This not only allows to do
serialization in one pass, but also avoid unnecessary copying.

Let's see how this works on a simple example: serialize two strings of arbitrary length into a byte buffer.
One string is trivial, because assumption is that buffer length is known, hence two.
Of course, we have to do it in such a way as to be able to get both strings back during deserialization stage, so we
have to encode the lengths as well.

Many formats are serializing length and then object bytes, like so:
pic: l1 abc l2 qwerty

There is a problem with this approach though: we need to know the object length, before we can write its bytes.
Seems like a weird problem, because the string is right there, just use its length?
But imagine that instead of a string, we are writing a complex object, consisting of many nested structs or even vectors
of them.

So we would either have to do a sizing pass and go through all that data first to figure out resulting length
(and make sure its actually exactly correct).

Or write a dummy length first, serialize the whole thing and then come back and fix the length.
The trouble with that, is that we don't yet know how many bytes the length itself will take, and we would like to use
variable length encoding to save space.

We could limit the object size to e.g., 256 bytes, but this is unnecessarily small, while 65536 using 2 bytes is already
too big for most things embedded. Or we could assume maximum length first, and then shift serialized bytes when we know
the actual size, it would be nice to avoid this copy operation though.

Core types of the wire format are -
[BufWriter](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/src/buf_writer.rs) and
[BufReader](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/src/buf_reader.rs).

Currently, both of them work on byte slices, though Vec based buffer is planned for more convenient use on std.
There are no alignment requirements imposed on the slices (i.e., the alignment is 1 byte).

### BufWriter

BufWriter is created from a mutable byte slice (which does not have to be initialized to zero, potentially saving a bit
of init time). BufWriter is keeping several indices into the provided slice to keep track of the current position.

```rust
use wire_weaver::shrink_wrap::prelude::*;

fn simple_wr() {
    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    wr.write_u8(0xaa).unwrap();
    let bytes = wr.finish().unwrap();
    assert_eq!(bytes, &[0x80, 0xaa]);
}
```

## write/read vs ser_shrink_wrap/des_shrink_wrap

[write](https://github.com/vhrdtech/wire_weaver/blob/master/shrink_wrap/src/buf_writer.rs#:~:text=pub%20fn%20write%3CT)

For Unsized types, size is read from the back of the buffer as reverse UNib32.
Then [split](BufReader::split) reader is used to actually deserialize the value.

Note that values deserialized with this method must be serialized with [write](crate::BufWriter::write).
Values serialized with [ser_shrink_wrap](SerializeShrinkWrap::ser_shrink_wrap) must be
deserialized with [des_shrink_wrap](DeserializeShrinkWrap::des_shrink_wrap).

to_ww_bytes/from_ww_bytes

## Next step

Check out available macros that greatly simplify working with the wire format: [derive](./derive.md).