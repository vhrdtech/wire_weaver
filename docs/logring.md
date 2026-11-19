---
level: basic
---

# logring

[logring](https://lr.vhrd.tech/) is a log store for embedded devices that keeps WireWeaver items. It has a book of its
own, the [logring book](https://lr.vhrd.tech/). This page says what logring is for and where it meets the rest of
WireWeaver.

## What logring keeps

Firmware writes many tracks of shrink_wrap items into one fixed-size ring on a Secure Digital (SD) card or a flash
chip. When the ring is full, the newest data overwrites the oldest.

A device that logs for months therefore always holds its most recent hours or days. A power cut costs at most the data
staged since the last flush, by default 64 kilobytes (KB) or one second.

The engine needs no heap and no allocator.

For example, a data logger on a vehicle writes two tracks: its Controller Area Network (CAN) bus frames and its
battery voltage. After a fault, a technician reads the card on a computer and exports the last few days of both tracks
as CSV.

## Where logring meets WireWeaver

- Every item is a [shrink_wrap](serdes/shrink_wrap.md) value, and the ring stores the device's API bundle. A computer
  therefore names every track and decodes every item without the firmware.
- logring writes to any medium behind `ww_stream`'s `LogMedium` trait. A WireWeaver stream can be served from the
  ring, so a host that reconnects receives what it missed.
- The [`ww` command line tool](cli.md) includes logring's commands as `ww logring`. The
  [Python package](python.md#log-files-wire_weaverlogring) includes its API as `wire_weaver.logring`.

## Where to read more

The [logring book](https://lr.vhrd.tech/) explains the ring, power-cut recovery, reading logs on a computer and the
format on the medium. Its page [Tapes and logring](https://lr.vhrd.tech/tapes_and_logring/) compares a logring with a
[tape](tapes.md) and says when to use which.
