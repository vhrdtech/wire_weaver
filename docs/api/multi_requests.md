# Multi requests

A multi request (CLT-15) answers several resources in one request/response pair instead of one round trip per
resource: several different properties or methods at the same level, or the same property/method across a whole
array. Each index is answered on its own, so one bad index never fails the others. Use them to cut round trips on
a slow or high-latency link - reading a dashboard's worth of properties, writing several settings at once,
backing up or restoring a whole array - not for a single resource, where a plain read/write/call is simpler and
already one round trip.

## Server side: `multi_req = true`

A server only answers `MultiRead`, `MultiCall` and `MultiWrite` when it opts in:

```rust
mod server_impl {
    wire_weaver::ww_codegen!(
        my_device_api :: MyDevice for super::Server,
        server = true, no_alloc = true, use_async = false,
        multi_req = true,
    );
}
```

Without it, a server refuses the request. With it, the dispatcher gets one more arm that treats every index of
`MultiCall`/`MultiRead`/`MultiWrite` as a `Call`/`Read`/`Write` of its own: an index that fails is answered with an
error in its own slot and the rest go on, the request never panics on malformed input (an unknown path, a mode the
server doesn't support, a path at the depth limit), and it is answered with `EventKind::Value` holding an
[`EitherAnyVec`](#the-reply-an-eitheranyvec-of-results) of `Either<Error, value>`, one per index, in index order.

**Cost.** This is not free: every handler whose reply can end up inside a multi response gets an extra branch in
its serialization path (write the value directly, or into the shared `EitherAnyVec`, decided at runtime by one
`bool`), on top of the multi dispatcher itself. Flash use grows with the number of resources the server has, not
just once for the feature - `multi_req = true` is an opt-in for this reason, not the default.

## `MultiIndex`: which resources

`multi_idx: MultiIndex<'i>` names the indices at the one path element where they diverge (see
[`split_common`](#the-generated-client-api) on the client side for how that element is found):

| Variant | Meaning |
|---|---|
| `List(indices)` | exactly these indices, answered in the given order |
| `Range(a..b)` | `a..b`, ascending; `a >= b` is an empty list, not an error |
| `Mask32(bits)` | bit `i` set selects index `i`, so up to 32 indices in one request |

`in_each_array_id: Option<UNib32>` is one more path element, the same for every index, right after the varying one
- e.g. `pin[]` selected by `multi_idx`, then `set_level` inside each selected pin. The request's own path is the
common prefix shared by every index (everything before the varying element).

## The reply: an `EitherAnyVec` of results

The answer is `shrink_wrap::either_any_vec::EitherAnyVec` ([SW-10](../serdes/special_types.md#optionanyvec-sparse-slots-of-any-types)),
read with `EitherAnyVecIter`: the type of each slot (the error type, and the index's own value type) is known to
both sides from the API model, not stored, and the element count is implied by `multi_idx.len()` rather than
written - `EitherAnyVec` only stores one bit per element (whether it is `Left`/error or `Right`/value) plus the
bytes of whatever each element actually is, packed with no padding between elements. It is the same low-level
machinery `OptionAnyVec` is built on ([`sparse` structs](../serdes/layouts.md#sparse-a-struct-of-optional-fields)
use that one), but for a vec where every slot is present and holds one of two types instead of an optional value
of one type.

### A byte-level example

Three indices answered as `Either<SimpleError, _>`: the first fails (`SimpleError::BadIndex`), the second is a
`u8` (`7`), the third a `u16` (`0x1234`) - the shape of a `MultiRead` reply where one index is out of range:

```rust
use either::Either;
use shrink_wrap::either_any_vec::EitherAnyVecWriter;
use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u8)]
enum SimpleError {
    BadIndex,
}

let mut buf = [0u8; 16];
let mut wr = EitherAnyVecWriter::new(&mut buf).unwrap();
wr.write::<SimpleError, ()>(Either::Left(SimpleError::BadIndex)).unwrap();
wr.write::<SimpleError, u8>(Either::Right(7)).unwrap();
wr.write::<SimpleError, u16>(Either::Right(0x1234)).unwrap();
let bytes = wr.finish_and_take().unwrap();
assert_eq!(bytes, [0x60, 0x00, 0x07, 0x34, 0x12, 0x01]);
```

`60`: the L/R flag byte, one bit per element MSB-first (`011` then padding - `Left`, `Right`, `Right`); `00` the
`BadIndex` discriminant; `07` the `u8`; `34 12` the `u16` (little-endian); the trailing `01` is the length (in
bytes) of the one value that is `Unsized` (`SimpleError`, since it wasn't marked `sized`) - `EitherAnyVec` stores
unsized elements' lengths in reverse order at the back, as `OptionAnyVec` does.

## The generated client API

### Several resources at the same level

`MultiRead<Out>`, `MultiCall<Out>` and `MultiWrite<Out>` (`wire_weaver_client::MultiRead`/`MultiCall`/`MultiWrite`)
are implemented for tuples of 2 to 8 of the generated `PreparedRead`/`PreparedCall`/`PreparedWrite` builders, at
any depth - any resources that share a common path prefix and diverge at exactly one element, optionally followed
by one more shared element (e.g. the same property across two array elements):

```rust
// N > 2, any depth, not just a root 2-tuple
let (x, y, mode) = (client.read_x(), client.read_y(), client.read_mode())
    .multi_read()
    .await
    .unwrap();

let (wx, wy, wcustom) = (
    client.write_x(1),
    client.write_y(2),
    client.write_custom(custom),
)
    .multi_write()
    .await
    .unwrap();
wx.unwrap();
wy.unwrap();
wcustom.unwrap();
```

Each item comes back as `Result<T, ErrorOwned>` in the same order the tuple was given. `MultiArgs::Same`/
`Different` - whether every index gets the same serialized argument/value or its own - is chosen automatically by
comparing the serialized bytes of each item.

### The same resource across a whole array

`read_array`/`call_array`/`write_array` (+ `_blocking`, `wire_weaver_client::multi`) take a `MultiIndexOwned`
selector and the existing generated per-index accessor as a closure, called once per index locally (no network
traffic) just to find the common path:

```rust
use wire_weaver_client::multi::{read_array, write_array};
use ww_client_server::MultiIndexOwned;

// write different: one value per channel, in list order
let r = write_array(
    MultiIndexOwned::List(vec![UNib32(0), UNib32(1), UNib32(2), UNib32(3)]),
    |i| client.ch(i).write_level((i as u8 + 1) * 10),
)
.await
.unwrap();

// read range: same order back
let r = read_array(MultiIndexOwned::Range(UNib32(0)..UNib32(4)), |i| {
    client.ch(i).read_level()
})
.await
.unwrap();
```

A method with a custom `#[errors(..)]` result (`CallError<E>`) can't go into a tuple `multi_call` or `call_array`;
call it on its own instead - see [Errors](errors.md#which-errors-a-method-returns).

## The dynamic client

`DynResource::multi_read`/`multi_write`/`multi_call` are the same three operations without generated code: `child`
names the resource inside the array's trait (`None` for an array of a flat property/method, e.g.
`property!(led[]: bool)`), and `DynMultiArgs`/`DynMultiValue` give `Same` (one value/argument list for every index)
or `Different` (one per index, in `multi_idx` order):

```rust
let channel = root.child("channel").unwrap();
channel
    .multi_write(
        cmd,
        MultiIndexOwned::List(vec![UNib32(0), UNib32(1), UNib32(2)]),
        Some("gain"),
        DynMultiValue::Different(&[num(N::F32(0.1)), num(N::F32(0.2)), num(N::F32(0.3))]),
    )
    .await
    .unwrap();
```

## Errors per index, and for the whole request

Most problems land on the one index they belong to and leave the rest answered: an unknown path or index
(`BadIndex`, `BadPath`), a property or method the device left out (`Unimplemented`), one entry of `MultiArgs::Different`
that doesn't deserialize (`ArgsDesFailed`), a write whose value doesn't fit its property (`PropertyDesFailed`). A
deferred method called inside a multi request answers `OperationNotSupported` in its slot instead of running (see
[Limits](#limits)) - the rest of the request still goes through.

The whole request fails instead (no `EitherAnyVec` at all) only for something that isn't about one index: the path
is deeper than the depth limit (`BadPath`), `MultiArgs::Different`/`MultiArgs::Different` itself doesn't deserialize
as a list (`ArgsDesFailed`), or the answer doesn't fit the reply buffer (`ResponseSerFailed`) - which is also why a
huge `Range` (e.g. `0..u32::MAX`) fails outright rather than running for billions of indices.

## Limits

- **Depth**: the request's own path plus the varying element plus an optional `in_each_array_id` must stay within
  `MAX_DEPTH` (16, hardcoded - [API-12](../features.md) would calculate it per API instead); a path one level over
  fails the whole request with `BadPath`.
- **Deferred methods** are refused with `OperationNotSupported` inside a multi request, even one that defers
  although its `method_model` says `immediate` ([deferred methods](methods.md#deferred) need a reply tied to one
  request's `seq`, which a shared multi-request slot can't carry).
- **A method with a custom error result** (`#[errors(..)]`, `CallError<E>`) can't go into a tuple `multi_call` on
  the client - call it on its own.
- **A trait level itself** (not a leaf resource) inside a multi request answers `OperationNotSupported`, as does a
  server without `multi_req = true` for any multi request at all.
