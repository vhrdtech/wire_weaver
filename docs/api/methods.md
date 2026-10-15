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

`client = "..."` in `ww_codegen!` generates a client struct with one function per method. It returns a
`PreparedCall`, which is executed with `call().await`, `blocking_call()`, `call_forget()` (no reply) or
`call_promise(marker)` (polled from immediate mode UI code).

A method without `Result` gives `Result<T, wire_weaver_client::Error>`. A method returning `Result<T, E>` gives
`Result<T, CallError<E>>`, so one `?` is enough:

```rust
pub enum CallError<E> {
    /// The device answered with its error: the `Err(e)` of the method.
    Device(E),
    /// The device has no implementation: its handler returned `Unimplemented`, or the device's API doesn't have
    /// the method (known locally from introspection, no round trip).
    Unimplemented { resource: String },
    /// The call didn't go through: link, timeout, disconnect, another framework error.
    Client(wire_weaver_client::Error),
}
```

```rust
// trait: fn move_motor(x: f32) -> Result<(), MotorError>;
client.move_motor(1.0).call().await?; // with anyhow, or a thiserror enum with #[from] CallError<MotorError>

match client.move_motor(1.0).call().await {
    Ok(()) => {}
    Err(CallError::Device(MotorError::EndStop)) => { /* the device said no */ }
    Err(CallError::Unimplemented { .. }) => { /* this device can't move motors */ }
    Err(e) => return Err(e.into()), // CallError::Client: link, timeout, ...
}
```

`Display` and `source()` of `CallError` are the inner error's, and it is `std::error::Error` when `E` is. Methods
without `Result` tell an unimplemented method apart with `wire_weaver_client::Error::is_unimplemented()`. A property
with a user error type (`property!(rw mode: u8, ModeError)`) gives the same `CallError<ModeError>` from `write()`.
See [Errors](errors.md) for declaring errors, common errors and errors of newer firmware.

### Several calls in one request

Several methods can be called in one request/response pair, each answered on its own:
`(client.inc(), client.dec()).multi_call().await`, or the same method across a whole array with `call_array(..)`.
This is shared machinery with properties and reads/writes - see [Multi requests](multi_requests.md) for the full
picture (the server's `multi_req = true` option, `MultiIndex`, the generated/array/dynamic-client APIs, errors and
limits).

### Error lists

`#[errors(..)]` on a method names the variants of its error enum it returns (API-32), `Common(InUse, Refused)` the
variants of the enum a `Common(CommonError)` variant holds, `Common` all of them:

```rust
#[ww_trait]
trait Sensor {
    /// Reads one sample.
    #[errors(Common(Busy, InUse), Nack)]
    fn read() -> Result<u16, SensorError<'i>>;
}
```

The list is checked when the trait is parsed (an unknown or repeated name is a compile error on that name), goes
into the API model and from there into the generated client's docs (`# Errors`, each entry with its variant's doc
line), `ww api tree`, Python's `help()` and `ww api check` (a changed list is a compatible change). The device and
the wire don't see it: a device may still return any variant, the client logs a `tracing` warning when one is
outside the list, and `tests_common` makes that a test failure (`wire_weaver_client::set_strict_error_lists`).
Without `#[errors]` a method may return every variant. Stream functions and properties take the same list. More in
[Errors](errors.md#which-errors-a-method-returns).
