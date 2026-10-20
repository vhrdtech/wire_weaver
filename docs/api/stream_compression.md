---
level: advanced
---

# Compressed streams

A counted stream can send its events compressed: each event's item goes through LZ4 with the items before it as
history, so a byte stream of text (a UART carrying a shell or a TUI, a log) takes a third to a fifth of the link.
Nothing changes in the API: the device chooses per stream by the state it keeps for it, the host offers the codec
when it opens the stream, and an older host simply gets the events as they are.

Measured on shell and TUI captures cut into 512-byte chunks, the way a UART's receive stream sends them: 3.5-5x with
a 12 KiB buffer (4 KiB kept as history), 3.2-3.9x with 2 KiB; 64-byte chunks still get 2.2-2.9x. Without history
across chunks the same 512-byte chunks only get about 2x, and 64-byte ones nothing. Random or already compressed data
goes out as it is, a few bytes longer per event. On an STM32G0 at 64 MHz the encoder costs 38-50 cycles per byte run
from RAM (57-75 from flash), so a 6 Mbaud UART (600 kB/s) takes 36-47 % of the CPU and needs only 120-170 kB/s of
the USB link.

## Device side

A counted stream's state, what `stream_<name>()` returns, is anything that implements
`ww_stream::codec::CountedState`. `ww_stream::tx::Counted` sends items as they are; `ww_stream::codec::Compressed`
wraps one with a codec and compresses once the host offers that codec. With `ww_stream`'s `lz4` feature the codec is
`ww_lz4::Encoder<HASH, BUF>`: `HASH` hash table entries (2 bytes each) and a `BUF`-byte buffer for the history and the
item being compressed.

```rust
use ww_stream::codec::Compressed;

// 4096 hash entries (8 KiB) and 12 KiB of history, 4 KiB of it kept when the buffer slides: 20 KiB of RAM
type RxStream = Compressed<ww_lz4::Encoder<4096, 12288>>;
// set up in place: never build it on the stack, and not as a const either (`ConstStaticCell::new(Compressed::new(..))`
// works, but keeps the 20 KiB as an initializer in flash)
static RX: StaticCell<RxStream> = StaticCell::new();
let rx: &'static mut RxStream = Compressed::init(RX.uninit(), |c| ww_lz4::Encoder::init(c, 4096));

impl ServerState {
    fn stream_rx(&mut self) -> &mut RxStream {
        self.rx
    }
}

// the producer is unchanged: `<name>_send` serializes the item into the encoder's buffer and compresses it
ser.rx_send(state.rx, &chunk, chunk.len() as u64, Some(t), &mut server.sink()).await?;
```

Smaller settings trade ratio for RAM: `Encoder<1024, 2048>` takes 4 KiB. The `ram` feature of `ww_lz4` puts the
encoder's hot functions into RAM on bare-metal targets (cortex-m-rt copies `.data`), about 30 % fewer cycles on a
part with flash wait states. Batched and reliable streams don't compress yet.

## Host side

`wire_weaver_client` offers LZ4 whenever it opens or resumes a counted stream, and decodes before the items reach
the stream's queue: generated clients, the dynamic client and Python get the same items as from an uncompressed
stream. `ClientConfig::stream_compression(false)` (or `.compression(false)` on one open) turns the offer off.
`StreamStats` says what was saved: `codec_bytes_in` came over the link, `codec_bytes_out` is what they decoded to
(Python: the same keys in `stats`).

## Staying in step

The device and the host keep the same history only while every compressed event reaches the host. So the device
starts the history over, and marks that event in its head (`HeadExt::codec_restart`):

- at every open and resume;
- after an event it couldn't send (the encoder saw it, the host never will), whose units the next event reports as
  lost by the device;
- every `restart_every` bytes of items (32 KiB by default, `Compressed::restart_every`), so a loss on the link the
  device can't see costs at most that much.

Units the producer drops before they reach the stream (`lost()`, a full ring) never went through the encoder: the
history stays as it is. A host that misses an event on the link (a jump of the position) drops the events after it
until the next restart, and reports their units as a `Gap { cause: Link }`, counted in `StreamStats::lost_codec`.

## On the wire

- `RequestKind::StreamOpen { codecs }`: the codecs the host decodes (`StreamCodecs { lz4 }`, evolvable), `None` from
  a host that doesn't offer any.
- `StreamOpened { codec }`: the one the device took, `StreamCodec::Lz4 { history }` with how far back its blocks
  reach, or `None`: events as they are.
- Each event's data is one [LZ4 block](https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md) (no frame, no
  checksum: the link has its own), which decodes with the bytes of the items before it as the dictionary
  (`LZ4_decompress_safe_usingDict`, lz4_flex's `decompress_into_with_dict`). An item that doesn't shrink is a block
  of literals only, so there is no flag per event.
- `HeadExt::codec_restart` on the event where the history starts over.
