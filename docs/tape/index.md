---
level: advanced
---

# Tapes

A **tape** is how WireWeaver records data over time: what a device streamed, what crossed a link, what a program
computed from either. The `ww_tape` crate writes and reads it. One format serves a live feed (blocks handed to a
consumer as they are made), a file on a PC, and a raw medium such as an SD card in a data logger. This chapter explains
the format for readers who have not seen its code: what a file holds, how time is stored, and what survives when
something goes wrong.

The format has four goals, and most of its choices follow from them:

- **Typed data.** Every item is a WireWeaver type, and the file carries the API bundle that defines it. A reader on
  another PC, years later, knows what the bytes mean without the device.
- **Exact time.** Times are integer ticks of a named clock with an exact rate. Nothing is rounded to whole nanoseconds,
  and a file holds tracks on many clocks.
- **Reads in place.** Large tracks are stored in columns that a reader queries as they lie in the file, without decoding
  them into a list first.
- **Damage stays local.** A crash, a torn write or a flipped bit costs the bytes it touched, and a reader says what was
  lost.

The link tap is one producer of tapes; a device log, a simulator or an analysis program writes
the same blocks. Nothing in the format knows any producer.

The chapter has three pages. This one covers records, clocks and the anatomy of a file. [Events and
series](events_and_series.md) covers how large tracks are stored: event chunks, the time codec, series and summaries.
[Damage and recovery](recovery.md) covers checksums, crashes, raw media and the limits of the format. The exact byte
layouts are in the module docs of `ww_tape`, linked from each section; this chapter explains them and does not repeat
them.

## Records, tracks and clocks

A **record** is one item on one track at one time. A **track** is a named sequence of items of one type: the frames a
link received, the temperature readings of a sensor, the log lines of a firmware. A track's definition (`TrackDef`)
gives it a small number used inside the file, a stable key that names the same track across files, the WireWeaver
resource path it comes from, and the clock its times count.

A **clock** is anything that counts time: the host's nanosecond clock, a device's microsecond timer since boot, an audio
codec's sample clock. Its definition (`ClockDef`) gives an exact rate in ticks per second, an exact resolution, and an
**origin**: what tick 0 is. The origin is an identity, never a number. It may be a device's boot, the Unix epoch, the
start of a media file, the start of the producer's run, or nothing known (a free-running counter).

A **tick** is one step of a clock, and every record time is a signed 64-bit count of ticks. Rates are exact fractions,
so clocks that are not whole numbers of hertz stay exact. A video at 60000/1001 frames per second lives on its stream's
60 000 Hz time base, one frame every 1001 ticks, and every frame time is exact.

### Clocks that don't share a time base

Two clocks are related only by a **clock map**: a list of map segments, each saying `b = b0 + (a - a0) · slope` over a
range of clock `a`, with an error bound in ticks of clock `b`. The slope is a reduced fraction, so a fitted drift such
as 1000000007/999999931 stays exact. A map must increase strictly across all its segments, or the order of items would
flip between the clocks. A forward jump of the wall clock (a step from a time server) starts a new map segment; a
backward step is a gap or a new clock.

Wall time is one more clock, with the Unix epoch as its origin. No block carries a wall time of its own. A producer that
knows the wall time at its start writes a map from its clock to the wall clock; one that estimates drift over time
writes the map as a track of its own, whose items are map segments.

A device restart is a new clock: the same counter, with the boot count as part of its key. The two boots are related
only by an estimated map, because nothing guarantees how long the device was off.

### Epochs: long captures at fine ticks

Signed 64-bit ticks cover about 292 years at 1 ns, but only about 106 days at 1 ps. A capture that needs more is cut
into **clock epochs**: each epoch is a new clock of the same counter, and an exact map with slope 1 and error 0 says
that tick 0 of the new epoch is tick `b0` of the previous one. A reader joins the epochs into one long coordinate; the
stored ticks stay 64-bit.

### Example: 48 kHz audio on a 1 ns clock

A recorder stamps everything on the host's nanosecond clock and records a 48 kHz audio stream. One sample lasts 1/48000
s, which is 20833⅓ ns: not a whole number of ticks. The track stores its period as the exact fraction 62500/3 ticks per
sample, and sample `i` sits at tick `start + ⌊i · 62500/3⌋`:

| Sample | Exact tick | Stored tick |
|---:|---:|---:|
| 0 | 0 | 0 |
| 1 | 20833.33… | 20833 |
| 2 | 41666.66… | 41666 |
| 3 | 62500 | 62500 |
| 172 800 000 (one hour) | 3 600 000 000 000 | 3 600 000 000 000 |

Each sample's tick is rounded down on its own, so the rounding never adds up: after an hour the samples sit exactly one
hour after the first. A period rounded to 20833 ns would drift by 57.6 ms per hour. The same recorder could also put the
audio on a 48 kHz clock of its own and relate it to the host clock by a map; that is the right choice when the codec's
crystal drifts against the host.

## Blocks

A tape is a sequence of **blocks**. Every block has a 32-byte header, a body and a CRC-32C checksum over everything
before it. The header starts with the magic `WWTP` and gives the block's version, its kind, its total length, a count
(records, items or entries, by kind), a sequence number and a run tag. A file is blocks appended back to back, never
padded between them; a live feed sends one block per message.

The header and the trailing CRC are the same in every version of the format. A reader can therefore check any block and
skip it by its length, even a block of a kind or version it doesn't know. A later version changes only the bodies, and
an old reader takes a block it doesn't understand as one it skips, not as damage.

The **sequence number** counts a writer's blocks from 0, so a gap in it says how many blocks were lost. The **run tag**
names one writer's unbroken append: a producer's start, a device's boot. The run tag is how a reader tells current data
from stale data left on a medium by an earlier recording; [Damage and recovery](recovery.md#runs-and-stale-data)
explains it.

The block kinds fall into four groups:

| Group | Kinds | Holds |
|---|---|---|
| Description | describe | what the tape is: the producer, clocks, maps, tracks, API bundles |
| Data | records, events chunk, series chunk | the items |
| Overview | summary | coarse views of long tracks, rebuilt from the data when lost |
| Structure | piece head, index, finish, tail | where things are, how the file ends, how files chain |

The byte layout of the header and of record blocks is in the [`ww_tape` crate
docs](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/lib.rs).

### Describe blocks

A **describe set** is one or more describe blocks that together say everything a reader needs before the data: the
producer, how blocks are placed (the layout), the API bundles, every clock, clock map and track definition in use, and
typed items such as a run record. After those come the newest records of every **sticky** track: state a reader joining
late needs, such as a link's session or a series' current timing.

The set is written in a canonical order, so the same input gives the same bytes in whatever order the producer added its
tracks. A producer writes a describe set at the start, when it adds a track, when a live consumer joins, and after
blocks were lost. It never repeats one on a timer, so a slow trickle of records costs a block header each, not a
description each. Its first and last blocks carry flags, so a reader can tell a whole set from part of one.

The writer also writes the newest set again as a **copy**: right after the first data block, then about every 128 MiB,
and once more before the end of the file. A copy carries a flag and adds nothing for a reader that read the original; it
keeps a file readable when its first set is damaged. The layout is in the [describe module
docs](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/describe.rs).

### Record blocks

A **record block** holds records of any tracks back to back. Each record is a head (the track number and which of the
block's clocks it counts), a time delta, the item's length and the item's bytes, all as variable-length integers. The
delta is against the previous record on the same clock in the same block, and the first one is absolute, so every block
reads alone.

Record blocks suit tracks that produce a few items at a time, where waiting to fill a chunk would delay a live reader.
Large tracks go into chunks, which the [next page](events_and_series.md) describes.

## Anatomy of a file

A complete file is a **piece**: a piece head, a describe set, the data with periodic index blocks between it, and at the
end a final index, a backup of the describe set, a finish block and a tail. The diagram shows one piece, start at the
top.

```
 piece start ─► ┌─────────────────────────────┐
                │ piece head                  │ ids, the previous piece's hash
                ├─────────────────────────────┤
                │ describe set                │ producer, layout, clocks, maps,
                │                             │ tracks, bundles, sticky records
                ├─────────────────────────────┤
                │ events chunk   (track 3)    │ first data block
                │ describe copy               │ the set again, flagged as a copy
                │ records        (tracks 1,2) │ small tracks, many per block
                │ series chunk   (track 5)    │
                │ summary        (track 3)    │
                │ ...                         │
                │ index (checkpoint) ◄──┐     │ every 4 MiB: the blocks placed
                │ ...                   │     │ since the previous checkpoint,
                │ index (checkpoint) ───┘     │ and a pointer back to it
                │ ...                         │
                │ describe copy               │ about every 128 MiB
                │ ...                         │
                ├─────────────────────────────┤
                │ final index    ◄──────┐     │ every block again, by track
                │ describe backup       │     │
                │ finish         ◄──┐   │     │ BLAKE3 hash of all bytes before it
                │ tail (164 B) ─────┴───┘     │ piece length, where the finish is
   end of file ─► └─────────────────────────────┘
```

A reader of a finished file starts at the end. It reads the last 164 bytes, the **tail**, which give the piece's length
and where its finish block is. The **finish** block holds a BLAKE3 hash of every byte before it, per-track totals
(items, ticks), and the offsets of the final index and the describe backup. The **final index** lists every block of the
piece sorted by track, with a directory in its head, so a reader of one track out of thousands reads that directory and
that track's list.

While the file is written there is no tail. The writer places a **checkpoint** index block about every 4 MiB, listing
every block placed since the previous checkpoint and pointing back at it. A reader of an unfinished file finds the
newest checkpoint from the end, follows the chain back, and scans only the stretch after the newest one. All offsets
count from the start of the piece, so an index stays valid wherever the piece sits. The layouts are in the module docs
of [index](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/index/mod.rs) and
[piece](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/piece/mod.rs).

A file that a writer started without a piece head (a plain stream of blocks) is still a valid tape; it is one piece with
no ids.

### Pieces and chains

A long recording can be cut into several pieces, each a complete file that reads alone. A writer cuts only between
chunks, by size, by chunk count or by age, and names the files `00000000.wwt`, `00000001.wwt` and so on. Bounded files
make retention simple (delete the oldest piece), and a crash costs only the piece being written.

The pieces of one recording form a **chain**, checked in both directions. Each piece head names the previous piece by
the content hash in its finish block. The tail cannot name the next piece's hash, since that piece isn't written yet, so
it names the next piece's id instead, derived from its own id and hash at the cut. The next head repeats that id. Each
piece also names the recording it belongs to (the result id), its sequence number in the chain, and for each track where
its items continue.

Pieces written back to back are one valid tape: `cat 00000000.wwt 00000001.wwt` gives a file any reader takes. A reader
finds the last piece's tail at the end, the piece's start from it, and the previous piece's tail just before that start,
so it walks back through concatenated pieces with one small read each. A chain whose first pieces were deleted reads as
"head cut", not as damage.

### Reading by position

`TapeReader` reads a tape through any byte source that reads at an offset: a slice in memory, a file, or an HTTP
resource fetched by range requests. It needs no file system and no threads, so it runs in a browser. A finished piece is
opened with a few small reads (tail, finish, the final index's directory) and then only the blocks a query needs.

## Same input, same bytes

A writer given the same input writes the same bytes. Blocks hold no wall time, describe records go in canonical order,
blocks are cut by size rather than by time, and every codec is integer-only or uses arithmetic that every platform
rounds the same way. A producer that seals blocks on a timer, or cuts pieces by age, gives up this property for the
blocks it seals that way.

One exception: an lz4-coded payload column is byte-identical only for one version of the lz4 library on targets of the
same pointer width and byte order. Decoding is unaffected.

Deterministic output makes files comparable by hash, lets a test pin a file's exact bytes, and lets two machines that
recorded the same feed confirm they hold the same data.

## See also

- [The link tap](../transport/tap.md): the producer that records a WireWeaver link into tapes, and a reference to
  every part of the format.
