# ww_lz4 changelog

## Unreleased

### 🚀 Features

- `Encoder::init(slot, keep)`: sets an encoder up in place in a `static`'s `MaybeUninit` (a `StaticCell`'s `uninit()`); a `static` built by the `const fn new` keeps its whole buffer as an initializer in flash (20 KiB for `Encoder<4096, 12288>` on Donguru).
- First version (SRV-27): streaming LZ4 block encoder, `no_std`, no alloc, ported from the P2534 `lz4s` bench. `Encoder<HASH, BUF>` (`const fn new(keep)`, `with_backoff`, `input`, `compress`, `compress_slice`, `reset`, `max_chunk`, `HISTORY`), `max_block_len`, `MAX_DISTANCE`, `Error`: one standard LZ4 block per chunk, matches reaching back into earlier chunks, a `u16` hash table, literal-only blocks with back-off for data that doesn't shrink; feature `ram` puts the hot functions in `.data.ww_lz4` on bare metal. With `std`: `Decoder` (`new`, `decode`, `reset`, `DecodeError`), the last 64 KiB as dictionary through lz4_flex. Example `ratio` prints ratio and speed over captured streams.
