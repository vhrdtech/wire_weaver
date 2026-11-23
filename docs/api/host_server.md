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
| `pumps` | The connection's [pumps](#streams-are-pumps), for the backend to keep. |

### The backend's entries

The connection calls the generated dispatcher through two traits of `wire_weaver`, which the backend implements by
forwarding to the generated functions. The second one passes the connection's origin and serves the requests that
run side by side:

```rust
impl WireWeaverAsyncApiBackend for Backend {
    type Medium = ();

    async fn process_bytes<'a>(&mut self, out: &mut EventWriter<'_, impl MessageSink>, medium: (), data: &[u8],
                               scratch: &'a mut [u8]) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(data, scratch, out, medium).await
    }

    fn version(&self) -> FullVersion<'_> {
        my_api::MY_API_FULL_GID
    }
}

impl WireWeaverSharedApiBackend for Backend {
    async fn process_exclusive_bytes<'a>(&mut self, origin: &CallContext, out: &mut EventWriter<'_, impl MessageSink>,
                                         medium: (), data: &[u8], scratch: &'a mut [u8])
                                         -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_from(origin, data, scratch, out, medium).await
    }

    fn is_shared(data: &[u8]) -> bool {
        Backend::is_shared(data)
    }

    async fn process_shared_bytes<'a>(&self, origin: &CallContext, out: &mut EventWriter<'_, impl MessageSink>,
                                      medium: (), data: &[u8], scratch: &'a mut [u8])
                                      -> Result<&'a [u8], ShrinkWrapError> {
        self.process_shared_from(origin, data, scratch, out, medium).await
    }
}
```

Every server has these generated functions. Without a `receiver_model` its `is_shared` says no to every request, so
all requests run alone; with one, its calls and reads run side by side as the next section describes. A firmware
never calls the shared ones, and its image is the same size with them.

A server generated with `serve_trait` gets both implementations and every handler from the API's local trait: the
backend is a struct with the service and the pumps ([Serving a local
implementation](local_trait.md#serving-a-local-implementation)).

### Who asked

`Connection::origin()` is the `CallContext` of the connection: its peer and its id. The connection passes it with
every request, and a handler reads it from `cx.call()`, with the request's own `seq`, priority, trace and what is
left of its [deadline](#deadline-and-cancel):

```rust
async fn whoami(&self, cx: &mut Context<'_, impl EventOut>) -> RpcResult<u32> {
    match cx.call().peer {
        Peer::Unix { uid, .. } => Ready(uid.0),
        _ => Ready(0),
    }
}
```

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

### Connections as tasks

Inside `serve`, all connections share the one thread that polls it. A server on a multi-threaded runtime can spread
them over its threads instead. `Server::accepting` takes the same arguments as `serve` and hands out each
connection as a future, which you start as a task:

```rust
let mut accepting = server.accepting(link, listener, make_backend, shutdown);
while let Some(connection) = accepting.next().await? {
    tokio::spawn(connection);
}
```

The limits and the shutdown work as in `serve`. `next` gives no connection while `connections_max` of them run,
and it returns `None` once the shutdown was asked for and every connection has ended. A connection is `Send` when
the clock, the listener's message halves and the backend are, so the compiler checks this where you spawn.

This loop is the server's spawner. A method that spawned the connections itself would have to require them to be
`Send` for any listener and backend. Rust cannot state that bound for the futures that traits like `MessageTx`
return, so the server hands the connections to the code that knows the concrete types.

## Where connections come from

A `Listener` yields connections. Each one is a pair of message halves (`ww_device::MessageTx` and `MessageRx`, the
traits every device medium implements) together with the peer. When a listener says that no more connections come
(a server on one stream, a source that closed), `serve` ends once the connections it has ended, and `accepting`
gives `None`.

### Clients in the same process

`InProcessListener` (feature `in_process`) accepts clients of the same process, which connect with
`ClientConfig::in_process_path`. It exists for tests and simulators. It lets you choose the peer that every
connection reports, so that you can test what your service does for a peer you cannot easily become:

```rust
let listener = InProcessListener::new("my_service", 16 * 1024)
    .with_peer(Peer::Unix { uid: Uid(1000), gid: Gid(100), pid: None });
```

### Byte streams

Everything else is a byte stream: a TCP connection, a local socket, the process's own stdin and stdout, or a stream
the program accepted itself. `byte_stream::listen` makes a listener from a source of such streams and a framing,
which says how a connection opens and how its messages are cut ([byte streams](../transport/byte_streams.md)):

```rust
let listener = listen(TcpIncoming::bind("0.0.0.0:7700").await?, PreambleFraming::new("my_service 0.3.0", 16 * 1024), clock);
```

| Source | Feature | Peer |
|---|---|---|
| `TcpIncoming::bind(addr)`, `TcpIncoming::new(listener)` | `tokio` | `Peer::Addr` |
| `LocalSocketIncoming::new(socket)` | `local_socket` | `Peer::Unix` with the client's process |
| `stdio()`: one client, the process's stdin and stdout | `tokio` | `Peer::Unix` of the process's own user |
| `handed_in(n)`: streams the program passes to `StreamSender::serve_stream(stream, peer)` | none | the one passed |

| Framing | Clients | What names the API |
|---|---|---|
| `PreambleFraming` | `ClientConfig::tcp`, `command`, `stream`, `ipc_path` | the preamble's target |
| `WsFraming` | `ClientConfig::websocket_url`, browsers | the URL's path |

The listener reads up to `HANDSHAKES_MAX` openings side by side, each within `OPENING_TIMEOUT` (two seconds), so a
client that connects and sends nothing delays nobody else. A source can be any `Stream` of `(stream, peer)` results
with futures-io streams; tokio streams are wrapped with `tokio_util::compat`. The peer comes from the source, never
from what the client says about itself.

### Several APIs on one source

`listen` returns the listener of the default API: preamble clients that ask for a device, WebSocket clients on `/`.
`StreamListener::service(name)` makes another listener on the same source, for the clients that ask for the service
`name` (`ClientConfig::service(name)`, or the path `/name`). Each listener goes to a server of its own, with its own
API, link setup and version check:

```rust
let tasks = listen(TcpIncoming::bind(addr).await?, PreambleFraming::new("cs-stored 0.1.0", 64 * 1024), clock);
let journal = tasks.service("journal");
let tasks_server = Server::new(clock).serve(tasks_link, tasks, make_tasks, shutdown_a);
let journal_server = Server::new(clock).serve(journal_link, journal, make_journal, shutdown_b);
futures_util::future::try_join(tasks_server, journal_server).await?;
```

A client that asks for a service nobody serves there is refused: `Error::HubRefused` with the status
`UnknownService` (an HTTP `404` over WebSocket). When a listener holds `QUEUED_MAX` connections that its server has
not taken yet, the next client is refused as busy.

### Connections the program opened

A program that answers each client's opening itself, because it serves several things on one socket, hands the
connections of one API to a server through `opened`. The [ww hub](../transport/hub.md#its-api) works this way: its
preamble picks the hub's own API or a tunnel to a device, and only the API's connections go to the server.

```rust
let (mut sender, listener) = wire_weaver_server::opened::opened(16);
// in the program's own accept loop, once it answered the client's preamble:
let (tx, rx, _ended) = wire_weaver_client::local_socket::device::connection_medium(read, write, 64 * 1024);
sender.serve(tx, rx, wire_weaver_server::local_socket::peer_of(peer)).await?;
```

The server starts each such connection with link setup. `OpenedSender::serve` waits while the listener holds 16
connections that its server has not taken yet, and fails with `Closed` once the listener is gone. The server ends
after its connections once every sender is dropped.

### Programs on the same computer

`LocalSocketIncoming` (feature `local_socket`) accepts programs of the same user on a local socket: a Unix domain
socket on Linux and macOS, a named pipe on Windows. A client connects with `ClientConfig::ipc_path`, the same way it
reaches a device that another process serves ([the hub](../transport/hub.md) uses these sockets too).

```rust
let socket = ww_local_socket::listen(&path, false)?;
let listener = listen(LocalSocketIncoming::new(socket), PreambleFraming::new("my_service 0.3.0", 16 * 1024), clock);
```

`listen` makes a socket reachable by its owner only, and the listener checks the user of every client again when it
accepts it. A service other users reach listens with a [policy](#who-may-call). On Unix the peer of a connection is
`Peer::Unix` with the client's user, group and process id. On Windows the pipe's client is checked the same way, but
`Peer` has no variant for a Windows user yet (SRV-71), so handlers see `Peer::Unknown` there. Any device name reaches
the API of this server, and a client that asks for the hub's own API is refused.

### Hosts on UDP

`UdpListener` (feature `udp`) serves hosts on one UDP socket, a connection per host address, as a
[device on UDP](../transport/udp.md#several-hosts-on-one-port) does. Clients connect with
`ClientConfig::udp_addr`, the same way they reach a device on the network:

```rust
let listener = UdpListener::<_, 8>::bind("0.0.0.0:9000".parse()?, 16 * 1024)?; // feature `tokio` too
Server::new(clock).serve(link, listener, make_backend, shutdown).await?;
```

`PEERS` (8 here) is how many hosts are served at once. A host gets a connection when it starts link setup and keeps
it until its link goes down: it leaves, or goes quiet for the link's peer timeout. With every slot taken, a new host
ends the connection heard from least recently and is served from its next hello. The peer of a connection is
`Peer::Addr` with the host's address, which proves nothing about who the host is: anyone who can reach the port is
served. Messages longer than a datagram are split across several, up to the listener's longest message (16 KiB
here).

`UdpListener::stop_handle` gives a handle that stops this listener alone: it takes no new host, and its
connections go on until they end ([moving to another address](#moving-to-another-address)). A receive error about
one host, such as an ICMP error of an earlier send, is skipped; an error of the socket itself ends `serve`.

Connections are `Send` when the socket is `Sync`, so they can run on any thread of a multi-threaded runtime
([connections as tasks](#connections-as-tasks)). On another event loop,
`UdpListener::new` takes a `ww_device::udp::StdUdpSocket` with that loop's readiness, or any `DatagramSocket`;
[on calloop](#on-a-calloop-loop) `ww_calloop::bind_udp` does it.

### On a calloop loop

A program built on [calloop](https://crates.io/crates/calloop) instead of tokio (a Wayland compositor, a GUI with
its own loop) serves through `ww_calloop`. It gives the server what comes from the loop: `LoopClock` (timers on the
loop's timers), `TcpIncoming` (TCP clients for a byte-stream listener) and `bind_udp` (a `UdpListener` whose socket
the loop watches). `serve` runs on the loop's executor, so backends may hold `Rc` state of the program:

```rust
let (executor, scheduler) = calloop::futures::executor()?;
event_loop.handle().insert_source(executor, |(), _, _| ())?;
let handle = event_loop.handle();
let clock = LoopClock::new(handle.clone());
let ws = listen(TcpIncoming::bind(&handle, "0.0.0.0:7000")?, WsFraming::new(16 * 1024), clock.clone());
let udp = bind_udp::<_, 8>(&handle, "0.0.0.0:7000".parse()?, 16 * 1024)?;
let server = Server::new(clock);
scheduler.schedule(async move {
    let _ = join(
        server.serve(link(), ws, &make_backend, shutdown.clone()),
        server.serve(link(), udp, &make_backend, shutdown),
    )
    .await;
})?;
```

WebSocket clients (`ClientConfig::websocket_url`) and UDP clients (`udp_addr`) then reach the same API on one port
number, each connection with its client's address as the peer.

#### Moving to another address

A program whose address changes while it runs (a setting the user edits) stops the old listeners and serves new
ones beside them. Each listener has a stop handle for this, which you take before the listener goes to `serve`:

```rust
let tcp = TcpIncoming::bind(&handle, old_addr)?;
let tcp_stop = tcp.stop_handle();
let udp = bind_udp::<_, 8>(&handle, old_addr, 16 * 1024)?;
let udp_stop = udp.stop_handle();
// later, in a callback of the loop, when the address changed:
tcp_stop.stop();
udp_stop.stop();
// bind the new address and schedule another `serve` for it
```

`TcpStop::stop` closes the TCP socket at once, so the old address is free to bind again and new clients are
refused. `UdpStop::stop` makes the UDP listener take no new host. The UDP socket stays open until the last of its
connections ends, since they all send and receive through it; a new host ends none of them. In both cases the
connections the listener accepted go on, and its `serve` returns once they ended. A shutdown future, in contrast,
ends every listener and every connection of a server.

A client whose TCP stream fails its setup on the loop (`set_nodelay`, or the loop can't watch the stream) is
dropped, and the listener goes on with the others.

### A server on stdio

A server run as an ssh forced command, or started by its client, serves its stdin and stdout. Logs go to stderr then:

```rust
let listener = listen(stdio(), PreambleFraming::new("my_service 0.3.0", 16 * 1024), clock);
Server::new(clock).serve(link, listener, make_backend, std::future::pending::<()>()).await?;
```

The client starts it with `ClientConfig::command("ssh", ["host", "my-service", "--stdio"])`. The server ends when its
client leaves.

### Socket activation

Under systemd, a `.socket` unit can own the socket and start the service when the first client connects. The
service then takes the socket it was passed instead of creating its own. `ww_local_socket::inherited` returns it,
and it fails with `ActivationError::NotActivated` when the program was started without one, so that the same
program can also run by hand:

```rust
let socket = match ww_local_socket::inherited(Some("api"), PeerPolicy::SameUser) {
    Ok(socket) => socket,
    Err(ActivationError::NotActivated) => ww_local_socket::listen(&path, false)?,
    Err(e) => return Err(e.into()),
};
let listener = listen(LocalSocketIncoming::new(socket), PreambleFraming::new("my_service 0.3.0", 16 * 1024), clock);
```

The units of a user service look like this. `FileDescriptorName` is the name that `inherited` looks for; without a
name, `inherited(None)` takes the first socket passed.

```ini
# ~/.config/systemd/user/my-service.socket
[Socket]
ListenStream=%t/my-service.sock
SocketMode=0600
FileDescriptorName=api

[Install]
WantedBy=sockets.target

# ~/.config/systemd/user/my-service.service
[Service]
ExecStart=%h/.cargo/bin/my-service
```

The socket file and its mode are the unit's, so `SocketMode=0600` keeps other users out. `inherited` checks that
the file lets in no one the policy doesn't: a looser mode, another group or an abstract socket (`ListenStream=@name`,
which has no permissions at all) is refused with `ActivationError::Loose`. A passed socket is taken once per
process, and the programs that the service starts do not inherit it.

## Who may call

Who called is the transport's word, never the client's own: `ClientMeta` (program, user, host) is a label for logs.
The peer each listener gives its connections:

| Listener | `Peer` |
|---|---|
| local socket, Linux and macOS | `Unix { uid, gid, pid }` from `SO_PEERCRED` / `getpeereid` (no pid on macOS) |
| local socket, Windows | `Unknown`, after the same-user check (SRV-71) |
| stdio | `Unix` with the server's own user: whoever could start it |
| TCP, UDP | `Addr`: an address, which proves nothing |
| a stream the program accepted | the `Peer` it passes to `serve_stream`: `Key` for a key the other side proved |
| in process | `Local`, or any peer given with `with_peer` (tests) |

A request a trusted mount forwarder sent on is its client's: `Server::trust_forwarders` names the peers whose word on
that is taken, and `cx.call().forwarded` is set ([mounts](mounts.md#who-calls-and-what-it-does)).

Two places decide: the socket keeps out who must not connect at all, and a hook decides each request.

### Listener policy

`ww_local_socket::listen_with` takes a `PeerPolicy`, which sets the modes of the socket file and of its directory
(`dir_with`) and is checked again for every connection the listener accepts:

| Policy | Socket file | Directory | Accepted |
|---|---|---|---|
| `SameUser` (the default, `listen`) | 0600 | 0700 | the server's user |
| `Users { group, uids }` | 0660, group `group` | 0710, group `group` | the server's user and `uids`; every member of `group` when `uids` is empty |
| `Any` | 0666 | 0711 | every local user, their uid passed to the hook |

```rust
let dir = Path::new("/run/my-service");
let policy = PeerPolicy::Users { group: service_group_gid, uids: vec![] };
ww_local_socket::dir_with(dir, &policy)?;
let socket = ww_local_socket::listen_with(&dir.join("api.sock"), false, policy)?;
```

The kernel checks group membership, supplementary groups included, when a client opens the socket file: the peer
credentials carry only the effective group, so the listener can't. The socket is bound at 0600 and gets its group
before its mode is widened, so it is never open to the wrong group. Nothing is world-writable unless the policy is
`Any`. A client of another user's service checks that service's user: `ww_local_socket::connect_to(path,
server_uid)`, in a `ClientConfig::stream` opener. Windows takes `SameUser` only (SRV-71).

### Authorization

`Server::authorize` puts a hook in front of every handler. The server asks it for every request, shared or alone,
before the request starts. A request it denies is answered `ErrorKind::Denied` and its handler never runs. A request
the server can't place (malformed, or of a kind this build doesn't know) is denied without asking. Link setup and
pings are not requests and are not asked about.

The hook is `wire_weaver::authorize::Authorize`, the same one a call in the same process asks through
[`Authorized`](local_trait.md#authorization-in-the-same-process): one `Arc` of a hook can guard both, and sees the
same path and items for the same call.

```rust
use wire_weaver::authorize::{Access, Denied, Resource, from_fn};

let api = Arc::new(ApiBundleOwned::from_ww_bytes_owned(&ww_self::compressed::bundle_bytes(
    &api_impl::WW_SELF_BYTES,
)?)?);
let hook = from_fn(|call: &CallContext, resource: &Resource<'_>| match call.peer {
    Peer::Unix { uid, .. } if admins.contains(&uid.0) => Ok(()),
    Peer::Unix { .. } if matches!(resource.access, Access::Read | Access::Introspect) => Ok(()),
    _ => Err(Denied),
});
Server::new(clock).authorize(hook, Some(api)).serve(link, listener, make_backend, shutdown).await?;
```

`Resource` says what the request does where:

- `access`: `Call`, `Write`, `Read`, `Stream`, `Observe` or `Introspect`;
- `place`: the path as the request names it, absolute or a trait wherever it sits (`TraitRef`);
- `path`: the path from the root, array indices included, also of a global path when the server has the API's model;
- `multi`: a multi request, which reaches resources under `path`;
- `items`: a `Step` per item on the way, from the root's level down to the item: its name, id, array index,
  [effects](metadata.md) (`#[effect]`, `read` by kind; a property's describe its write) and `ww_self` metadata
  (`#[claim]`, `#[label]`, `#[shares]`). A keyed array's `index_of`, `key` and `keys` are a step after the array's
  `keyed!` item, all `read`. `None` without the model; `resource.item()` is the last step, `resource.effects()` its
  effects;
- `api`: the model, for what the steps leave out (`api.item_chain(path)` gives the items' kinds and types).

A hook that names what it allows and answers `Denied` to the rest denies by default, also items added to the API
later. Without `authorize` every request runs.

### Waiting for an answer

`check` answers a `Verdict`: `Allow`, `Deny`, or `Pending`, an answer that comes later. A pending verdict is a future
of `Ok(())` or `Err(Denied)`: a person approving an agent's `outbound` or `destructive` call, a second factor, a
policy service. The check itself stays quick and never blocks; what takes long is the future.

```rust
let hook = from_fn(move |call: &CallContext, resource: &Resource<'_>| {
    let effects = resource.effects();
    if !(effects.contains(Effects::OUTBOUND) || effects.contains(Effects::DESTRUCTIVE)) {
        return Verdict::Allow;
    }
    // what the person reads, owned: the resource lives only as long as the check
    let question = format!("{:?} asks for {:?}", call.peer, resource.path);
    let answer = approvals.ask(question); // a future of bool
    Verdict::Pending(Pending::new(async move {
        if answer.await { Ok(()) } else { Err(Denied) }
    }))
});
```

The server waits for the answer as part of the request:

- the request counts as running and holds its place in the [order](#order-of-requests): a pending shared call lets
  the other shared calls run, a pending write holds the requests after it;
- its [deadline and cancel](#deadline-and-cancel) end the wait as they end a handler, and dropping the future
  withdraws the question (the approval queue sees its sender go away);
- an approval starts the handler, a refusal answers `Denied`;
- a request without a deadline waits as long as the answer takes and holds one of `Limits::requests_max` meanwhile,
  so a hook that asks a person bounds its own future (a timeout that answers `Err(Denied)`) and limits the open
  questions per peer.

Nothing changes on the wire: the client sees a slower answer, then the result, `Denied` or `DeadlineExceeded`. A
client that may wait for a person gives that call a longer timeout.

Why a future, not a "pending" answer on the wire: a sync policy costs nothing (no allocation, no `.await` in the
connection's loop), and an approval reuses what every request already has, its deadline, its cancel and the
connection's order. An answer "ask again later" would need a new error kind, a ticket to retry with in every client,
the dynamic client, Python and the local trait, and a second state machine for the same decision. In the same process
`Authorized` awaits the same future in the call's own, so both paths have one shape.

## Order of requests

The requests of one connection start in the order they arrive. How they run depends on the item, as the
`receiver_model` of the server says ([shared handlers](handlers.md)):

- **Shared** calls and reads run side by side through their `&self` handlers. A quick call is answered while a slow
  one still runs, and replies come back in the order the calls finish.
- **Everything else runs alone**: property writes, stream opens and closes, sinks, observing, and every item that the
  model leaves exclusive. Such a request waits until the shared requests before it have finished, and the requests
  after it wait until it has finished. A write followed by a read is therefore never reordered.

A server without a `receiver_model` runs every request alone, one at a time. In every case a slow handler delays
only the later requests of its own connection, never another connection.

A request that runs alone waits for the shared ones before it until they finish or their deadline ends them. So one
slow call holds up every request after the next write on the same connection, for as long as the call's deadline
allows. An API with calls that take long should make such a call a stream, or the client should send its writes on
another connection.

While a handler runs, the connection keeps working. It reads further requests into a queue, answers the pings of the
link and writes out the events that the handler sends, such as stream items. A handler can send as many events as it
likes before it returns its reply, and the client receives them in order, before the reply.

## Deadline and cancel

A client tells the server how long it waits for each request, and the server stops working on requests nobody waits
for any more:

- **Deadline.** A request carries what was left of the client's timeout when it was sent. The server drops the
  request when that time runs out, whether it still waits in the queue or its handler runs, and answers
  `DeadlineExceeded`. A handler that is dropped stops at the `.await` where it waits; a write that was still queued
  never runs.
- **Cancel.** When a client gives up on a request (its timeout passed, or the future of a `call()` was dropped), it
  sends a cancel for it. The server drops the request, queued or running, and answers `Cancelled`. A request that
  finished already is not answered again.
- **Trace.** A client that calls with `with_call_context(&cx)` sends `cx.trace`, the W3C trace context of its span.
  A handler reads it from `cx.call().trace`, next to `cx.call().deadline`.

These terms travel in the request's context, an optional part of the request on the wire. The server says in link
setup that it reads contexts, and only then does a client send them; a firmware never says so, so devices get the
requests as before. The errors a server answers with (`DeadlineExceeded`, `Cancelled`, `Denied`, `Internal`) reach a
client whose `ww_client_server` predates them as a `UserStr` with the same text. The client maps both kinds to the
`CallError` of the same name; its own timeout is `DeadlineExceeded` too, so a caller sees one kind whichever side
gave up first.

## Sending without being asked

A service also sends on its own: stream items when something happens, and the values of observed properties when
they change. A device does this in its main loop, between requests. A host backend sends stream items through
pumps, which run whatever else runs, and everything else in the three methods of the `Backend` trait.

### Streams are pumps

A pump is a stream of items that the connection serializes and sends as they come, also while calls run. A stream's
open handler adds one to the connection's `Pumps`, and its close handler removes it:

```rust
const NEWS: PumpId = PumpId::new(1);

async fn news(&mut self, _cx: &mut Context<'_, impl EventOut>) -> RpcResult<()> {
    let items = self.shared.news.subscribe().filter_map(|item| ready(item.ok()));
    self.pumps.add(NEWS, items, |item, buf| api_impl::stream_data_ser().news(item, buf).map(<[u8]>::len));
    Ready(())
}

async fn close_news(&mut self, _cx: &mut Context<'_, impl EventOut>) {
    self.pumps.remove(NEWS);
}
```

The backend keeps the handle that comes with `Connection::pumps`. A pump starts once the answer of the request that
added it went out, so the client knows the stream before its first item. It waits while the connection's event queue
is full, and it ends with its stream or with the connection, whoever still holds a handle. An item longer than the
link's message limit is skipped. `add_ending` adds a pump that sends one more event when its stream ends, such as
the stream's `Close` sideband (`stream_data_ser().news_sideband(StreamSideband::Close, buf)`), so the client's
stream ends with it.

### Streams that are not `Send`

`Pumps` take `Send` streams, so a connection stays `Send` and can run on any thread. A service on one thread, with
its state in `Rc` and `RefCell`, has streams that aren't `Send`. Its connections take `LocalPumps` instead, with the
same `add`, `add_ending` and `remove`. The factory says which: a closure of `&Connection` gives `Pumps`, one of
`&LocalConnection` wrapped in `Local` gives `LocalPumps`, and a `MakeBackend` of your own names them as its
`type Pumps`:

```rust
let make = Local(move |c: &LocalConnection| MyBackend::new(shared.clone(), c.pumps.clone()));
server.serve(link, listener, make, shutdown).await?;
```

Such connections are not `Send`. `serve` runs them on the thread that polls it; with `accepting`, spawn them as
local tasks. A server from a local trait without `Send` is the main user ([Local trait](local_trait.md#without-send)).

### The backend's own turn

The `Backend` trait has three methods for the rest, such as observed properties:

| Method | What it is for |
|---|---|
| `wait` | Completes when the backend has something to send. The connection waits on it while no request runs. |
| `next_wake` | The time the backend wants to send next, such as the time an observed property is due. |
| `poll` | Sends what is due. It runs after every request, when `wait` completes and at `next_wake`. |

All three have defaults that do nothing, so a backend that only answers requests needs one line:

```rust
impl wire_weaver_server::Backend for Backend {}
```

`wait` is dropped whenever a request arrives, so it must be cancel-safe: anything it took in before it was dropped
stays in the backend. `poll` always runs to its end before the next request starts. All three take `&mut self`, so
they run only while no request runs, shared ones included.

### When a connection ends

A client may hold something in the program while it is connected: keys it pressed, a lock it took, a region of the
screen it covered. When the connection ends, the backend's `end` method lets go of it. It runs once, after the last
request of the connection ended and before the backend is dropped, whatever ended the connection: the client left,
its link timed out, or the server shut down.

```rust
impl wire_weaver_server::Backend for Backend {
    async fn end(&mut self) {
        // the client is gone: nothing it pressed stays pressed
        self.input.release_all().await;
    }
}
```

The client hears nothing from `end`, since its link is gone already. The server gives `end` at most
`shutdown_grace` and then drops it, so a backend that waits on something that never comes can't hold up the server.
`end` doesn't run when the connection's future itself is dropped before it finished: when the program drops the
future of `serve`, or aborts a task that [`accepting`](#connections-as-tasks) handed out.

### One value for all clients

An observed property of a host service usually belongs to the service, and any client may change it. Each
connection has its own `ObserveState`, because each client chooses what it observes and how often. A write
therefore has to reach the backends of the other connections.

`Fanout<T>` carries such news. It gives every subscriber a queue of its own. A subscriber that reads slowly loses
its oldest items and learns how many, and it never delays the sender or the other subscribers.

```rust
struct Shared {
    mode: Mutex<u8>,
    mode_changed: Fanout<()>,
}

impl Backend {
    async fn set_mode(&mut self, _cx: &mut Context<'_, impl EventOut>, value: u8) -> SetResult<CommonError<'_>> {
        *self.shared.mode.lock().unwrap() = value;
        self.shared.mode_changed.send(()); // every connection hears of it
        Set
    }
}

impl wire_weaver_server::Backend for Backend {
    async fn wait(&mut self) {
        // `mode_changes` is this connection's `shared.mode_changed.subscribe()`
        if self.mode_changes.recv().await.is_some() {
            self.mode_changed = true;
        }
    }

    fn next_wake(&mut self, now: Instant) -> Option<Instant> {
        self.observe.next_wake(now.as_micros()).map(Instant::from_micros)
    }

    async fn poll(&mut self, out: &mut EventWriter<'_, impl MessageSink>, medium: (), now: Instant) {
        if std::mem::take(&mut self.mode_changed) {
            self.mark_changed_mode();
        }
        let mut buf = std::mem::take(&mut self.update_buf);
        let _ = self.poll_properties(out, medium, now.as_micros(), &mut buf).await;
        self.update_buf = buf;
    }
}
```

See [Observing properties](observing.md) for `ObserveState`, `mark_changed_<name>` and `poll_properties`. A
`Subscription` is also a `Stream`, so a stream that every listening client follows is a pump over one.

## Limits

`Limits` bounds what a server and each connection may hold. `Server::new(clock).limits(limits)` sets them.

| Limit | Default | At the limit |
|---|---|---|
| `connections_max` | 64 | The server stops accepting. Further clients wait in the listener until a connection ends. |
| `requests_max` | 256 | A request that would go over it, across all connections, is answered with `Overloaded`. |
| `calls_max` | 16 | Shared requests of one connection that run at once. Further ones wait in its queue. |
| `pending_max` | 32 | The connection stops reading. The client's transport pushes back on the client. |
| `events_max` | 64 | A handler waits inside its send until the connection wrote events out. |
| `shutdown_grace` | 5 s | See below. It also bounds each backend's `end`. |
| `disconnect_linger` | 200 ms | See below. |

None of these limits drops a request. A full queue slows down the client that filled it, and nobody else.

`requests_max` is the one limit that answers instead of waiting. Back pressure works per connection, so it can't
bound the work of all connections together: a request that would go over that bound is answered with the error
kind `Overloaded` at once, and its handler never runs. Since nothing ran, the client may send it again later. On
the client the error is `call::ErrorKind::Overloaded`, the same kind as a handler's common error `Busy`; a handler's
`Busy` speaks about a resource it guards, `Overloaded` about the server as a whole.

The longest message is not a limit of the server: it comes from the listener's medium and from the client, and link
setup agrees on the smaller of the two (see [Large messages](large_messages.md)). A reply over it is answered with
an error that names both sides' limits.

## Shutting down

When the shutdown future completes, the server stops accepting connections. Each connection then answers the
requests it already has, sends the stream items that are ready, disconnects its client and ends. The client is told
the reason the shutdown future completes with: `()` is `DisconnectReason::Shutdown`, and a future of a
`DisconnectReason` gives that one, as the ww hub gives `HubHandover` when a newer hub takes over (any type that
implements `ShutdownReason`). A future that never completes is `std::future::pending::<()>()`. It keeps the
connection open for `disconnect_linger` after that message, so that a client which is writing at that moment still
reads the reason instead of a transport error. A connection with nothing to
answer ends at once.

Requests that are still running when `shutdown_grace` has passed are dropped, and their connections are
disconnected. Each backend's [`end`](#when-a-connection-ends) then runs, again within `shutdown_grace`. `serve`
returns after the last connection ended.
