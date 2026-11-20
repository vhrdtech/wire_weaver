# ww_lz4

> Streaming LZ4 block encoder for no_std and no alloc: one standard LZ4 block per chunk, history kept across
> chunks

For a byte stream cut into chunks (a UART's RX, a log). Each chunk becomes one standard LZ4 block whose matches
may reach back into earlier chunks, up to the history `Encoder` keeps; a decoder that keeps the bytes it decoded
before as the dictionary decodes it (`Decoder`, feature `std`; lz4_flex's `decompress_into_with_dict` or
liblz4's `LZ4_decompress_safe_usingDict` elsewhere). History is what makes small chunks compress: 512 B chunks of
shell output go 3.5-5x with a few KiB of history, about 2x without. The encoder lives in a `static` (mostly
buffer, never built on the stack) and is set up in place by the `const fn` `Encoder::new`.

```rust
use ww_lz4::{Encoder, max_block_len};

// 2^12 hash entries (8 KiB) and 12 KiB of history and input, 4 KiB of it kept when the buffer slides
static mut ENC: Encoder<4096, 12288> = Encoder::new(4096);
let enc = unsafe { &mut *(&raw mut ENC) }; // or a StaticCell, a Mutex, ..

let mut block = [0u8; max_block_len(512)];
let input = enc.input(512)?; // the free tail of the history, at least 512 bytes
input[..4].copy_from_slice(b"log "); // DMA or serialize straight into it
let n = enc.compress(4, &mut block)?;
// send block[..n]; on stream open or lost bytes, enc.reset() and the decoder's reset()
```

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.
