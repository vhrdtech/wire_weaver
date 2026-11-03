---
level: advanced
---

# Large messages and result sets

Every message on a link has a limit. This page explains where the limit comes from, what happens at it, and how to
return results that grow with the data.

## The link's limit

Each side says the longest message it takes during link setup:

- The device's limit is `dev_max_message_len` in its `DeviceInfo`. On `ww_device` it is the size of the receive buffer
  (`MessageRx::max_message_len`).
- The host's limit is `ClientConfig::max_message_len`, 16 KiB by default (`DEFAULT_MAX_MESSAGE_LEN`). It is sent in
  `LinkSetup`, and the transport's receive buffers are sized for it.

The link's limit is the smaller of the two, and it holds both ways:

```rust
let client = Device::config(|c| c.max_message_len(256 * 1024)).connect().await?;
```

On the host, `DeviceApiInfo::message_limit()` gives a `MessageLimit` with both limits, `agreed()` and `set_by()`. On
the device, it is `DeviceLink::agreed_max_message_len`.

A hub carries messages on a link of its own to the device, with its own limit, so a client asking for more through a
hub is still bound by that link.

## Over the limit

Nothing over the limit is sent, and the call fails at once instead of waiting for its timeout:

- A request over the limit is refused on the host before it is sent.
- A reply over the limit is refused by the device, which answers with `ResponseSerFailed` at
  `err_seq::REPLY_OVER_LIMIT`.

Both reach the caller as `Error::MessageTooBig`, and its text names both numbers and the side that set the limit:

```text
reply is over the link's message limit of 16384 bytes, set by the host (device 65536 from its DeviceInfo, host 16384 from ClientConfig::max_message_len)
```

Through a [local trait](local_trait.md#errors) the same error is `ErrorKind::MessageTooBig` (and
`CallError::MessageTooBig`), which carries the numbers: `error.too_big()` gives which message (`MessageKind::Request`
or `Reply`), its length when known and the link's limit, as `Bytes<u32>`.

Raise the smaller limit, or better, stop sending the result as one message, see [Result sets](#result-sets).

## Replies longer than the scratch buffer

A `no_alloc` server writes a reply into the fixed `scratch` buffer given to `ww_device::Server`, so that buffer also
limits its replies. A reply that doesn't fit is answered with `ResponseSerFailed`.

A `no_alloc = false` server serializes a handler's owned value into a growable buffer first:

- A value that fits is copied into `scratch` and goes out as usual.
- A longer one is sent from the growable buffer as a whole event, up to the link's limit, without being copied into
  `scratch`. So `scratch` can stay small while the link's limit is large.

This covers method returns, property reads and the deferred `<method>_send_return`, which also answers the call with
the over-limit error when the sink refuses the reply. Three cases stay bound by a fixed buffer:

- An item of a [multi request](multi_requests.md) goes into the one reply the request shares, in `scratch`.
- A `use_async = false` server on an async event loop queues what it sends in `event_scratch` (`EventWriter::queued`),
  so a long reply has to fit there. On `ww_device::blocking::Server` it doesn't.
- A stream item goes through `event_scratch`, as it does in a `no_alloc` server.

## Result sets

A result that grows with the data doesn't belong in one reply, whatever the limit is. A big message holds up every
other call on the connection while it is sent, and the limit is hit one day in production. There are two idioms for
it, and both keep each message small.

### A stream of rows

A [stream function](streams.md#stream-functions) takes the query as arguments, and each item is one row:

```rust
#[ww_trait]
trait Archive {
    /// Rows matching `filter`, closed by the device after the last one.
    #[stream(Reading<'i>)]
    fn scan(filter: Filter<'i>);
}
```

The device sends the rows after the open is answered, then closes the stream:

```rust
// in the event loop: what the device sends before the open is answered doesn't reach the handle
for row in rows {
    api_impl::stream_data_ser().scan_send(&row, out).await?;
}
api_impl::stream_data_ser().scan_sideband_send(StreamSideband::Close, out).await?;
```

On the host, the end of the result is the end of the stream. A plain `Stream` shows the device's `Close` to `recv_any()`
as `Sideband(Close)`. The [local trait](local_trait.md) maps the stream form to an `ItemStream`, which simply ends
there, so a local implementation and the client look the same to a call site:

```rust
let mut rows = archive.scan(&CallContext::local(), filter).await?; // ItemStream<ReadingOwned>
while let Some(row) = rows.next().await {
    show(row?);
}
```

A plain stream doesn't promise delivery: a lost row is gone unnoticed, and the host drops items beyond
`ClientConfig::stream_queue_bytes`. A result that must arrive whole is a [counted or reliable
stream](counted_streams.md), or pages.

### Pages with a cursor

A method takes a cursor and answers with a page and the next cursor:

```rust
#[ww_trait]
trait Archive {
    /// Up to `limit` rows from `cursor` on, from the start without one.
    fn page(filter: Filter<'i>, cursor: Option<Cursor>, limit: Fixed<Row, One, u32>) -> Page<'i>;
}

pub struct Page<'i> {
    pub rows: RefVec<'i, Reading<'i>>,
    /// Where the next page starts, None on the last one.
    pub next: Option<Cursor>,
}
```

The host calls it until `next` is `None`. Each page is an ordinary reply, so calls from other tasks run between pages.
A cursor costs the device nothing between calls and needs no open stream, which fits a device that serves many hosts
or loses its link. Pick a page length well under the link's limit.

`tests/large_messages` has all of this in working code: a long reply through each path, both over-limit errors, 10 000
rows as a stream and paging, with one generic call site run on the client and on a local implementation.

## Keeping an eye on version skew

A long-running server sees clients of different ages, and a client built against an older API may ask for what no
longer fits. `DeviceLink` keeps who is connected for each session:

- `peer_version()` gives the user API version the host connected with (`PeerVersion`).
- `peer_meta()` gives the program, its `app_version`, the user and the host, as the host said in link setup.
