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

Stream writes are not acknowledged - the write message is sent out, and no response is awaited by a client, server
publishes a
stream update and similarly does not wait for any response from a client.
It is possible, though, to implement a token-based or some other form of backpressure using the sideband channel.

Streams can have a beginning and an end, for example, to implement a file IO or firmware update, to deal with small
chunks
at a time and yet be able to signal a completion event. It is also possible to send a user-defined delimiter, to be
delivered in order with stream data, that can be used to implement frame synchronization.

Another useful property of streams is that they work on the object level. For the `slice` stream in the example above,
each individual array size is guaranteed to be preserved, even if multiple stream updates are transferred together at
transport level. Sending `[1, 2, 3]`, `[4]`, `[5, 6]` will result in the same arrays received on the other end, in the
same order.

## Sideband channel

In order to facilitate stream open/close operations, frame synchronization and other operations, all streams
in addition to a data channel have:

* A sideband command channel
* A sideband event channel

All sideband data is sent in-order with the payloads (no reordering).

### Sideband commands

The following stream sideband commands are supported:

* Open
* Close
* FrameSync
* ChangeRate(ShaperConfig)
* SizeHint(u32)
* User(u32)

All commands are optional, including open and close.
Depending on the application, it might be beneficial to start a stream automatically without waiting for the command.

See [StreamSidebandCommand](https://github.com/vhrdtech/ww_stdlib/blob/main/ww_client_server/src/lib.rs#:~:text=StreamSidebandCommand)

### Sideband events

The following stream sideband events are supported:

* Opened
* Closed
* FrameSync
* SizeHint(u32)
* User(u32)
