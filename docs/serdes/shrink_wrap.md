# Wire format

All serializing and deserializing operations are going through a wire format called `shrink_wrap`.
It is targeting both microcontroller and host usage.

Features:

* 1-bit, 4-bit and 1-byte alignment
* Support all the types listed on the [types page](../types.md)
* `no_std` without allocator support (even with types like String and Vec, for both reading and writing)
* `std` support (standard Vec and String are used)
* Zero copy deserialization
* Self-referential types
* Built-in mechanism for backwards and forwards compatibility
* Used in auto-generated serdes and API code
* Can be used stand-alone as well

# Next step

Check out available macros that greatly simplify working with the wire format: [derive](./derive.md).