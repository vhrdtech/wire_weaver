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
    async fn led_on(&mut self) { /* do things */ }
    async fn set_brightness(&mut self, value: f32) {}
    async fn temperature(&mut self) -> f32 { 20.0 }
    async fn user_type(&mut self, state: State) {}
    async fn user_ret_type(&mut self) -> UserType<'_> {}
}

mod server_impl {
    wire_weaver::ww_api!(
        "../../api/src/lib.rs" as api::MyDevice for ServerState,
        server = true, no_alloc = true, use_async = true,
    );
}
```

`ww_api` proc-macro invocation will implement `async fn process_request_bytes(..) -> Result<..>` function, which takes
in request bytes,
deserializes and processes them and eventually calls one of the methods on self.

### sync

By setting `use_async = false` a blocking implementation is generated. And there is also a possibility to
return values later, via a provided request id (for example if executing a method and getting a result takes a long
time).

### deferred

### Resource names mapping

In order to avoid complex shared data structures and allocation on `no_std`, all API levels are squished into one.

## Client side