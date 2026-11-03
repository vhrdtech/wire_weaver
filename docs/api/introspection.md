---
level: advanced
---

# Introspection data

A device built with `introspect = "no_docs"` or `"with_docs"` keeps its whole API in flash, as a serialized
`ww_self` bundle (`WW_SELF_BYTES`, see [ww_self](../std_library/ww_self.md)). A client asks for it with
`RequestKind::Introspect` and gets every method, property, stream and type the device serves, with doc comments
when the device kept them. The dynamic client, `ww`, Python and the hub work from it, so they talk to a device
without its API crate.

```rust
wire_weaver::ww_codegen!(
    my_api :: MyApi for super::Server,
    server = true, no_alloc = true, use_async = true,
    introspect = "with_docs",
    introspect_compression = "lz4", // optional, see below
);
```

## What a device sends

The answer is a stream at the root path (`[]`), under the request's seq:

1. With compression only: a `StreamSideband::User(INTROSPECT_COMPRESSED)` marker
   (`ww_client_server::INTROSPECT_COMPRESSED`).
2. The bytes of `WW_SELF_BYTES` as stream data, 128 bytes per event.
3. `StreamSideband::Close`.

A device with introspection off answers with `Close` alone, and the client reports no introspection data. The
client joins the data events in order, so the chunk size is not part of the format.

Traits and types known from the API snapshots of `ww_stdlib` and the framework are left out of what a device sends,
and the client puts them back (`wire_weaver_snapshots`). What is left is mostly the device's own API and its doc
comments: on a real board the docs are the larger part.

## API hashes and the cache

`API_HASH_NO_DOCS` and `API_HASH_WITH_DOCS` (`api_hash()`, `API_ID`) are the hashes of the bundle without and with
docs, as a device would send it uncompressed. A device reports them at link setup, and a client looks the bundle up
in its cache (`~/.wire_weaver/`) by them before it downloads anything. `Introspect::get` uses the cache,
`download` and `download_promise` always ask the device and store what they got.

## Compressed introspection data

`introspect_compression = "lz4"` keeps `WW_SELF_BYTES` compressed. The bundle is compressed at build time with
`ww_lz4`'s encoder; the device only stores the bytes and sends them, it carries no decoder. The client decompresses
them before anything else, so the dynamic client, `ww`, Python, the hub and `ww tap` work the same with both. The
default is `"none"`. `ww api scaffold --introspect with_docs --introspect-compression lz4` prints the same arguments.

What it saves, release builds measured with and without the option from the same checkout (3 Nov 2026):

| Device | Firmware | Bundle | Flash (text + data) |
|---|---|---|---|
| rePort (RP2350, async server, `opt-level = "s"`) | 408,632 B | 14,016 -> 9,697 B | -3,880 B |
| Donguru (STM32G0, async server, `opt-level = "z"`) | 148,188 B | 3,974 -> 3,056 B | -1,228 B |

The difference between the bundle and the flash is code: the marker is one more event, sent from the same place as
the data and `Close` (+434 B of code on rePort, -284 B on Donguru). LZ4 does about 1.3-1.45x on this data, mostly
doc text; it pays off for a device with a large API or many doc comments.

The API's identity doesn't change: the hashes stay those of the uncompressed bundle, so a compressed and a plain
build of the same API have the same `API_ID`, share their cache entries, and are compared by the evolution rules
as the same API.

### The format

`ww_self::compressed` has the encoder for build time and the decoder for hosts. All integers are little-endian:

| Bytes | What |
|---|---|
| 4 | `WWz4`, never the first bytes of a bundle (it starts with `ww_self::MAGIC`) |
| 4 | the length of the bundle once decompressed |
| 4 + n | per block: its length `n`, then a standard LZ4 block of `n` bytes |

A block may refer back into the blocks before it, up to 64 KiB, as in a `ww_lz4` stream; a bundle up to 32 KiB is
one block. Any LZ4 block decoder with a dictionary reads it (`ww_lz4::Decoder`, lz4_flex's
`decompress_into_with_dict`). A host refuses a declared length over 16 MiB and blocks that decompress to another
length than the header says.

### Older clients

The data starts with its own magic, so a client tells compressed from plain bytes without the marker. The marker is
there for clients built before compression existed: they take no sideband but `Close` on the introspection stream
and stop with an "unexpected event" error at it, instead of reading the compressed bytes as a bundle. A client
without introspection data can still call the device through its API crate (`client = "std_client"`).

Old devices never send the marker, so new clients read them as before. Compression is not a wire change for
anything else: requests, events and the bundle format are the same.
