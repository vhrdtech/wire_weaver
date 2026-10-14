# Stream stores

A reliable stream ([Stream delivery](stream_reliability.md)) keeps every event it sends until the host acknowledges
it, so it can send it again after a loss and send the backlog after a reconnect. Where these events are kept is a
**store**, a `ww_stream::store::StreamStore` the device picks for each stream. The API doesn't change with the store: the same stream can keep 256 KB in RAM on
one device and 32 GB on an SD card on another.

| Store | Where | Survives | Feature |
|---|---|---|---|
| `RamRing<N>` | `N` bytes of RAM | host reconnects | always there |
| `LogStore<M, BUF>` | SD card, NOR flash | reboots and power cuts | `log-store` (+ `sdmmc` / `nor-flash`) |

Both keep events as they go on the wire, head included, so a resent event is bit-identical: its timestamp is the
device's time when the data was made, not when it was sent.

When a store is full, it does what its `FullPolicy` says:

- **`DropNewest`** (default): the new event is refused (`PushError::Full`), the sender counts its units as lost and
  reports them in the next event's `lost`. What is kept stays contiguous.
- **`DropOldest`**: the oldest events are forgotten to make room, a ring of the most recent data. Their units are
  counted in `StoreState::dropped_units`.

Either way the host learns exactly how much is missing and where.

## RAM: `RamRing`

```rust
use ww_stream::store::{FullPolicy, RamRing};

static SAMPLES: StaticCell<RamRing<{ 256 * 1024 }>> = StaticCell::new();
let store = SAMPLES.init(RamRing::new().with_policy(FullPolicy::DropOldest));
```

Each event takes its item, its encoded head and 21 bytes (`RECORD_OVERHEAD`).

## SD card or flash: `LogStore`

`LogStore` is a log on a block device that keeps a stream's data across reboots and power cuts: a field logger keeps
sampling to its SD card while no host is there, reboots, and the host still gets everything when it connects.

```toml
ww_stream = { version = "0.5", features = ["sdmmc"] }      # or "nor-flash"
```

```rust
use ww_stream::store::log::{LogConfig, LogStore, SdMedium};
use ww_stream::store::FullPolicy;
use ww_stream::tx::Reliable;

// blocks 2048.. of the card are the log's, the rest is free for a FAT partition
let medium = SdMedium::new(sd_card, 2048, num_blocks - 2048);
let config = LogConfig { segment_size: 64 * 1024, policy: FullPolicy::DropNewest };
// session 1 is used only when the card holds no log yet
let store: LogStore<_, 512> = LogStore::mount(medium, config, 1).await?;
let mut samples = Reliable::new(store);
// a log from before the reboot: continue its session and positions
samples.continue_stored_session();
```

On NOR flash, wrap an `embedded-storage-async` `NorFlash` (a QSPI chip, or a partition of the MCU's own flash) in
`NorMedium(flash)`; segments are then a multiple of the flash's erase size. Any other storage can implement
`LogMedium`: its write and erase unit sizes, whether it needs erasing before a write, read, write and erase.

### What it keeps

- **Positions and session**: after a reboot `state()` gives back the session and `next`, the position the stream
  continues from, so the host sees one session across the reboot, not a new stream.
- **Released position**: what the host acknowledged is freed. It is saved now and then, so after a reboot a few
  acknowledged events may be sent again; the host drops them as duplicates. Events not acknowledged are never lost.
- **Clock epochs**: the device's clock restarts with a reboot, so each mount starts a new clock epoch
  (`clock_epoch()`), and the first event pushed after a mount carries it in `HeadExt::clock_epoch`, unless the
  sender set it already. The host maps each epoch to real time, or shows "before reboot N".

### Power cuts

A push writes its record before it returns. If the power fails during a push, at most that record is lost: on the
next mount the store finds the end of the log and counts a record it finds cut short in
`recovery().torn_records`. Positions continue after the last whole record; the cut one was never sent, so the host
never saw it.

This holds for a power cut at any write or erase, which the tests check one by one on a simulated SD card and NOR
flash (`ww_stream/src/tests/log_store.rs`). On an SD card it relies on a 512-byte block write either happening or
not, as SD cards do; on NOR flash nothing is assumed about a cut write.

### Sizing

The log is a ring of **segments** of `segment_size` bytes, after two small superblock slots at the start:

- A segment is the unit of room: it is freed once every event in it is acknowledged (or dropped), and the rest of a
  segment stays empty when the next event doesn't fit, and on every reboot (a new segment starts the new epoch).
  Make segments several times the biggest event.
- Use many segments: a mount reads the superblock, at most a few segment headers and the last segment, and a read
  finds its segment by a binary search over the headers, so a big card mounts as fast as a small one. 64 KiB
  segments on a 32 GB card give half a million.
- `BUF` is the size of the two buffers the store keeps, a multiple of the medium's write size: 512 for an SD card.

Each event takes its item, its encoded head (see `StreamHead`) and a 12-byte record header, rounded up to 4 bytes on
an SD card or to the write size on NOR flash.

`LogStore::mount` returns `LogError::Geometry` when the medium holds a log with another segment size or medium size;
`LogStore::format` starts a new, empty log on it.

### Testing with a simulated medium

With the `std` feature, `SimSd` and `SimNor` are an SD card and a NOR flash in RAM. `cut_power_at(n)` makes the
`n`-th write or erase the one the power fails in (it is torn, every later call fails), and `image()` /
`from_image()` carry the content over a reboot, so a device's own power-cut tests run on the host.
