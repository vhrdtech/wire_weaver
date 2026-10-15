# ww_log

`ww_log` is the log of a device: its `defmt` frames, decoded on the host with the firmware's ELF (`LogDefmt`), or
text records for firmware without `defmt` (`LogText`). Both are [reliable streams](../api/stream_reliability.md):
the device keeps what it logged until the host has it, also while no host is connected, and tells the host exactly
how many messages it dropped and where. It is `no_std` by default and depends only on `wire_weaver`.

```toml
[dependencies]
ww_log = "0.1"
```

```rust
#[ww_api_root]
pub trait MyDevice {
    ww_impl!(log: ww_log::LogDefmt);
}
```

The device side of `LogDefmt` is the `ww_defmt_logger` crate: the firmware's `defmt` global logger, which also
tees every frame to RTT (see [below](#the-device-side-ww_defmt_logger)).

## `LogDefmt`

```rust
#[ww_trait]
pub trait LogDefmt {
    const DEFMT: DefmtInfo<'i>;
    #[stream(DefmtFrames<'i>, reliable)]
    fn frames() -> Result<(), LogError<'i>>;
}

pub struct DefmtInfo<'i> {
    pub encoding: DefmtEncoding,                          // Raw, Rzcobs
    pub build_id: MaxLen<RefVec<'i, u8>, 32>,
}
pub struct DefmtFrames<'i> {
    pub bytes: MaxLen<RefVec<'i, u8>, 512>,               // one or more whole frames
}
```

* `DEFMT` - the encoding `defmt` is built with (its `encoding-raw` / `encoding-rzcobs` cargo feature), and the
  GNU build id of the firmware: the host takes the ELF with this build id to decode the frames (the format
  strings are interned there, not sent).
* `frames` - the log. **Its unit is one `defmt` frame**, so the stream head's `units` is the number of frames in an
  item and its `lost` is the exact number of log messages the host won't get. An item never holds half a frame: a
  decoder fed one item after the other never loses sync, also with raw encoding and after a loss.

Timestamps are `defmt`'s own (`defmt::timestamp!`), inside each frame; the head's `t` stays off.

### Logs from before the host connects

The device starts the stream's session at boot, without a host, and keeps logging into the store. A host that
wants these logs **resumes** that session from its start instead of opening the stream (opening starts a new
session and empties the store):

```rust
let status = device.log().frames_status().read().await?;
let cursor = StreamCursor { session: status.session.unwrap(), pos: UVlq64(0) };
let mut frames = device.log().frames_resume(cursor).call().await?;
```

The backlog arrives as the backfill run while new logs come live, see
[stream reliability](../api/stream_reliability.md). When the store is full, its `FullPolicy` decides: with
`DropOldest` it keeps the latest logs, which matter most after a fault, and the host gets the dropped ones as one
gap. A store on SD or NOR flash (`ww_stream`'s `LogStore`) keeps the logs across reboots and power cuts with nothing
changed in the API.

### Decoding on the host

`defmt-decoder` does the decoding: `Table::parse(&elf)` for the table, a stream decoder fed with the bytes of each
item, frames out of it until it needs more bytes. `examples/defmt_log` is a complete host program: it reads
`DEFMT`, refuses an ELF whose build id differs from the device's (`--force` to use it anyway), resumes the session
and prints decoded frames and gaps, something like:

```text
INFO  heartbeat 41
--- 312 log messages lost (Device) ---
INFO  flood 189 of 500
```

### The build id

The linker writes a GNU build id when asked (`-C link-arg=--build-id=sha1`, a hash of the linked output: it
changes with every byte of code); a section in `memory.x` puts it into flash where the firmware reads it at run
time:

```text
SECTIONS {
    .note.gnu.build-id : ALIGN(4)
    {
        __build_id_start = .;
        KEEP(*(.note.gnu.build-id));
        __build_id_end = .;
    } > FLASH
} INSERT AFTER .rodata;
```

`ww_firmware_info::build_id()` (its `build-id` feature) reads the id between the two symbols. `examples_mcu/rp2`
does all three steps (`build.rs`, `memory.x`, `src/bin/usb_log.rs`). Without the note, `build_id` is empty and a
host can only guess the ELF.

## The device side: `ww_defmt_logger`

`defmt` allows one global logger, called from any context, interrupts included, and it can't wait; a reliable
stream's store is async. `ww_defmt_logger` sits between them:

1. `global_logger!(LOG, 4096)` makes a `DefmtLogger` with a ring of 4 KiB the `#[defmt::global_logger]`. A `defmt`
   macro takes a critical section (as `defmt-rtt` does), encodes the frame into the ring and, with the `rtt`
   feature, into an RTT up channel. The whole frame is committed at once; a frame that doesn't fit the free space
   (or is longer than 512 bytes) is dropped whole and counted.
2. `LOG.pump(&mut stream)` in the event loop takes the committed frames out of the ring, without a critical section,
   packs up to 512 bytes of whole frames into one `DefmtFrames` item and pushes it into the store, `units` =
   frames. Frames the ring dropped become the stream's `lost` exactly where they were dropped.

```rust
ww_defmt_logger::global_logger!(LOG, 4096);

// at boot
let channels = rtt_init! { up: { 0: { size: 1024, mode: NoBlockSkip, name: "defmt" } } };
LOG.set_rtt_channel(channels.up.0);       // probe-rs and `cnt run` keep showing the logs
state.log_stream.start_session(1).await?; // keep logs until a host resumes

// the event loop
loop {
    if let Either::First(ready) = select(server.wait(), Timer::after_millis(5)).await
        && let Some(LinkEvent::Down(_)) = server.handle(ready, state).await
    {
        state.log_stream.detach();
    }
    LOG.pump(&mut state.log_stream).await?;
    if server.is_up() {
        state.poll_streams(&mut server.sink(), now_ms, &mut buf, 2048).await?;
    }
}
```

* **RAM**: the ring (a power of two; each frame takes its encoded length plus 6 bytes) and about 1 KiB for packing
  an item. The ring only has to hold what is logged between two pumps; the store holds the rest.
* **Encoding**: `ww_defmt_logger::encoding()` tells which one `defmt` is built with, for `DEFMT`. With rzCOBS the
  very first frame after boot starts with an extra `0x00` (defmt's resync marker), which decoders skip.
* **Contexts**: frames from an interrupt never interleave with one being written. A `defmt` macro inside a `defmt`
  macro panics, as with `defmt-rtt`. `defmt::flush()` doesn't wait, the ring is emptied by `pump` only.
* **Other global loggers**: none may be linked next to it: not `defmt-rtt`, not `rtt-target`'s `defmt` feature.
* **Async server**: a reliable stream needs `use_async = true`.

The ring is tested on the host with four threads logging into a 512-byte ring while another pumps: every frame
arrives whole and in order, and received plus lost is exactly what was logged. `examples_mcu/rp2`'s `rp2_usb_log`
logs over USB and RTT at once (`just run rp2 usb_log`, host: `examples/defmt_log`, `--flood 500` to overload it).

## `LogText`

For firmware without `defmt`: the `log` crate on embassy or std targets, ESP-IDF.

```rust
#[ww_trait]
pub trait LogText {
    #[stream(LogRecord<'i>, reliable)]
    fn records() -> Result<(), LogError<'i>>;
    property!(rw level: LevelFilter, LogError<'i>);
}

pub struct LogRecord<'i> {
    pub severity: Severity,               // Error, Warn, Info, Debug, Trace
    pub target: MaxLen<&'i str, 48>,      // module path
    pub message: MaxLen<&'i str, 256>,
    pub truncated: bool,                  // the message was cut at the device's limit
}
pub enum LevelFilter { Off, Error, Warn, Info, Debug, Trace }
```

* `records` - one record per message, unit: record. The head's `t` carries the device time of the record. Logs from
  before the host connects come the same way as with `LogDefmt`, by resuming.
* `level` - records less severe than this are not produced (`LevelFilter::allows(severity)`); filters compiled into
  the firmware still apply. Writing it may fail with `Common(Unsupported)` when the level is fixed at compile time.

`Severity` and `LevelFilter` are complete as they are (`no_future_version`): the five levels of `log`, `tracing`
and `defmt`.

## Errors

`LogError` has only `Common(CommonError)` for now ([common errors](ww_error.md)): `Busy` when another host has the
log open, `Unsupported` when the device logs elsewhere only. It stays an enum so variants can be added.
