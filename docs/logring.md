---
level: basic
---

# logring

[logring](https://vhrdtech.github.io/logring/) is a log store for embedded devices that keeps WireWeaver items. It has
a book of its own, the [logring book](https://vhrdtech.github.io/logring/). This page says what logring is for and
where it meets the rest of WireWeaver.

## What logring keeps

Firmware writes many tracks of shrink_wrap items into one fixed-size ring on an SD card or a flash chip. When the ring
is full, the newest data overwrites the oldest, so a device that logs for months always holds its most recent hours or
days. A power cut costs at most the data staged since the last flush, by default 64 KB or one second. The engine needs
no heap and no allocator.

For example, a data logger on a vehicle writes its CAN bus frames and its battery voltage into two tracks. After a
fault, a technician reads the card on a PC and exports the last few days of both tracks as CSV.

## Where logring meets WireWeaver

- Every item is a [shrink_wrap](serdes/shrink_wrap.md) value, and the ring stores the device's API bundle, so a PC
  names every track and decodes every item without the firmware.
- logring writes to any medium behind `ww_stream`'s `LogMedium` trait, and a WireWeaver stream can be served from the
  ring, so a host that reconnects receives what it missed.
- The [`ww` command line tool](cli.md) includes logring's commands as `ww logring`, and the
  [Python package](python.md#log-files-wire_weaverlogring) includes its API as `wire_weaver.logring`.

## Where to read more

The [logring book](https://vhrdtech.github.io/logring/) explains the ring, power-cut recovery, reading logs on a PC and
the format on the medium. Its page [Tapes and logring](https://vhrdtech.github.io/logring/tapes_and_logring/) compares
a logring with a [tape](tapes.md) and says when to use which.
