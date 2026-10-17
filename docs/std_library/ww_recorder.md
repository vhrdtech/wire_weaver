---
level: advanced
---

# ww_recorder

`ww_recorder` (STD-34) controls a **recorder**: a device that logs many tracks (streams of any type) to an SD card or
flash on its own, with sessions, recordings and markers, and shows the log over USB or reads it back over
WireWeaver. The first user is `ww_logring` (project P2628, repo `logring`), but nothing in the API is specific to
it: any recorder with a card, tracks and a ring can serve it.

The API carries **control, not data**. Every track is a stream of some `#[ww_trait]` in the device's own API (a CAN
frame stream, a UART monitor, a temperature), so a host reads logged data back with that stream's own reliable
protocol (backfill from the store, see [streams](../api/streams.md)), and a PC tool decodes a card from the API
bundle stored on it. `Recorder` is what you call to see the state of the card, format it, label spans and moments,
and tune how it writes.

```rust
#[ww_trait]
pub trait Recorder {
    ww_impl!(track[]: Track);
    fn status() -> Status<'i>;
    fn card() -> Option<CardFacts<'i>>;
    fn prepare_format(action: FormatAction) -> Result<FormatOffer<'i>, RecorderError<'i>>;
    fn format(token: u32, rw_mb: Option<u32>) -> Result<(), RecorderError<'i>>;
    fn recording_start(trigger: Trigger, pre_trigger_s: u16) -> Result<u32, RecorderError<'i>>;
    fn recording_stop() -> Result<u32, RecorderError<'i>>;
    fn recordings(after: u32) -> Result<MaxLen<RefVec<'i, RecordingInfo>, 8>, RecorderError<'i>>;
    fn marker(kind: MarkerKind, label: MaxLen<&'i str, 64>, value: Option<i64>) -> Result<(), RecorderError<'i>>;
    fn sync() -> Result<(), RecorderError<'i>>;
    property!(rw settings: Settings, RecorderError<'i>);
    fn counters() -> RecorderCounters;
    #[optional]
    fn refresh_usb() -> Result<(), RecorderError<'i>>;
}

#[ww_trait]
pub trait Track {
    fn info() -> TrackInfo<'i>;
    property!(rw enabled: bool, RecorderError<'i>);
    property!(rw durability: Durability, RecorderError<'i>);
    #[optional]
    property!(rw compression: Compression, RecorderError<'i>);
    fn counters() -> TrackCounters;
}
```

## Status

`status()` answers what the recorder is doing:

| `State` | Meaning | What a UI offers |
|---|---|---|
| `NoCard` | nothing in the slot | "insert a card" |
| `NeedsFormat` | a card without a log (new, or from another system) | "format?" with [`prepare_format`](#formatting) |
| `Unbound { reason }` | the card has a ring file, but it is not where the device put it (moved, deleted, replaced by a PC, other card); logging stays off and nothing is overwritten | re-create the ring file (keeps the read-write files) or format |
| `Formatting { percent }` | a card check or format is running | progress |
| `Logging` | everything works | |
| `Full` | the ring is full and the full policy is `DropNewest`: new items are refused and counted | download, then format or switch the policy |
| `Failed { reason }` | a write error, or the card was removed while logging | check the card |

`Status` also has the session of this boot (counts up for the life of the card), the running recording, the device
time of the newest record, and how much of the ring holds data. `State` is an `Unsized` enum, a later version can add
a state without breaking older hosts.

`card()` is `None` without a card, else `CardFacts`: kind (SD, eMMC, NOR), capacity, product name, manufacturer and
serial from the CID, and what the card reports about its own geometry: allocation unit, the recording unit of its
speed class, speed class, UHS grade and video class (`0` where the card reports nothing). It also says what is on the
card (`CardContent`: unrecognised, other files, a ring, an unbound ring) and the result of the last card check
(`CardCheck`).

## Formatting

A format erases a card, so it takes two steps and the second needs what the first returned:

1. `prepare_format(action)` reads the card and writes nothing. It answers a `FormatOffer`: a one-use `token`, how long
   it stays valid, the card, how many bytes would be erased, whether the card holds ring data or files in its
   read-write region (a UI warns on these), the default and the limits of the read-write region in MB, and the ring
   size that gives. `action` is `Format` (everything) or `RecreateRing` (after `Unbound`, keeps the read-write files).
2. `format(token, rw_mb)` carries it out. The token works once, for the card the offer was made for, and expires.
   `rw_mb` of `None` is the offer's default and is ignored for `RecreateRing`. The call stops logging, runs the
   **card check**, then writes the layout; progress is `State::Formatting` in `status()`.

The card check is why the second step is separate: it overwrites the card. It tests that the card really has the
capacity it claims (a fake card wraps around when written past its true size), measures sustained sequential write
speed and the longest stall, and sweeps write sizes to find the best flush size for this card. The result is
`CardCheck { aliasing_ok, write_kib_s, worst_stall_ms, best_flush_bytes, rating }` where the `Rating` compares the
card with the configured data rate: `Good`, `Marginal` (keeps up on average, but its worst stall is longer than the
RAM buffer holds: drops are possible), `TooSlow` or `Unchecked`. A card that fails the aliasing test is refused with
`CardFailed` before anything is formatted.

A device with no UI can start the same flow from a button (long press) and call the same handlers itself.

## Recordings and markers

A **recording** is a labelled span of the log with an id that counts up for the life of the card. It is started by a
`Trigger` (`Api`: this call; `Button`; more kinds are planned) and stopped by a call, the button or a duration.
`recording_start(trigger, pre_trigger_s)` returns the id. In the default `RecordingMode::Continuous` everything is
logged all the time, so a recording only labels a span and `pre_trigger_s` moves its start back by that many seconds
for free; in `Triggered` mode tracks are written only inside recordings, and a pre-trigger needs a RAM ring per
track (`Unsupported` on a device that has none). `recordings(after)` lists them oldest first, eight per page, with
their trigger, device-time start and length, how they ended (`StopReason`) and how many bytes of the ring they
cover. Recordings whose data the ring already overwrote are not listed. A recording survives a reboot; a power cut
in the middle continues it.

A **marker** is a moment to find again: `marker(kind, label, value)` writes a record into the system track (track 0)
at once, not with the next flush. `MarkerKind` is `Api`, `Button`, `Trigger` or `User { code }` (the application's
own, stored as a byte `>= 3`, see `MarkerKind::code`).

`sync()` writes what is in RAM to the card now and answers when it is there: before a planned power-off, or from a
brown-out warning.

## Settings and tracks

`settings` is one property, `Settings`:

| Field | Default | Takes effect |
|---|---|---|
| `flush_bytes` | 64 KB, or the card check's best | at once; a multiple of 512 and of the card's recording unit, at most a segment |
| `flush_interval_ms` | 1000 | at once; the most a power cut takes when data comes slowly |
| `full` | `DropOldest` (a logger); `DropNewest` stops when full | next boot |
| `mode` | `Continuous`; or `Triggered` | next boot |
| `usb_view` | split by recording; `by_time`, `by_track`, `csv` add folders and files | next refresh or mount |

A write outside the limits answers `Common(InvalidArgument)`. Larger flushes are faster on most cards and lose more in
a power cut: nothing written before the last completed flush is lost, and the next mount counts what was in RAM.

`track[]` is the array of the session's tracks, indexed by track number: **0 is the system track** (markers,
recordings, drops, card events), the data tracks follow from 1. `valid_indices_root_track` returns those, and they
change only on a boot. Each `Track` has:

- `info()`: the stream's resource path in the device API with indices (`can[1].rx`; the system track is `system`),
  the item type, what a position counts (`bytes`, `frames`), a stable 64-bit key (the same track across sessions
  and firmware that keeps the path) and the nominal rate;
- `enabled` (a disabled track keeps its place and position and counts its items as dropped; track 0 can't be
  disabled), `durability` (`Normal`: with the next flush; `Immediate`: written right after each item, for events and
  errors) and the optional `compression` (`None`, `Lz4`, `Heatshrink`);
- `counters()`: records and item bytes stored this boot, where the stream continues (`next`, counts across boots),
  up to where the track is on the card (`durable`), up to where the host released it, and units dropped.

## Counters

`counters()` is for a status screen and for deciding whether a card is good enough: bytes written since the format,
data segments reused and ring size (their ratio is the overwritten span), torn blocks and records found by mounts,
units dropped in all tracks, card stalls over the threshold with the longest, card write errors, the number of
sessions, and the device time of the oldest data still in the ring. A card that falls behind shows as counted
drops and card events in track 0, never as silent loss. Numbers are variable length: the counters of an idle track
fit in 8 bytes (a test checks it).

## USB view

`refresh_usb()` (optional) tells the USB mass storage view to show the log as it is now. The view is a snapshot taken
when the host mounts the virtual volume, so new data shows only after a re-mount; this raises a media change
instead. A device without a USB view leaves the call out.

## Serving it

Generate handler stubs with `ww api scaffold` (AGENTS.md describes the flow) for the same options as your
`ww_codegen!` call:

```sh
cargo ww api scaffold ww_stdlib/ww_recorder --name Recorder
```

It prints the server struct, a stub for every handler (`status`, `card`, `prepare_format`, ..., `track_info`,
`track_counters`, `valid_indices_root_track`, the `settings` getter and setter) and the matching `ww_codegen!`; the
output compiles as it is. A device that has no USB view or codec leaves out `refresh_usb` and `track[].compression`
with `omit = [..]`.

## Evolution

No type uses `sized` or `final_structure`, so fields can be appended with `#[default = ..]` and variants added
without breaking older hosts; an enum reads a newer variant as `Unknown(u32)`. Adding a method
or an optional item is wire compatible. `ww_recorder` is versioned like any API crate (its version is its
identity, see [crate layers](../evolution/crate_layers.md)).
