---
level: advanced
---

# Local trait: one call site, local or remote

A `#[ww_trait]` API normally has two sides: a server whose handlers are inherent methods, and a generated client
whose calls are prepared objects (`client.add(3).call().await`). Code that calls the client can only ever talk to a
device or service through a transport.

Some APIs are worth having both ways: a library that runs in the same process when it can, and in another process or
on another machine when it must. `local_trait` gives such an API a real async Rust trait. The generated client
implements it, a plain struct implements it, and a call site written once against the trait works with either. The
local case is a direct function call: no server, no serialization, no channel.

The worked example is `examples/local_trait_api` (the API and its trait) with `examples/local_trait` (a local struct,
a server generated from the trait, the client and one generic caller run against all of them).

!!! note "What the trait covers (CLT-57, CLT-58)"
    Methods, property reads and writes and plain streams are in the trait, at every [nested level](#nested-levels),
    in [resource arrays](#resource-arrays) and in [keyed arrays](#keyed-arrays). Observing, sinks and counted,
    reliable or batched streams are not yet: the trait's docs list what it leaves out, use the client for those.
    Every level's trait is also implemented for `Authorized`, which asks an [authorization
    hook](#authorization-in-the-same-process) before each call (SRV-72).

## The trait, in the API crate

The trait belongs where both sides can name it: in the API crate. The crate asks for it with `crate ::` and no
`for <struct>`, behind a feature, since the trait uses `std` types (owned strings and vectors, boxed streams):

```rust
#[ww_api_root]
pub trait CounterApi {
    /// Adds `by` times `step` to the total, returns the new total and sends it on `totals`
    #[effect(edit)]
    fn add(by: u32) -> u32;
    /// The total divided by `parts`, `Err(CounterError::DivByZero)` for 0 parts
    #[effect(read)]
    fn split(parts: u32) -> Result<u32, CounterError>;
    #[effect(read)]
    fn snapshot() -> Snapshot<'i>;
    #[effect(edit)]
    property!(rw step: u32);
    stream!(totals: u32);
}

#[cfg(feature = "local")]
mod local {
    wire_weaver::ww_codegen!(crate :: CounterApi, local_trait = "Counter");
}
#[cfg(feature = "local")]
pub use local::{Counter, CounterDyn};
```

with `local = ["std"]` in the API crate's features, `std` turning on `wire_weaver/std`. The trait's errors and
streams are `wire_weaver::call`'s, so the API crate doesn't depend on `wire_weaver_client` and its transports. The
name is yours to pick: the `#[ww_trait]` name itself is already taken by the trait rustdoc shows for the API.

What comes out, for the API above:

```rust
use wire_weaver::CallContext;
use wire_weaver::call::{CallError, Error, ItemStream};

pub trait Counter: Send + Sync {
    fn add(&self, cx: &CallContext, by: u32) -> impl Future<Output = Result<u32, Error>> + Send;
    fn split(&self, cx: &CallContext, parts: u32)
        -> impl Future<Output = Result<u32, CallError<CounterError>>> + Send;
    fn snapshot(&self, cx: &CallContext) -> impl Future<Output = Result<SnapshotOwned, Error>> + Send;
    fn read_step(&self, cx: &CallContext) -> impl Future<Output = Result<u32, Error>> + Send;
    fn write_step(&self, cx: &CallContext, step: u32) -> impl Future<Output = Result<(), Error>> + Send;
    fn totals(&self, cx: &CallContext) -> impl Future<Output = Result<ItemStream<u32>, Error>> + Send;
}
```

- Each function has the name and the result of the client's own call: `add` for `client.add(by).call()`,
  `read_step` / `write_step` for the property, `totals` for opening the stream.
- The first argument is always the [call's context](#who-asked-callcontext): who asked, the deadline, the
  priority, the trace.
- Arguments and results are the owned types (`SnapshotOwned`, `String`, `Vec`), as on the client.
- Every result is a `Result`: the call can fail on its way (`Error`), and a method returning `Result<T, E>` gives
  `Result<T, CallError<E>>`, one `?` per call as on the [client](methods.md#client-side). See
  [Errors](#errors).
- A stream is an `ItemStream<T>`, a boxed `futures` stream of `Result<T, Error>` that ends when the stream is
  closed.
- Functions take `&self` and their futures are `Send`, so a generic caller can run on any task; a local type keeps
  its state behind a `Mutex` or atomics. A single-threaded program whose state is `Rc` / `RefCell` asks for the
  trait [without `Send`](#without-send).

## A local implementation

A plain struct implements the trait with `async fn`s:

```rust
impl Counter for LocalCounter {
    async fn add(&self, _cx: &CallContext, by: u32) -> Result<u32, Error> {
        let total = {
            let mut state = self.state.lock().unwrap();
            state.total += by * state.step;
            state.total
        };
        let _ = self.totals.send(total); // a tokio broadcast channel
        Ok(total)
    }

    async fn split(&self, _cx: &CallContext, parts: u32) -> Result<u32, CallError<CounterError>> {
        if parts == 0 {
            return Err(CallError::Api(CounterError::DivByZero));
        }
        Ok(self.state.lock().unwrap().total / parts)
    }

    async fn totals(&self, _cx: &CallContext) -> Result<ItemStream<u32>, Error> {
        // any futures stream of Result<u32, Error>, boxed
        Ok(item_stream(/* .. the broadcast receiver as a stream .. */))
    }
    // ..
}
```

The API's own error is returned as `CallError::Api(e)`, as a device would; `wire_weaver::call::item_stream` boxes
any stream into an `ItemStream`.

## Errors

`wire_weaver::call` (feature `std`) has the errors of the trait, the same for a local implementation and for the
client:

- `Error` is what went wrong on the way: an `ErrorKind` and an optional cause. The kinds are `Unimplemented`,
  `Denied`, `Unavailable`, `DeadlineExceeded`, `Cancelled`, `Overloaded`, `MessageTooBig` (a request or reply over
  the link's [message limit](large_messages.md#over-the-limit), with which one, its length and the limit) and
  `Internal`. `error.kind()` reads it;
  `Error::denied("why")`, `Error::from_kind(ErrorKind::Overloaded)` and `ErrorKind::Overloaded.into()` make one.
- `CallError<E>` is a method's error with the API's own error type: `Api(E)` is the API's `Err(e)`, the other
  variants carry the kinds above with their cause. `?` turns an `Error` into a `CallError<E>`;
  `call_error.into_api()` gives the API's error or the `Error` back.
- On the client, `wire_weaver_client::Error` and `wire_weaver_client::CallError<E>` convert into these, the client's
  error kept as the cause: a timeout is `DeadlineExceeded`, a lost link `Unavailable`, a missing item
  `Unimplemented` (`client_error.call_kind()` says which). The kind is what a caller matches on; the cause is for the
  log.

```rust
match counter.split(cx, 0).await {
    Ok(half) => println!("{half}"),
    Err(CallError::Api(CounterError::DivByZero)) => println!("nothing to split"),
    Err(e) if e.kind() == Some(ErrorKind::Unavailable) => println!("try again later: {e}"),
    Err(e) => return Err(e.into()),
}
```

## Who asked: `CallContext`

`wire_weaver::CallContext` is one small `Copy` value per call (`no_std`, no allocator) that says who asked and under
which terms:

| Field        | What it is                                                                                        |
|--------------|---------------------------------------------------------------------------------------------------|
| `seq`        | the request's sequence number, 0 for a call in the same process                                   |
| `priority`   | how soon the call should be served, `Priority::NORMAL` unless set                                 |
| `peer`       | who asked, as the transport saw it: `Local`, `Unix { uid, gid, pid }`, `Key(PeerKey)`, `Addr`, `Unknown` |
| `trace`      | the caller's W3C trace context, if it sent one                                                     |
| `deadline`   | what was left of the caller's time budget, `Option<Millis<u32>>`                                   |
| `connection` | the connection the call came on, to tell callers of one server apart                              |

A caller in the same process passes `CallContext::local()` (`Peer::Local`, no deadline, no trace), or a copy with
its terms changed:

```rust
let cx = CallContext::local()
    .with_priority(Priority::URGENT)
    .with_deadline(Millis::new(200));
counter.add(&cx, 3).await?;
```

A server handler has one in its `Context`, filled from the request and the transport: `cx.call()`. Forwarding it to
a local implementation keeps the caller's identity and terms (see [Serving a local
implementation](#serving-a-local-implementation)). The peer comes from the transport only, never from what the
caller says about itself, so a local implementation can decide by it:

```rust
async fn write_step(&self, cx: &CallContext, step: u32) -> Result<(), Error> {
    if let Peer::Addr(addr) = cx.peer {
        return Err(Error::denied(format!("{addr}: the step is not set over the network")));
    }
    self.state.lock().unwrap().step = step;
    Ok(())
}
```

The client's implementation of the trait sends the request with `cx`'s priority (when it isn't `NORMAL`) and uses
its deadline as the call's timeout. The same is there for any client call: `client.add(3).with_call_context(&cx)`.
The trace and the deadline travel in the request's context ([Deadline and
cancel](host_server.md#deadline-and-cancel)); a `wire_weaver_server` fills in the peer from its transport.

## Authorization in the same process

A call through the trait reaches the implementation directly, so no server asks its [authorization
hook](host_server.md#authorization). `wire_weaver::authorize::Authorized` puts the same hook in front of a local
implementation: `ww_codegen!(.., local_trait = "..")` implements every level's trait for it, and each function asks
the hook before it forwards.

```rust
use std::sync::Arc;
use wire_weaver::authorize::{Access, Authorized, Denied, Resource, from_fn};

let hook = Arc::new(from_fn(|call: &CallContext, resource: &Resource<'_>| {
    if resource.access == Access::Read || resource.effects().is_read() {
        Ok(())
    } else {
        Err(Denied)
    }
}));
let counter = Authorized::new(LocalCounter::new("local"), hook.clone());
counter.add(&cx, 3).await?; // asks the hook: Call of `add`, effects `edit`: Denied
// the same hook for remote callers
Server::new(clock).authorize(hook, Some(api)).serve(link, listener, make, shutdown).await?;
```

What the hook sees for a call through `Authorized`:

| `Resource` field | For a local call |
|---|---|
| `access` | `Call` for a method and a keyed array's `index_of`, `Read` for a property read, valid indices and a key, `Write` for a property write, `Stream` for a stream and `keys` |
| `place`, `path` | always absolute: item ids from the root, each array index after its array's id |
| `items` | one `Step` per level on the way and the item, with the effects and `ww_self` metadata of the API's model |
| `multi` | `false` |
| `api` | `None`: the steps come from constants the code generation wrote, no model is decoded |

The `CallContext` is the caller's: `CallContext::local()` gives `Peer::Local`, a server forwarding to the wrapper
passes `cx.call()` with the remote peer. A server asks the hook for the same call with the same access, path and
steps (`tests/serve_trait`, `authorized_calls_ask_what_a_server_asks`), so one policy covers both.

- **Levels.** An accessor asks nothing; it hands out the level wrapped with the same hook (`Authorized<T::Desk<'_>,
  &A>`), so every function of a nested level and of an array element is checked, with the element's index in the
  path. A level of another crate gets its implementation from that crate's `ww_codegen!`.
- **Denied.** A denied call returns `ErrorKind::Denied` (`CallError::Denied` for a method with its own error) and the
  implementation never runs.
- **Pending.** A [pending verdict](host_server.md#waiting-for-an-answer) is awaited inside the call's future, before
  the implementation runs. Dropping the call withdraws the question; nothing else bounds the wait, a caller that
  wants a limit puts a timeout around the call.
- **Hooks.** `A` is any `Authorize`: a `from_fn` closure, a type of yours, or a `&`, `Box` or `Arc` of one. The
  wrapper of the `Send` trait needs `A: Send + Sync`; of the [trait without `Send`](#without-send), none, or `Sync`
  when a level of another crate keeps its `Send` trait. `inner()` reaches the implementation unchecked.

Why one hook and a future for approvals: an agent's `outbound` or `destructive` call may need a person's yes (an
approval queue). The hook answers `Verdict::Pending` with a future of that answer, and both paths await it inside the
call they guard: the server within the request's deadline and cancel, `Authorized` within the call's future. A
"pending" answer that the caller retries would need a ticket in every client and in this trait, a second mechanism for
the same decision. See [Waiting for an answer](host_server.md#waiting-for-an-answer).

## The client implements it too

The client's `ww_codegen!` names the trait with `impl_local_trait`:

```rust
wire_weaver::ww_codegen!(
    local_trait_api :: CounterApi for super::CounterClient,
    client = "std_client",
    impl_local_trait = "local_trait_api::Counter",
);
```

Each trait function calls the client's generated one and awaits it; a stream is turned into an `ItemStream` with
`Stream::into_item_stream`.

## One call site

```rust
pub async fn exercise(
    counter: &impl Counter,
    cx: &CallContext,
) -> Result<Report, Box<dyn std::error::Error>> {
    let mut totals = counter.totals(cx).await?;
    counter.write_step(cx, 2).await?;
    counter.add(cx, 3).await?;
    let half = counter.split(cx, 2).await?;
    let first = totals.next().await;
    // ..
}

let cx = CallContext::local();
exercise(&LocalCounter::new("local"), &cx).await?;   // direct calls
exercise(&client, &cx).await?;                       // over any transport
```

`examples/local_trait` runs the same `exercise` against a `LocalCounter` and against the client connected to a
server over the `in_process` transport, and checks both give the same report.

## Picking at run time: trait objects

`async fn`s in a trait don't make a trait object, so the trait comes with a companion, `CounterDyn`: the same
functions returning boxed futures. Every implementor of `Counter` implements `CounterDyn`, and `Box<dyn CounterDyn>`
and `Arc<dyn CounterDyn>` implement `Counter`, so a generic call site takes them too:

```rust
let counter: Arc<dyn CounterDyn> = if remote {
    Arc::new(CounterClient::config(..).connect().await?)
} else {
    Arc::new(LocalCounter::new("local"))
};
exercise(&counter, &CallContext::local()).await?;
```

The companion's functions take `self` and `cx` for the same lifetime: `fn add<'a>(&'a self, cx: &'a CallContext,
by: u32) -> BoxFuture<'a, Result<u32, Error>>`.

Bring only one of the two traits into scope where you call: with both, `counter.add(..)` is ambiguous.

## Nested levels

An API with a nested level, `ww_impl!(desk: DeskApi)`, gets one trait per level. The root's trait hands out the
nested level through an accessor, and the nested level's trait has the level's own functions. Each level names its
trait on its definition, since `ww_codegen!` names only the root's:

```rust
#[ww_api_root]
pub trait PlantApi {
    ww_impl!(desk: DeskApi);
}

#[ww_trait(local = "Desk")]
pub trait DeskApi {
    fn ping(n: u32) -> u32;
    property!(rw lamp: u32);
}
```

`ww_codegen!(crate :: PlantApi, local_trait = "Plant")` then generates `Plant` and `Desk`, each with its `Dyn`
companion. The accessor is not async: reaching a level never asks the device, only its functions do. Its type is an
associated type of the parent's trait, so a local parent hands out a reference to a field:

```rust
pub trait Plant: Send + Sync {
    type Desk<'a>: Desk + 'a where Self: 'a;
    fn desk(&self) -> Self::Desk<'_>;
}

impl Plant for LocalPlant {
    type Desk<'a> = &'a LocalDesk;

    fn desk(&self) -> &LocalDesk {
        &self.desk
    }
}
```

`&T` of any implementor implements the trait too, so `&LocalDesk` is a `Desk`. The client's accessor gives the
client's level struct, which implements `Desk` with `impl_local_trait` as the root's client implements `Plant`.

A level defined in another crate keeps that crate's trait. The other crate generates it with its own `ww_codegen!`
and re-exports it at its root, next to the `#[ww_trait]` it belongs to: `pub use local::{Gauge, GaugeDyn};`. The
root's trait finds it there, in the same `Send` form as its own.

A level whose own API is known only at run time — another process's, present or absent — is not `ww_impl!`'s
business: it is a [mount slot](mounts.md), served by `wire_weaver_server::mount` rather than `serve_trait`.

## Resource arrays

An array of levels, `ww_impl!(unit[]: UnitApi)`, gets an accessor that takes the element's index and can fail:

```rust
type Unit<'a>: Unit + 'a where Self: 'a;
fn unit(&self, index: u32) -> Result<Self::Unit<'_>, Error>;
fn unit_valid_indices(&self, cx: &CallContext) -> impl Future<Output = Result<ValidIndicesOwned, Error>> + Send;
```

An array of properties or streams has no accessor. Its functions take the element's index as their first argument
after `cx`: `read_slot(cx, index)`, `write_slot(cx, index, value)`, `feed(cx, index)`.

Every array also has `<array>_valid_indices`, the indices that exist right now. A local implementation answers them
from its own state, and the client reads them from the device. An index that names no element is refused with
`ErrorKind::BadIndex`, the same error a device answers for it:

```rust
fn unit(&self, index: u32) -> Result<&LocalUnit, Error> {
    self.units
        .get(index as usize)
        .ok_or_else(|| ErrorKind::BadIndex.into())
}
```

The index is a `u32` for an array declared `name[]`. An array with its own index type, `channel[ChannelIdx]`, takes
that type instead. The type converts into `u32` for the client (`From<ChannelIdx> for u32`), and a served trait gets
it back from the request through `TryFrom<u32>`; an index that doesn't convert is answered `BadIndex`.

`examples/local_trait` has an array of levels, its lanes, and `tests/serve_trait` (`levels`) runs every kind of
nested level and array against a local implementation, through `serve_trait` and through the client.

## Keyed arrays

A [keyed array](arrays.md#keyed-arrays) addresses its elements by a key: `keyed!(unit by Tag)` follows `unit[]`. Its trait gets
three functions, and an array of levels a fourth one that the trait provides:

| Function | What it gives |
|----------|---------------|
| `unit_index_of(cx, key)` | the index of the element with `key`, `None` when there is none |
| `unit_key(cx, index)` | the key of the element at `index` |
| `unit_keys(cx)` | an `ItemStream<KeyEvent<u32, Tag>>`: elements as they appear and go away |
| `unit_by(cx, key)` | the element itself: `unit_index_of`, then the accessor `unit` |

The index is the array's, as in [Resource arrays](#resource-arrays): `u32`, or the array's own index type. A local
implementation answers `unit_index_of` and `unit_key` from its table and builds `unit_keys` from its own events; it
never implements `unit_by`. A key that borrows (`keyed!(unit by Name<'i>)`) is taken and given as its owned form, `NameOwned`. The client asks the device once per key and keeps the answer in the connection's key
cache, which follows the `keys` stream.

## Serving a local implementation

One implementation of the trait can also be served to remote clients. `serve_trait` generates the server's handlers,
each calling the trait on one shared service, so nothing is written twice:

```rust
use std::sync::Arc;
use wire_weaver_server::{Backend, Connection, Pumps, Serves};

/// One connection's server: the shared counter and the connection's pumps.
pub struct CounterServer {
    service: Arc<LocalCounter>,
    pumps: Pumps,
}

impl CounterServer {
    pub fn new(service: Arc<LocalCounter>, connection: &Connection) -> Self {
        CounterServer { service, pumps: connection.pumps.clone() }
    }
}

impl Serves for CounterServer {
    type Service = LocalCounter;
    type Pumps = Pumps;

    fn service(&self) -> &LocalCounter {
        &self.service
    }

    fn pumps(&self) -> &Pumps {
        &self.pumps
    }
}

impl Backend for CounterServer {}

mod api_impl {
    wire_weaver::ww_codegen!(
        local_trait_api :: CounterApi for super::CounterServer,
        server = true,
        serve_trait = "local_trait_api::Counter",
    );
}
```

It is served on [`wire_weaver_server`](host_server.md), every connection with a server of its own around the same
`Arc`: `Server::new(clock).serve(link, listener, |c: &Connection| CounterServer::new(counter.clone(), c), shutdown)`.

What the generated handlers do:

| Item | Handler |
|------|---------|
| method | `&self`, calls the trait's function with `cx.call()` and the decoded arguments |
| property read | `get_x(&self, ..)`, calls `read_x` |
| property write | `set_x(&mut self, ..)`, calls `write_x` |
| stream | the open calls the trait's function and adds the `ItemStream` as the connection's pump, the close removes it |
| nested level | each of its handlers reaches the level through the accessors from the service, then calls the level's function |
| array element | the element's index from the request, the array's index type through `TryFrom<u32>` |
| valid indices | `<array>_valid_indices`, asked before every request under the array |
| keyed array | `index_of` and the key's read call `<array>_index_of` and `<array>_key`; the `keys` stream is a pump of `<array>_keys` |

- **Order.** Calls and reads take `&self`, so the requests of one connection run side by side, started in the order
  they came; writes and stream opens run alone, after the requests before them finished ([Order of
  requests](host_server.md#order-of-requests)). A write followed by a read is never reordered.
- **Who asked.** `cx.call()` is the request's `CallContext`: the peer the transport saw, the connection, the
  caller's trace and what is left of its deadline. The implementation decides by it as it does in the same process.
- **Deadline and cancel.** The server drops a call's future at the request's deadline or on the client's cancel, so
  the implementation stops at the `.await` where it waits.
- **Errors.** `Ok` is the answer, `CallError::Api(e)` the API's error. Every other error is answered with its kind:
  `Denied`, `DeadlineExceeded`, `Cancelled`, `Overloaded`, `Unimplemented`, or the common error the implementation
  returned; `Unavailable`, `MessageTooBig` and `Internal` are `Internal`. A client of an older `ww_client_server`
  sees the kinds it does not know as an unknown error.
- **Indices.** An index outside `<array>_valid_indices`, or one the accessor refuses, is answered `BadIndex`. The
  pump of an array's stream is named by the stream and the element's indices, so two elements' streams run side
  by side.
- **Streams.** Every connection gets its own `ItemStream`; sharing items between clients is the implementation's
  business, a `broadcast` channel for example. The pump ends at the stream's end or its first error, and closes the
  client's stream, so the client's `ItemStream` ends there too.

`serve_trait` sets the handlers' shape itself: `use_async = true`, owned arguments (`no_alloc = false`), calls and
reads shared (`receiver_model = "_=shared"`), immediate methods and getters and setters; leave out those arguments.
Items the trait leaves out (see the note above) are handlers written by hand beside the generated ones, in another
`impl CounterServer` block, with the signatures `ww api scaffold --alloc --use-async --receiver-model "_=shared"`
prints. Firmware keeps its hand-written handlers: the trait's owned types and boxed futures don't fit it.

## Without `Send`

A program on one thread, such as a GUI or a calloop loop, keeps its state in `Rc` and `RefCell`, and its futures and
streams aren't `Send`. `local_trait_send = false` gives it the trait in a form without `Send`:

```rust
#[cfg(feature = "local")]
mod local {
    wire_weaver::ww_codegen!(crate :: CounterApi, local_trait = "Counter", local_trait_send = false);
}
```

What changes against the `Send` form:

| | `Send` form | `local_trait_send = false` |
|---|---|---|
| the trait | `Counter: Send + Sync`, futures `+ Send` | no `Send` anywhere |
| a stream | `ItemStream<T>`, `item_stream(..)` | `LocalItemStream<T>`, `local_item_stream(..)` |
| the companion's futures | `BoxFuture<'a, T>` | `LocalBoxFuture<'a, T>` |
| shared trait object | `Arc<dyn CounterDyn>` | `Rc<dyn CounterDyn>` |

A local implementation then holds an `Rc` across an `.await` and builds its streams from `Rc` state:

```rust
impl Counter for LocalCounter {
    async fn totals(&self, _cx: &CallContext) -> Result<LocalItemStream<u32>, Error> {
        let state = self.state.clone(); // Rc<RefCell<State>>
        Ok(local_item_stream(stream::iter(0..3).map(move |_| Ok(state.borrow().total))))
    }
    // ..
}
```

The client implements this form too, with the same flag beside `impl_local_trait`; it is `Send` itself, its item
streams coerce into `LocalItemStream`:

```rust
wire_weaver::ww_codegen!(
    local_trait_api :: CounterApi for super::CounterClient,
    client = "std_client",
    impl_local_trait = "local_trait_api::Counter",
    local_trait_send = false,
);
```

`serve_trait` serves both forms with the same generated code. A service in an `Rc` gives its server
[`LocalPumps`](host_server.md#streams-that-are-not-send) instead of `Pumps`, and the factory is wrapped in `Local`:

```rust
use wire_weaver_server::{Backend, Local, LocalConnection, LocalPumps, Serves};

pub struct CounterServer {
    service: Rc<LocalCounter>,
    pumps: LocalPumps,
}

impl Serves for CounterServer {
    type Service = LocalCounter;
    type Pumps = LocalPumps;
    // fn service, fn pumps as above
}

let make = Local(move |c: &LocalConnection| CounterServer::new(counter.clone(), c));
server.serve(link, listener, make, shutdown).await?;
```

The connections are then not `Send`: `serve` runs on a single-threaded executor (a tokio `LocalSet`, calloop), or
the connections `accepting` hands out are spawned as local tasks. `tests/serve_trait` (`not_send`) serves such a
service on one thread and runs the same calls against it directly and through the client.

## The trait without the API crate

`ww_codegen!(api_crate :: Api, local_trait = "Name")` also works in any crate that depends on the API crate, as the
`methods` test does. Only a trait made in the API crate can be implemented both by the client and by local types
in other crates, though, since a crate can't implement a trait it doesn't own for a type it doesn't own.
