---
level: advanced
---

# Host server: one API, many clients

A device serves one host at a time, so its server is one link and one backend. A service on a computer is
different: a daemon, a simulator or a tool with an API of its own has many clients at once, and each of them
connects, works and leaves on its own schedule. `wire_weaver_server` is the runtime for that case. It takes
connections from a listener and serves each of them with the same generated server a device uses.

The crate is the counterpart of `wire_weaver_client`. Nothing changes in the API crate or in the generated code: a
host server is built from `ww_codegen!(.., server = true, ..)` exactly as a firmware is.

## What a connection is

A **connection** is one client's link to the server, from its link setup until the link goes down. Each connection
gets its own **backend**: the struct that `ww_codegen!` implements the dispatcher on, with the handlers you write.
State that belongs to one client lives in that struct, and state that all clients share lives behind an `Arc` that
every backend holds.

```rust
struct Backend {
    shared: Arc<Shared>, // the service itself
    id: ConnectionId,    // this client
}

impl Backend {
    async fn echo(&mut self, _cx: &mut Context<'_, impl EventOut>, x: u32) -> RpcResult<u32> {
        Ready(x)
    }
}
```

The server makes a backend with a factory that you pass in. The factory runs once the link of a connection is up,
and receives a `Connection` that describes it:

| Field | What it holds |
|---|---|
| `id` | A number that tells the connections of one server apart. |
| `peer` | Who connected, as the transport saw it. |
| `client` | What the client says about itself: program, user, host and version. Use it in logs, never to decide. |
| `client_version` | The API version the client was built against, to see version skew. |
| `message_len_max` | The longest message either side takes, agreed in link setup. |

## Serving

`Server::serve` is a future. It accepts connections, runs them and ends after a shutdown. You run it on the executor
your program already has:

```rust
use wire_weaver_server::{Connection, Server};

let link = ww_device::LinkConfig::new(
    my_api::MY_API_FULL_GID,
    my_api::MY_API_WIRE_VERSION,
    api_impl::api_hash(),
    api_impl::image_facts(),
    ww_client_server::COMPACT_VERSION,
);
let shared = Arc::new(Shared::default());
Server::new(clock)
    .serve(
        link,
        listener,
        move |connection: &Connection| Backend { shared: shared.clone(), id: connection.id },
        shutdown,
    )
    .await?;
```

The four arguments are the description of the API for link setup, the listener, the factory and a future that
completes when the server should stop.

The crate contains no async runtime. Time comes from a `ww_device::Clock`, the same trait a firmware implements, and
the shutdown signal is any future. All connections run inside the `serve` future, so the server needs no spawner and
accepts backends that are not `Send`. This makes it usable on a single-threaded event loop as well as on a
multi-threaded runtime: the future is `Send` whenever its parts are.

## Where connections come from

A `Listener` yields connections. Each one is a pair of message halves (`ww_device::MessageTx` and `MessageRx`, the
traits every device medium implements) together with the peer.

The crate has one listener today, behind the `in_process` feature. `InProcessListener` accepts clients of the same
process, which connect with `ClientConfig::in_process_path`. It exists for tests and simulators, and it lets you
choose the peer that every connection reports, so that you can test what your service does for a peer you cannot
easily become:

```rust
let listener = InProcessListener::new("my_service", 16 * 1024)
    .with_peer(Peer::Unix { uid: Uid(1000), gid: Gid(100), pid: None });
```

## Order of requests

The requests of one connection run one at a time, in the order they arrive. A slow handler therefore delays the
later requests of its own connection, and no other connection.

While a handler runs, the connection keeps working. It reads further requests into a queue, answers the pings of the
link and writes out the events that the handler sends, such as stream items. A handler can send as many events as it
likes before it returns its reply, and the client receives them in order, before the reply.

## Limits

`Limits` bounds what a server and each connection may hold. `Server::new(clock).limits(limits)` sets them.

| Limit | Default | At the limit |
|---|---|---|
| `connections_max` | 64 | The server stops accepting. Further clients wait in the listener until a connection ends. |
| `pending_max` | 32 | The connection stops reading. The client's transport pushes back on the client. |
| `events_max` | 64 | A handler waits inside its send until the connection wrote events out. |
| `shutdown_grace` | 5 s | See below. |

None of these limits drops a request. A full queue slows down the client that filled it, and nobody else.

The longest message is not a limit of the server: it comes from the listener's medium and from the client, and link
setup agrees on the smaller of the two (see [Large messages](large_messages.md)). A reply over it is answered with
an error that names both sides' limits.

## Shutting down

When the shutdown future completes, the server stops accepting connections. Each connection then answers the
requests it already has, tells its client that the server is going away and ends. A connection with nothing to
answer ends at once.

Requests that are still running when `shutdown_grace` has passed are dropped, and their connections are
disconnected. `serve` returns after the last connection ended.
