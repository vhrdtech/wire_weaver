# Streams

Two types of streams are supported - from server to client (`stream!`) and from client to server (`sink!`).
I.e., naming is from the perspective of the device (node) - stream out, sink in.

```rust
#[ww_trait]
trait MyDevice {
    stream!(byte: u8);
    sink!(word: u32);
    stream!(slice: Vec<u8>);
    sink!(user_defined: Vec<LedState>);
}
```

Any type supported by the SerDes system works with streams as well. Streams can be used for many things, e.g., sending
status updates or bytes from USART, frames to be transmitted on CAN bus, etc.

Stream writes are not acknowledged - write message is sent out and no response is awaited by client, server publishes a
stream update and similarly do not wait for any response from client.
It is possible though to implement a token-based or some other form of backpressure using sideband channel.

Streams can have a beginning and an end, for example to implement a file IO or firmware update, to deal with small
chunks
at a time and yet be able to signal a completion event. It is also possible to send a user defined delimiter, to be
delivered in order with stream data, that can be used to implement frame synchronisation.

Another useful property of streams is that they work on object level. For the `slice` stream in the example above,
each individual array size is guaranteed to be preserved, even if multiple stream updates are transferred together at
transport level. Sending `[1, 2, 3]`, `[4]`, `[5, 6]` will result in the same arrays received on the other end, in the
same order.

You can subscribe to stream updates, in an asynchronous or blocking manner, see more on
the [detailed page](detailed.md).
