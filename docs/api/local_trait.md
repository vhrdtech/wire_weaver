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
`for <struct>`, behind a feature, since the trait uses `std` types and the client's error types:

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

with `local = ["std", "dep:wire_weaver_client"]` in the API crate's features. The name is yours to pick: the
`#[ww_trait]` name itself is already taken by the trait rustdoc shows for the API.

What comes out, for the API above:

```rust
pub trait Counter: Send + Sync {
    fn add(&self, by: u32) -> impl Future<Output = Result<u32, wire_weaver_client::Error>> + Send;
    fn split(&self, parts: u32)
        -> impl Future<Output = Result<u32, wire_weaver_client::CallError<CounterError>>> + Send;
    fn snapshot(&self) -> impl Future<Output = Result<SnapshotOwned, wire_weaver_client::Error>> + Send;
    fn read_step(&self) -> impl Future<Output = Result<u32, wire_weaver_client::Error>> + Send;
    fn write_step(&self, step: u32) -> impl Future<Output = Result<(), wire_weaver_client::Error>> + Send;
    fn totals(&self)
        -> impl Future<Output = Result<wire_weaver_client::local::ItemStream<u32>, wire_weaver_client::Error>> + Send;
}
```

- Each function has the name and the result of the client's own call: `add` for `client.add(by).call()`,
  `read_step` / `write_step` for the property, `totals` for opening the stream.
- Arguments and results are the owned types (`SnapshotOwned`, `String`, `Vec`), as on the client.
- Every result is a `Result`: the remote call can fail on its way (`Error`), and a method returning `Result<T, E>`
  gives `Result<T, CallError<E>>`, the same [one `?` per call](methods.md#client-side) as the client.
- A stream is an `ItemStream<T>`, a boxed `futures` stream of `Result<T, StreamError>` that ends when the stream is
  closed.
- Functions take `&self` and their futures are `Send`, so a generic caller can run on any task; a local type keeps
  its state behind a `Mutex` or atomics.

## A local implementation

A plain struct implements the trait with `async fn`s:

```rust
impl Counter for LocalCounter {
    async fn add(&self, by: u32) -> Result<u32, Error> {
        let total = {
            let mut state = self.state.lock().unwrap();
            state.total += by * state.step;
            state.total
        };
        let _ = self.totals.send(total); // a tokio broadcast channel
        Ok(total)
    }

    async fn split(&self, parts: u32) -> Result<u32, CallError<CounterError>> {
        if parts == 0 {
            return Err(CallError::Device(CounterError::DivByZero));
        }
        Ok(self.state.lock().unwrap().total / parts)
    }

    async fn totals(&self) -> Result<ItemStream<u32>, Error> {
        // any futures stream of Result<u32, StreamError>, boxed
        Ok(item_stream(/* .. the broadcast receiver as a stream .. */))
    }
    // ..
}
```

The API's own error is returned as `CallError::Device(e)`, as a device would; `wire_weaver_client::local::item_stream`
boxes any stream into an `ItemStream`.

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
pub async fn exercise(counter: &impl Counter) -> Result<Report, Box<dyn std::error::Error>> {
    let mut totals = counter.totals().await?;
    counter.write_step(2).await?;
    counter.add(3).await?;
    let half = counter.split(2).await?;
    let first = totals.next().await;
    // ..
}

exercise(&LocalCounter::new("local")).await?;   // direct calls
exercise(&client).await?;                       // over any transport
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
exercise(&counter).await?;
```

Bring only one of the two traits into scope where you call: with both, `counter.add(..)` is ambiguous.

## Serving a local implementation

The server side doesn't implement the trait (its handlers take the request's `Context` and answer with `RpcResult`,
`GetResult`, `SetResult`), but its handlers can forward to a local implementation, so the logic is written once:

```rust
impl CounterServer {
    async fn add(&mut self, cx: &mut Context<'_, impl EventOut>, by: u32) -> RpcResult<u32> {
        let Ok(total) = self.core.add(by).await else { return Unimplemented.into() };
        let _ = api_impl::stream_data_ser().totals_send(&total, cx).await;
        Ready(total)
    }
}
```

This needs an `async` server with `no_alloc = false` (the trait's types are the owned ones) and `std`. Streams
don't forward by themselves: the server sends the stream's items from its handlers, the local implementation to its
local listeners.

## The trait without the API crate

`ww_codegen!(api_crate :: Api, local_trait = "Name")` also works in any crate that depends on the API crate, as the
`methods` test does. Only a trait made in the API crate can be implemented both by the client and by local types
in other crates, though, since a crate can't implement a trait it doesn't own for a type it doesn't own.
