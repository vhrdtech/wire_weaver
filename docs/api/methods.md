# Methods

Methods can be defined as follows:

```rust
#[ww_trait]
trait MyDevice {
    fn led_on();
    fn set_brightness(value: f32);
    fn temperature() -> f32;
    fn user_type(state: State);
    fn user_ret_type() -> UserType<'i>;
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
pub enum State {
    Off,
    On,
}

#[derive_shrink_wrap]
#[owned = "std"]
pub struct UserType<'i> {
    a: u8,
    b: RefVec<'i, u8>,
    c: SomeOtherType<'i>
}
```

Any number of arguments are supported, and they can be of any type supported
by [ShrinkWrap](https://crates.io/crates/shrink_wrap).

## Server side

Server side is responsible for deserializing requests from clients and dispatching them to user-provided functions.
On a high level, this is the process that takes place:

1. Incoming byte array is deserialized into `ww_client_server::Request`.
2. Resource path contained in the `Request` is used to reach appropriate resource (or `BadPath` error is sent back).
3. Depending on the resource kind, appropriate actions are handled (Call, Read, Write, etc.).
4. User defined request types are deserialized (method arguments, sink data, property set values).
5. User provided action is called.
6. User defined types are serialized (method return types, stream data, property get values).
7. Response is serialized and sent back to client.

Generated code can be of two flavors - `async` and `sync`. The only difference is that each user action is `await`'ed in
the async version.

Additionally, there is `deferred` mode that can be turned on for user-selected methods. It works with both sync and
async versions and allows to immediately return from the user handler and send an answer later. For example
`move_motor(x: f32) -> Result<(), Error>` method can take a long time to finish, without deferred, it would block all
other API resources.

### async

On the server side, this is how generated server code is tied with user provided implementation:

```rust
use api::{State, UserType};

struct ServerState {
    // any user data required for the server to function (e.g, peripherals, channels)
}

impl ServerState {
    async fn led_on(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
        /* do things */
        Ready(())
    }
    async fn temperature(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<f32> {
        Ready(20.0)
    }
    async fn user_type(&mut self, _cx: &mut Context<'_, impl EventOut>, state: State) -> RpcResult<()> {
        Ready(())
    }
}

mod server_impl {
    wire_weaver::ww_codegen!(
        api :: MyDevice for super::ServerState,
        server = true, no_alloc = true, use_async = true,
    );
}
```

`ww_codegen!` implements `async fn process_request_bytes(bytes, scratch, out, medium) -> Result<..>` on
`ServerState`, which deserializes a request, calls one of the handlers and serializes the reply into `scratch`.
`ww api scaffold <api_crate>` prints all the handlers a server is expected to implement, with exact signatures.

### sync

By setting `use_async = false`, a blocking implementation is generated: handlers are plain `fn`s and take
`cx: &mut Context<'_, impl BlockingEventOut>`. A sync server also runs on the async `ww_device::Server`: events
sent by its handlers are queued and sent out after it returns (`EventWriter::queued()`), so they are limited by the
event scratch buffer size.

### Handler context

Every handler (methods, `get_`/`set_`/`changed_` of properties, `sideband_`/`write_` of streams) gets
`cx: &mut wire_weaver::Context` as the first argument:

* `cx.seq()` - sequence number of the request, 0 if the client does not expect a reply.
* `cx.medium()` - the medium the request came from. The type is set with `ww_codegen!(.., medium = "crate::Medium")`
  (`()` by default) and the value with `Server::with_medium(..)`. A device with USB and CAN runs one server per
  medium, all handling requests with the same server state.
* `cx.reply_to()` - medium and seq to answer a deferred call later, `None` if no reply is expected.
* `cx` sends events: stream updates and property updates with the generated
  `stream_data_ser().<name>_send(&value, cx).await` (`_send_blocking` for sync servers), or any event with
  `send_event()`. Events sent from a handler go out before its reply.

```rust
#[derive(Copy, Clone, PartialEq)]
enum Medium { Usb, Can }

impl ServerState {
    async fn start_adc(&mut self, cx: &mut Context<'_, impl EventOut, Medium>, rate: u32) -> RpcResult<()> {
        if cx.medium() == Medium::Can {
            self.adc_rate = rate.min(100);
        }
        // first sample right away, the rest from the event loop through server.sink()
        _ = server_impl::stream_data_ser().adc_send(&self.sample(), cx).await;
        Ready(())
    }
}
```

`cx` only reaches the medium the request came from. Events for other media are sent from the event loop through
their servers' `sink()`.

### deferred

`method_model = "move_motor=deferred, _=immediate"` lets a method return `Deferred` and answer later. The handler
keeps `cx.reply_to()`, and the reply is sent with the generated `ServerState::move_motor_send_return(out, seq, output)`
(`_send_return_blocking` for sync), where `out` is another handler's `cx` or `server.sink()` of `reply_to.medium`.
A deferred call that is never answered times out on the client.

### Resource names mapping

In order to avoid complex shared data structures and allocation on `no_std`, all API levels are squished into one.

## Client side