---
level: advanced
---

# Arrays

A property, stream, sink or trait can be an array of resources, written with `[]` after its name:

```rust
#[ww_trait]
trait Board {
    stream!(adc[]: u16);
    sink!(lanes[]: [u8]);
    property!(rw gain[]: f32);
    ww_impl!(port[]: ww_gpio::Bank);
}
```

Methods can't be arrays, put them in an array of traits instead. The length isn't part of the API: the device says
which indices exist with a `valid_indices_root_<name>()` handler returning `ValidIndices` (a range or a list), and
a request for another index is answered `ErrorKind::BadIndex`. Bounds in the API itself (`adc[8]`,
`motor[1..=4]`) are not supported yet. The length of a list or string inside a resource's type is bounded with
[`MaxLen`](../types.md#bounded-lists-and-strings-maxlent-n) instead.

On the host, an array item takes its index first: `device.port(0).pin(3).read_pull()`, `device.adc(2).call()`. On the
device, every handler of an item inside arrays, at any depth, gets the indices leading to it as one more argument,
`index_chain: [UNib32; N]`, after `cx`; `ww api scaffold` prints the exact signatures. The generated code stays
flat: the whole API tree becomes plain function calls on the server struct, with no per-index state.

A whole array can be read, written or called in one request, see [Multi requests](multi_requests.md).

## Array of resources vs resource of array

Here, `led` is itself an array: each index is a resource of its own, with its own path, read and written
separately.

```rust
#[ww_trait]
trait ArrayOfResources {
    property!(rw led[]: bool);
}
```

Here `led` is one resource whose type is an array: all three values are read and written in one go.

```rust
#[ww_trait]
trait ResourceOfArrays {
    property!(rw led: [bool; 3]);
}
```

Both together:

```rust
#[ww_trait]
trait ArrayOfArrays {
    property!(rw rgb_led[]: [u8; 3]);
}
```
