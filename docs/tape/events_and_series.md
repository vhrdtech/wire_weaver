---
level: advanced
---

# Events and series

Record blocks store each item with its own time delta and length, which suits a slow trickle of items. A track with
millions of items needs something else: a form a reader can search and decode without walking every item before the one
it wants. `ww_tape` stores such tracks in **chunks**, one track per chunk, coded column by column. This page explains
the two kinds of chunk, how they store time, and the summaries that let a viewer draw a long track without reading it.
It assumes the [overview](index.md).

## Chunks

A **chunk** block holds a run of consecutive items of one track. An **events chunk** holds up to a few thousand items of
an event track, each with a start tick and optionally an end, a flags word and a payload. A **series chunk** holds
samples of a fixed-rate series, which have no time of their own.

The chunk's head places it without any other block. It names the track by its identity hash and number, and gives the
first item number, the item count, the first and last tick, and the latest end of any item. A reader that lost the
describe set and the index around a chunk still knows which track the chunk belongs to and which items and times it
holds. An overlap query ("which spans cover tick t?") uses the latest end without decoding the ends.

After the head come the **columns**: starts, ends, flags, payload lengths and payloads for events, or sample values for
a series. Each column names its kind and its **codec**, the rule its bytes follow, so a chunk of one track can use
another codec than the chunk before it. A reader that meets a column kind or codec it doesn't know refuses that column
and still reads the others.

Columns are read in place. All numbers are little-endian and need no alignment, and the payload ends with 16 zero bytes,
so a 16-byte load at any column byte stays inside the block. The bytes a reader holds in memory are the bytes in the
file: there is no decoded copy to build or cache. The layout is in the [chunk module
docs](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/chunk/mod.rs).

## Event times

An events chunk stores its start ticks in one of two **time modes**:

- **Implicit.** Item `j` starts at `first tick + j × period`, and the column holds only the period. A chunk whose items
  are evenly spaced takes this mode, and so does a chunk of one item.
- **Linear.** The items are cut into **groups** of 128. Each group stores a 25-byte record (its first tick, a slope, the
  smallest residual, a bit width and an offset), then each item's distance from the line the slope draws, bit-packed at
  the group's width.

The slope is what makes periodic data cheap. A 1 kHz event with a little jitter lies close to a line through its group's
first and last item, so each residual needs only the bits of its jitter, not the bits of the 1 ms period. The encoder
keeps the slope only when it gives a narrower width than no slope, so data without a trend loses nothing. It uses
integers only, so the same starts always give the same bytes.

A read needs no decode. Item `j` is one group record and one load. A search for the first item at or after a tick
bisects the groups' first ticks, then uses the slope to narrow the items inside the group to the few the residual width
allows. Whole groups decode with hand-written kernels for AVX-512, AVX2 and NEON, chosen at run time, beside a scalar
path that gives the same values. The layout is in the [time codec
docs](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/times/mod.rs), the bit packing in
[bitpack](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/bitpack/mod.rs).

### What it costs

The codec was chosen by measuring candidates on 10 million events per workload, at chunks of 4096 items, on a virtual
machine with 14 vCPUs of an AMD EPYC Genoa host. Bytes per event include group records and chunk overhead; raw 64-bit
ticks take 8.01.

| Workload | Linear groups | Delta-of-delta varints | Elias-Fano |
|---|---:|---:|---:|
| 1 ms period, ±300 ns jitter, 1 ns ticks | 1.48 | 1.88 | 2.79 |
| exact 1 ms period with a few gaps | 0.022 | 0.99 | 2.79 |
| bursty traffic, 1 ns ticks | 3.84 | 2.43 | 3.16 |
| a real system journal, 6.4 M entries, 1 µs ticks | 2.97 | 2.76 | 2.51 |

The bench stored the residual minimum as 8 bytes, a 29-byte group record against the format's 25, so these sizes are an
upper bound. On the same machine, fetching one item by number took 2 to 25 ns and finding the first item at or after a
tick 15 to 220 ns, faster than a binary search over raw ticks at that size (244 to 280 ns), because the packed track is
smaller.

Bursty nanosecond traffic is the one case where the linear codec loses: varints take 2.43 bytes per event against 3.84.
Varints can't be read in place, though; fetching one item cost 190 to 310 ns on the same machine because the reader must
walk to it. The format pays about 1.4 bytes per event on such data for random access.

A frame clock shows why a clock should match its source. One million video frames at 60000/1001 frames per second cost
0.010 bytes per frame on the stream's own 60 000 Hz clock (an implicit chunk, period 1001), and 0.36 bytes on a 1 ns
clock, where rounding leaves a residual of 0 or 1 ns per frame.

### Ends and flags

An event is an **instant** or a **span** with an end. Each chunk picks one of three end modes:

- **Absent:** every item is an instant, and no column is stored.
- **Next start:** each item ends where the next begins, as in back-to-back states. No column is stored; the last end is
  the chunk head's latest end.
- **Durations:** each item's `end - start`, packed per group of 128 above the group's smallest duration. The smallest
  duration plus the largest value the width holds bounds the group's longest item, so an overlap query knows how far
  back to look.

On the same bench, back-to-back spans cost 2.98 bytes per event for starts and ends together, against 16.01 raw; spans
with gaps between them 5.97.

**Flags** are an optional 32-bit word per item, with the severity in its low bits. They are packed per group beside the
OR of the group's flags, so a zoomed-out view colours a range by the ORs without decoding a single item. A group without
flags costs its record only, and a chunk where every item's flags are 0 has no column.

### Payloads

An item's payload is its WireWeaver value in shrink_wrap bytes. An events chunk stores them in two columns: the lengths,
packed per group of 128 above the group's shortest, and the payload bytes back to back. Each group record says where its
first payload starts, so item `i`'s payload is found from one record and one group of lengths. The payload bytes are
stored raw and read in place, or coded with lz4 when that is smaller and decoded once when the chunk is opened. The
layout is in the [payload codec docs](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/payloads/mod.rs).

## Series

A **series** is a track of samples at an exact period: an ADC channel, an audio stream, a motor's phase currents. It
stores no time per sample. The track's definition (`SeriesDef`) gives the type of one value as the API declares it, the
storage element (`I16`, `F32`, `ComplexI16` and so on) and channel count derived from it, the period as an exact
fraction of ticks per sample, and the summaries the writer builds.

### Fixed point first

For data a device controls, store fixed-point integers. An exact_units `Fixed<Ampere, Milli, i32>` is an `i32` on the
wire and an `i32` in the series column; the scale (10⁻³ A per step) lives once in the track, not in every sample. The
numbers stay integers from the device to the file, and nothing passes through a float.

Integer columns are coded with **DELTA**: per group of 128 samples, the differences between neighbours, minus their
smallest difference, bit-packed. A ramp or a constant packs to zero bits per sample. **DELTA2** codes differences of
differences the same way, which suits signals with a steady slope. Every group restarts, so any range of samples decodes
from its group's start.

Floats a device didn't choose (a sensor that only reports `f32`, a value from another program) are coded with **ALP**.
ALP finds floats that began as decimals, such as an ADC count times 0.1, and stores them as the integers they came from,
bit-packed per group of 128. Floats with full mantissas take ALP's second scheme, which codes their front bits through a
small dictionary. Values that don't come back bit for bit (NaN, infinities, `-0.0`) are kept whole as exceptions. The
encoder never lets a group grow by more than 7 bytes over its raw size. The layout is in the [ALP
docs](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/samples/alp/mod.rs).

Fixed point beats every float codec, on every data set measured. `just bench-sample-codecs` stores the same data as
fixed-point integers with DELTA and as floats with ALP and with the other float codecs it keeps for comparison (XOR
codecs after Gorilla and Chimp, and DELTA over the float bits); those others are bench baselines, not codecs of the
format. It ran in chunks of 4096 samples on an AMD Ryzen AI 9 HX 370. Ratios are against the raw float column, so a
fixed-point column of `f64` data shows twice the ratio of the same column against `f32`; decode times are nanoseconds
per value.

| Data | Fixed point, DELTA | ALP | Best other float codec |
|---|---|---|---|
| motor simulation, `f32` | 3.10, 0.66 ns | 1.64, 1.17 ns | 1.99, 1.02 ns |
| motor simulation, `f64` | 6.20, 0.65 ns | 2.51, 1.24 ns | 2.51, 2.60 ns |
| speech, `f32` (fixed: 16-bit PCM) | 3.43, 0.71 ns | 1.16, 1.27 ns | 1.23, 5.59 ns |
| 12-bit ADC × 3.3/4096, `f32` | 7.31, 0.65 ns | 1.59, 1.17 ns | 1.84, 0.81 ns |
| 12-bit ADC × 3.3/4096, `f64` | 14.63, 0.65 ns | 1.32, 1.59 ns | 1.57, 7.28 ns |
| 12-bit ADC × 0.1, `f32` | 7.31, 0.65 ns | 4.67, 0.81 ns | 1.84, 0.83 ns |
| 12-bit ADC × 0.1, `f64` | 14.63, 0.65 ns | 9.02, 0.72 ns | 1.59, 7.27 ns |

Fixed point is 1.6 to 9.3 times smaller than the best float codec on each set, and decodes fastest. ALP wins among the
float codecs only where the floats began as short decimals (ADC × 0.1: 9.02 against 1.59 for `f64`). Elsewhere it is
level with the best of them or behind by up to 18 %, and it decodes at 0.72 to 1.59 ns per value, where the XOR codecs
take up to 7.3 ns.

A fixed-point source hands the writer the integers that ALP has to rediscover from the floats, and ALP rediscovers them
only when the scale is a short decimal: 0.1 is, 3.3/4096 is not, and then nothing simpler than the float bits is left.
Store the ADC counts with the scale in the type, and no float codec is needed. To rerun the comparison, run `just
bench-sample-codecs` in the repository; it fetches the speech sample once and prints the full table in about 20 s.

### Raw device formats

Some devices send samples in their own packing: 12-bit values two to three bytes, 24-bit values, big-endian words. A
series column may hold them exactly as sent, with codec ids for big-endian and for 10-, 12- and 24-bit packing in either
bit order. A reader converts only the samples it reads, and any sample reads in place. The recording path then copies
the device's bytes and does no arithmetic. The table of sample codecs is in the [samples module
docs](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/samples/mod.rs).

### Series segments: where samples are in time

A series track's records are its **series segments**. A series segment names its first sample's index, a time grid, and
the scale of the stored numbers from there on. Sample `i` sits at `grid_tick + (i - grid_index) · period`, rounded down.
One series segment starts the track, and a new one starts at every gap, every restart and every change of scale.

A gap or a restart starts a new grid after the last tick of the one before. A change of scale keeps the grid, so no
sample moves by rounding. The track is sticky, so every describe set repeats its newest series segment, and every piece
holds the series segment its first samples are in. A series chunk never spans two series segments: its samples share one
grid and one scale, and its head's first and last tick are its samples' ticks on the grid.

### Example: an autoranging channel changes prefix

A current meter reports a `ww_si` quantity whose prefix follows its range. It records 1.234 A as 1234 with prefix milli,
then switches to its micro range at sample 10 000 and reports 980 µA as 980 with prefix micro. On the wire, the `Series`
batch restarts its numeric column at the item where the prefix changed.

The writer does the same in the file. It seals the open chunk after sample 9 999 and writes a new series segment: first
index 10 000, the same grid as before, scale 10⁻⁶. The next chunk's integers mean microamperes. Sample 10 000's tick is
the one the old grid gives it, so no time moves.

A summary node that covers samples 8192 to 12 287 holds samples of both scales. It is kept at their **common scale**,
the largest scale both are whole multiples of: 10⁻⁶ here. Every millampere number in it is multiplied by 1000, so 1234
becomes 1 234 000, and the node's minimum and maximum stay exact integers. Each summary block states its scale, so it
reads without the series segments. Two scales without a common one (a ratio that isn't a fraction of integers) are
refused, never mixed.

### Wire batches and file columns

A device sends samples to the host in `Series<T>` batches, and the file stores them in
sample columns. The two layers code the same numbers for different readers:

| | `Series<T>` on the wire | Sample columns in a tape |
|---|---|---|
| Size | a batch of about 32 items | a chunk of up to megabytes |
| Read | from the start, item by item | any sample range, by group, in place |
| Encoder | sized for a microcontroller | sized for a host, with SIMD kernels |
| Scale | a prefix per item or in the type | one per series segment |

Neither layer holds the other. The host decodes a batch once and codes its numbers again for the file. The integers pass
through unchanged, and a `ww_si` prefix change ends the chunk where the batch restarts its column. The batch's time
column is not stored: the grid gives every sample's tick, and the host starts a new series segment where the device's
samples leave the grid.

### Series files

Files that hold series are aligned: every chunk's sample payload starts on a 4 KiB boundary, a memory page and a direct
I/O block. The chunk head sits just before its payload, and small blocks waiting for a place (summaries, checkpoints)
fill the gap before the head, so alignment costs little space. Event files are not aligned. The file's describe set
states which it is.

Each series chunk carries a CRC-32C per 64 KiB of payload (a **CRC segment**), so a flipped bit in a 4 MiB chunk costs
one 64 KiB CRC segment, not the chunk. [Damage and recovery](recovery.md) continues from there.

## Summaries

A viewer that draws a track of a billion items over 2000 pixels must not read a billion items. **Summary blocks** hold a
coarse view of a track that the writer builds as it goes, in a tree over item numbers.

- **Event trees.** A level-0 node covers 2048 items; a node one level up covers 16 nodes of the level below (32 768
  items, then 524 288, then 8.4 million). A node holds the first item's start, the last item's start, the OR of the
  items' flags, and for spans the latest end. Counts follow from the item range.
- **Series summaries.** A level-0 node covers a power of two of samples that the track chooses, and each level up again
  covers 16 nodes. A node holds, per channel, the statistics the track declares: minimum, maximum, sum, sum of squares,
  a count of clipped or NaN samples, a flags OR, and for complex samples the mean and maximum of `I² + Q²`. It holds no
  ticks, since the series segments give them.

Nodes run over item numbers, not time, so a sparse track has no empty nodes, and the nodes of consecutive pieces of one
track line up. Every statistic merges: a parent is its 16 children merged in item order. A reader that builds coarser
bins of its own gets exactly what the writer would have stored.

For a track of 1 billion events, level 2 holds about 1900 nodes, roughly one per pixel of a 2000-pixel row. An overview
reads those nodes and no items. Zoomed in, a viewer reads the nodes of the visible range, then the chunks they point to.

Summary blocks are small, a few KiB, and listed in the index like any block. A lost summary block costs nothing that
can't be rebuilt from the data. The layout is in the [summary module
docs](https://github.com/vhrdtech/wire_weaver/blob/master/ww_tape/src/summary/mod.rs).

## See also

- [`Series<T>`](../serdes/series.md): the wire format of the sample batches a device sends.
