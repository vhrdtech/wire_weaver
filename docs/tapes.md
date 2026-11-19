---
level: basic
---

# Tapes

A **tape** is how WireWeaver records data over time: what a device streamed, what crossed a link, what a program
computed from either. The `ww_tape` crate writes and reads tapes, and the format has a book of its own, the
[ww_tape book](https://vhrdtech.github.io/ww_tape/). This page says what a tape is for and where it meets the rest of
WireWeaver.

## What a tape holds

A tape is a sequence of self-checking blocks that carry the items of many tracks. A **track** is a stream of items of
one WireWeaver type, timed on one clock. A tape also carries the API bundles of the devices it records, so a reader on another PC, years
later, can name every track and decode every item without the device. For example, `ww capture` records a link into
a tape, and `ww explain` later reads that file on any PC and explains its requests and replies without the device.

The format is built for four goals: typed data, exact time on named clocks, large tracks read in place without
decoding them first, and damage that stays local. A crash, a torn write or a flipped bit costs only the bytes it
touched, and a reader says what was lost.

## Where tapes meet WireWeaver

- The [link tap](transport/tap.md) records a WireWeaver link into tapes, and `ww capture` and `ww explain` write and read
  them.
- A tape's items are [shrink_wrap](serdes/shrink_wrap.md) values, and a series track stores the sample batches of a
  [`Series<T>`](serdes/series.md) as columns.
- A track's identity comes from the resource names of the API, so two runs that number resources differently still
  give the same track ids.

## Where to read more

The [ww_tape book](https://vhrdtech.github.io/ww_tape/) explains the concepts, writing and reading tapes, events and
series, recovery after damage, and the format block by block. Its [feature tracker](tape/features.md) is part of this
book's feature tracker too.
