---
level: advanced
---

# Arrays

A property, stream, sink or trait can be an array of resources, written with `[]` after its name:

```rust
#[ww_trait]
trait Board {
    stream!(adc[]: u16);
    #[effect(outbound)]
    sink!(lanes[]: [u8]);
    #[effect(edit)]
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
`index_chain: [ww_client_server::ResourceIndex; N]`, after `cx`; `ww api scaffold` prints the exact signatures. The generated code stays
flat: the whole API tree becomes plain function calls on the server struct, with no per-index state.

A whole array can be read, written or called in one request, see [Multi requests](multi_requests.md).

## Array of resources vs resource of array

Here, `led` is itself an array: each index is a resource of its own, with its own path, read and written
separately.

```rust
#[ww_trait]
trait ArrayOfResources {
    #[effect(actuate)]
    property!(rw led[]: bool);
}
```

Here `led` is one resource whose type is an array: all three values are read and written in one go.

```rust
#[ww_trait]
trait ResourceOfArrays {
    #[effect(actuate)]
    property!(rw led: [bool; 3]);
}
```

Both together:

```rust
#[ww_trait]
trait ArrayOfArrays {
    #[effect(actuate)]
    property!(rw rgb_led[]: [u8; 3]);
}
```

## Keyed arrays

An index says where an element sits on one device, in one run: a packet dissector gives flow 3 to whatever traffic
it met third. `keyed!` lets a client address the elements by a key of their own instead:

```rust
#[ww_api_root]
trait Dissector {
    ww_impl!(flow[]: Flow);
    keyed!(flow by FlowKey);
}
```

`keyed!` follows the array it keys (traits, properties, streams or sinks) and takes an item id at its position, like
any other item. The key is any wire_weaver type without a float in it. The API crate and the server crate depend on
`ww_keyed`, whose `KeyEvent` the generated code names.

The companion is a fixed set of operations, the same for every keyed array, at the path of `flow_keys`:

| Id | Item | What it is for |
|---|---|---|
| 0 | `fn index_of(key: FlowKey) -> Option<I>` | the index of the element with that key |
| 1 | `property!(ro key[I]: FlowKey)` | the key of an element; all of them in one multi read |
| 2 | `stream!(keys: KeyEvent<I, FlowKey>)` | `Added { index, key }` when an element appears, `Removed { index }` |

`I` is the array's own index type, `UNib32` for `flow[]`.

On the device the companion takes two handlers, `flow_index_of(cx, key) -> RpcResult<Option<I>>` and
`get_flow_key(cx, index_chain) -> GetResult<FlowKey, ()>` (always a getter, whatever the `property_model`); the
server keeps its key table, as it keeps the flows. The key array's indices are the array's own, checked by
`valid_indices_root_flow`, so `get_flow_key` is only called for an index the device has. The `keys` stream needs no
handler: the generated code opens and closes it, and the device announces each element with
`stream_data_ser().flow_keys_send(&KeyEvent::Added { index, key }, cx)` (`flow_keys_send_blocking` in a blocking
server). An index may go to another key only after its `Removed`. `ww api scaffold`
prints the handlers.

One `keyed!` costs 316 B of flash on a Cortex-M0 at `fw-z` in a blocking server, 276 to 784 B on M0 and M3 at
`fw-z` and `fw-3`, blocking and async (TEST-30, `qemu-flash-keyed` against `qemu-flash-arrayed`). The `keys`
stream's send helper is not counted: it is linked only where it is used.

### Keys that borrow

A key may borrow, a name for instance (API-72):

```rust
#[ww_api_root]
trait Dissector {
    property!(ro detector[]: Bytes<u64>);
    keyed!(detector by DetectorName<'i>);
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct DetectorName<'i>(pub &'i str);
```

A server without an allocator takes and gives the key as written, `detector_index_of(cx, key: DetectorName<'_>)`
and `get_detector_key(..) -> GetResult<DetectorName<'_>, CommonError<'_>>`, so it can answer from a `&'static str`
table. Everything that keeps a key takes its owned form, `DetectorNameOwned`: a server with an allocator, the
client and its key cache, the [local trait](local_trait.md#keyed-arrays) and `serve_trait`. The key type therefore
needs `owned` in its `derive_shrink_wrap`; without it `keyed!` is refused with that hint.

Both forms write the same bytes: `"east"` is `65 61 73 74 04`, the string with its length at the back. These bytes
are what a tape's `Segment::Key` hashes for the element, so a track of that detector keeps its key whatever index
the device gave it. `tests/keyed` pins them.

### Keyed arrays on the client

The generated client finds an element by its key (CLT-72):

```rust
if let Some(flow) = client.flow_by(&key).await? {
    let bytes = flow.read_bytes().read().await?;
}
let key = client.flow_key(3).await?;    // the key of flow 3
let mut keys = client.flow_keys().await?; // KeyEvent::Added and Removed as they happen
```

`flow_by` returns the element's client, `None` when the device has no element with that key; it returns the index
for an array of properties, streams or sinks. `flow_index_of(&key)` returns the index for any array. Each has a
`_blocking` twin. The key type needs `Clone`. The API's [local trait](local_trait.md#keyed-arrays) has the same
lookups, for a local implementation and the client alike.

`flow_by` asks the device's `index_of` once per key and keeps the answer in the connection's key cache, shared by
every clone of the client: later lookups send nothing. Keys the client sees otherwise go into the cache too: a
`flow_key` read, an `Added` on the `keys` stream. An entry goes away on a `Removed`, on a `BadIndex` answer to a
request through that index, and on a reconnect; the cache holds at most 1024 keys per keyed array and forgets the
oldest first. A key not found is asked again each time. Between a flow's removal and the next request through its
index, a cached index can already belong to another flow: a client that must not touch the wrong element keeps the
`keys` stream open, so the `Removed` drops it at once.

The dynamic client, `ww` and Python take a key in a path. In `ww` and the dynamic client
(`DynResource::walk_on`) the brackets hold the key as a Rust expression of its type, all fields given:

```sh
ww stream 'flow[FlowKey { proto: Proto::Tcp, src: Endpoint { addr: [10, 0, 0, 1], port: Port(6000) },
    dst: Endpoint { addr: [10, 0, 0, 2], port: Port(443) }, first_seen: Point { coord: 100 } }].packets'
```

A time point in a key is written as its `coord` (`Point { coord: 100 }`) for now. A number in brackets is an index.

In Python an int indexes and anything else is a key, in the form any argument takes:

```python
key = {"proto": "Tcp", "src": {"addr": [10, 0, 0, 1], "port": 6000},
       "dst": {"addr": [10, 0, 0, 2], "port": 443}, "first_seen": {"coord": 100}}
dev.flow[key].bytes.read()  # KeyError when the device has no such flow
dev.flow.by(key)            # None instead
dev.flow.key_type           # "FlowKey"
```
