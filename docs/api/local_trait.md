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
a server, the client and one generic caller run against all of them).

!!! note "Prototype (CLT-57)"
    Methods, property reads and writes and plain streams of the API's root level are in the trait. Nested trait
    levels, resource arrays, observing, sinks and counted, reliable or batched streams are not yet: the trait's docs
    list what it leaves out, use the client for those.

## The trait, in the API crate

The trait belongs where both sides can name it: in the API crate. The crate asks for it with `crate ::` and no
`for <struct>`, behind a feature, since the trait uses `std` types (owned strings and vectors, boxed streams):

```rust
#[ww_api_root]
pub trait CounterApi {
    /// Adds `by` times `step` to the total, returns the new total and sends it on `totals`
    fn add(by: u32) -> u32;
    /// The total divided by `parts`, `Err(CounterError::DivByZero)` for 0 parts
    fn split(parts: u32) -> Result<u32, CounterError>;
    fn snapshot() -> Snapshot<'i>;
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
  its state behind a `Mutex` or atomics.

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
  `Denied`, `Unavailable`, `DeadlineExceeded`, `Cancelled`, `Overloaded` and `Internal`. `error.kind()` reads it;
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
The trace and the deadline don't travel on the wire yet, and the transports don't fill in the peer yet: until they
do, a server sees `Peer::Unknown` for a remote caller.

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

## Serving a local implementation

The server side doesn't implement the trait (its handlers take the request's `Context` and answer with `RpcResult`,
`GetResult`, `SetResult`), but its handlers can forward to a local implementation, so the logic is written once:

```rust
impl CounterServer {
    async fn add(&mut self, cx: &mut Context<'_, impl EventOut>, by: u32) -> RpcResult<u32> {
        let Ok(total) = self.core.add(cx.call(), by).await else { return Unimplemented.into() };
        let _ = api_impl::stream_data_ser().totals_send(&total, cx).await;
        Ready(total)
    }
}
```

`cx.call()` hands the request's `CallContext` to the local implementation, so it sees the remote caller as the
server did. This needs an `async` server with `no_alloc = false` (the trait's types are the owned ones) and `std`. Streams
don't forward by themselves: the server sends the stream's items from its handlers, the local implementation to its
local listeners.

## The trait without the API crate

`ww_codegen!(api_crate :: Api, local_trait = "Name")` also works in any crate that depends on the API crate, as the
`methods` test does. Only a trait made in the API crate can be implemented both by the client and by local types
in other crates, though, since a crate can't implement a trait it doesn't own for a type it doesn't own.
