---
level: advanced
---

# Byte streams in depth

[Serial lines and other byte streams](../basic/byte_streams.md) reaches a host server over TCP or a command. This page
adds the listener's framing and what a handler learns about its peer.

Connects a client to a server over any reliable byte stream (TR-19): a TCP connection, the stdin and stdout of a
command (`ssh host my-service --stdio`), or a stream the program opens itself, such as an iroh bi-stream. The bytes
are the same as on a [local socket](hub.md#wire-format): a preamble that names what the client asks for, an answer,
then length-prefixed [ww_link](ww_link.md) messages. One listener can serve several APIs, each a named service.

The server side is `wire_weaver_server`'s byte-stream listeners, described with the [host server](../api/host_server.md#byte-streams).

## Client side

The `byte_stream` feature of `wire_weaver_client` (the `local_socket` feature turns it on) adds these targets to
`ClientConfig`:

| Method | Connects to |
|---|---|
| `tcp("host:7700")` | a TCP server |
| `command(program, args)` | a command it starts for the connection, over the command's stdin and stdout |
| `stream(StreamOpener::new(shown, open))` | the stream that `open` returns, a futures-io `AsyncRead + AsyncWrite` |
| `service(name)` | with any of the above or `ipc_path`: the service `name` instead of the server's default API |

```rust
let client = MyClient::config(|c| c.tcp("10.0.0.7:7700").service("journal")).connect().await?;
let over_ssh = MyClient::config(|c| c.command("ssh", ["nas", "my-service", "--stdio"])).connect().await?;
```

- A command is started for every connection and killed when the connection ends. Its stderr goes to the client's
  stderr, so a server's logs and ssh's prompts stay visible.
- `StreamOpener::new(shown, open)` takes a closure that opens a new stream for every connection; `shown` names it in
  errors and in the device info. Tokio streams are wrapped with `tokio_util::compat`. Two configs are equal when they
  hold the same opener.
- The connection is opened when the client connects, so a refusal is an error at once: `Error::HubRefused` with the
  server's status, such as `UnknownService` for a service the server doesn't have, instead of a timeout.

## Framing

The messages are framed by a length prefix and trust the stream: one lost or flipped byte puts the reader out of
step, which ends the connection. That is right for TCP, pipes and QUIC streams. A lossy stream, such as a UART or a
radio link, needs a framing with resynchronization and a checksum: `ww_framer` over the stream, a medium beside this
one, not built yet.

On the server, the framing is a parameter of the listener (`Framing`): `PreambleFraming` for the clients above,
`WsFraming` for [WebSocket](websocket.md) clients on the same kind of source. Under WebSocket framing the URL's path
names the API: `/` the default one, `/journal` the service `journal`.

## Security

A byte-stream server takes anyone who can reach the stream. On a local socket only the same user can; TCP has no
authentication or encryption yet (TLS is TR-20), so bind a TCP listener to a trusted network only, or reach the
server through ssh with `command`. Handlers see who connected in `cx.call().peer`: the TCP address, the local
socket's process, or the peer the program passed with a stream it accepted itself. A host server decides who may
do what with [listener policies and an authorization hook](../api/host_server.md#who-may-call).
