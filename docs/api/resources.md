# API

Define a custom protocol as collections of resources - methods, properties or streams and generate server and client
side code.
Multiple levels are supported, each resource is identified via a number path from root, forming a tree. Efficient path
is used, consisting of UNib32 encoded numbers, taking as little as 4 bits.

Resources can be arranged into "ww-trait's" and then "implemented" at various points in the API tree. Accessing them is
possible in the same manner via resource paths, or through their globally unique ID (crate name + version or unique ID +
version). Many useful "ww-traits" are planned, implementing things like firmware update, event counters, logging, power
management, etc. That way code to handle them all can be reused greatly between very different projects. UI can also be
arranged into small reusable blocks.

Two models are planned: client-server and bus. Client-server model is functional (`ww_client_server` crate) and
supported in code generation for both server and client side, std and no_std. Bus model is still in development.

## Methods

### Async and sync

### Deferred and Immediate

## Streams

```rust
trait Log {
    fn defmt_bytes() -> Stream<u8>;
    fn sink(stream_in: Sink<u8>);
}
```

## Properties

### Get/Set and value on change

## Traits

#### User handler flattening (no_std)

## Resource arrays

Any resource can also be an array - method, property, stream and trait implementation:

```rust
trait ArrayOf {
    fn run<N: u32>();
    stream!(adc[]: u16);
    property!(led[]: bool);
    ww_impl!(motor[]: ww_motor_control::Motor);
}
```

TODO: size bounds

Traits inside other traits can also contain arrays, all the indices leading up to them are accumulated and passed as
Rust array `[u32; N]` argument into a corresponding user handler.

### Array of resources vs resource of array

Here, resource led is itself an array, when accessing it - an index will be added to the resource path.
Each one of three bool's is accessed separately from each other.

```rust
trait ArrayOfResources {
    property!(led[3]: bool);
}
```

On the other hand, here led is not an array, but it's type is. All three boolean's are accessed in one go.

```rust
trait ResourceOfArrays {
    property!(led: [bool; 3]);
}
```

Both can be used together as well, for example:

```rust
trait ArrayOfArrays {
    property!(rgb_led[3]: [u8; 3]);
}
```

## Transport protocols

Several transport protocols are supported:

* USB (nusb on host side, embassy on embedded, no drivers needed on Windows/Mac/Linux)
* WebSocket (for reliable control access)
* UDP (for telemetry)
* TODO: CAN Bus (using CANOpen)

Others could be easily implemented, possibly reusing the same code.

USB and UDP transports support multiple events per packet/datagram. Many small messages can be accumulated over a time
window conserving bandwidth and allowing much higher message throughput per unit of time that would otherwise be
possible with one message per packet/datagram.

