# Stream stores

A reliable stream ([Stream delivery](stream_reliability.md)) keeps every event it sends until the host acknowledges
it, so it can send it again after a loss and send the backlog after a reconnect. Where these events are kept is a
**store**, a `ww_stream::store::StreamStore` the device picks for each stream: it is the `S` of the stream's
`ww_stream::tx::Reliable<S>` state ([Reliable streams](streams.md#reliable-streams)). The API doesn't change with the
store: the same stream can keep 256 KB in RAM on one device and 32 GB on an SD card on another.

| Store | Where | Survives | Feature |
|---|---|---|---|
| `RamRing<N>` | `N` bytes of RAM | host reconnects | always there |
| `PersistentRamRing<N>` | `N` bytes of `.uninit` RAM | soft resets (fault → reset), not power cuts | `persistent-ram` |
| `LogStore<M, BUF>` | SD card, NOR flash | reboots and power cuts | `log-store` (+ `sdmmc` / `nor-flash`) |

All keep events as they go on the wire, head included, so a resent event is bit-identical: its timestamp is the
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

## RAM across a soft reset: `PersistentRamRing`

A `RamRing` whose bytes sit in RAM the startup code leaves alone, so they are still there after a soft reset: a
HardFault or panic handler that resets the chip (`SCB::sys_reset`) doesn't take the last logs before the crash with
it. The next boot takes the events up again and the host gets them when it connects. A power cut loses them; RAM
after a power-on holds garbage, which is never taken for events.

```toml
ww_stream = { version = "0.5", features = ["persistent-ram"] }
```

```rust
use core::mem::MaybeUninit;
use ww_stream::store::{FullPolicy, PersistentRamRing, RamArea};
use ww_stream::tx::Reliable;

// cortex-m-rt leaves `.uninit.*` alone at boot
#[unsafe(link_section = ".uninit.ww_logs")]
static mut LOG_AREA: MaybeUninit<RamArea<4096>> = MaybeUninit::uninit();

// once, at boot
let area = unsafe { RamArea::from_uninit(&mut *core::ptr::addr_of_mut!(LOG_AREA)) };
let store = PersistentRamRing::attach(area).with_policy(FullPolicy::DropOldest);
// `store.recovery().formatted`: nothing was kept (a power-on), a new ring was started
let mut logs = Reliable::new(store);
// the ring from before the reset: continue its session and positions
logs.continue_stored_session();
```

Where the area goes is the firmware's business, and differs per chip: it must be RAM the startup code neither zeroes
nor copies into (`.bss`, `.data`), nor a firmware's own RAM init (a loop zeroing a whole SRAM bank at boot takes the
area with it). With `cortex-m-rt`, a `.uninit.*` section is enough. The same area must not be used for anything
else, and only one store may be attached to it.

### What it keeps

The same as a `LogStore` across a reboot: session, positions, the released position, `dropped_units`, and a new
**clock epoch** per boot (`clock_epoch()`, 0 for a new ring), which the first event pushed after it carries in
`HeadExt::clock_epoch` unless the sender set it already. `state().persistent` is true.

### How it checks the RAM

The area holds two 64-byte header slots and the ring. After every change (push, release, reset) the ring's header
is written to the slot not written last, with a magic, the layout version, `N`, a sequence number and a CRC-32C; at
boot, the good slot with the higher sequence number wins. A record goes into free room first, and old records a push
drops to make room are forgotten by a slot written before the new record goes over them, so a reset at any byte
leaves the state before or after the change, never a mix. Every record has its own CRC-32C, checked at boot too: if
something wrote over the area before the reset, the ring is cut before the first bad record
(`recovery().truncated_bytes`) and the stream continues after it.

The host tests (`ww_stream/src/tests/persistent_ram.rs`) carry the area's image over a simulated reset
(`RamArea::image()`, `load_image()`), fault a push, release and reset after every byte written, and attach to random
garbage, flipped header bits and records written over.

### Sizing

Each event takes its item, its encoded head and 25 bytes (`PERSISTENT_RECORD_OVERHEAD`); the area is `N` bytes and
128 for the slots (`RamArea::<N>::SIZE`). Every change also writes a 64-byte slot; a push computes two CRCs
(over the slot and over the record).

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

## Other stores

Any storage can hold a reliable stream by implementing `StreamStore` (for a block device, implementing `LogMedium`
for `LogStore` is usually less work):

```rust
pub trait StreamStore {
    type Error;
    /// Append an event, head and item bytes as they go on the wire.
    async fn push(&mut self, head: &StreamHead, item: &[u8]) -> Result<(), PushError<Self::Error>>;
    /// Copy the event at `pos`, or the first one after it, into `buf`.
    async fn read<'b>(&mut self, pos: u64, buf: &'b mut [u8]) -> Result<Option<StoredEvent<'b>>, Self::Error>;
    /// Events before `pos` are acknowledged and may go.
    async fn release(&mut self, pos: u64) -> Result<(), Self::Error>;
    /// Session, oldest and newest kept positions, next position, bytes used and capacity, persistent or not.
    fn state(&self) -> StoreState;
    /// A new session: drop everything, positions from 0.
    async fn reset(&mut self, session: u32) -> Result<(), Self::Error>;
}
```

A full store either refuses the push with `PushError::Full` (drop newest) or forgets its oldest events and moves
`state().oldest` on, counting their units in `dropped_units` (drop oldest).
