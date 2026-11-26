---
level: basic
---

# Basic

The Basic part teaches what you need to build a device with WireWeaver and talk to it from a computer. After it, you
can put your own types on the wire with [[ShrinkWrap]] and write an [[API trait]]. You can serve the trait from
firmware and call it from Rust, from the command line and from Python. You can also keep the API working while it changes.

The part assumes that you know Rust. It assumes nothing about WireWeaver: each chapter builds on the chapters before
it, so read them in order. Every page in this part has the Basic [[reading level]].

## Chapters

| Chapter | What it teaches |
|---|---|
| [Your data on the wire](types_on_the_wire.md) | Your own types as bytes on a radio link or in a file: the derive macro, borrowed and owned types, the built-in types, and types that change. |
| [A first device](../api/overview.md) | An API trait with methods, properties, streams, traits and errors, and the code generated from it. |
| [Tools](../cli.md) | The `ww` command line tool and the Python package, used on a running device. |
| [Getting connected](transports.md) | USB, RTT, WebSocket, UDP and byte streams, and a host that keeps trying when a device goes away. |
| [Numbers and errors](../std_library/ww_si.md) | Numbers with units, steps and bounds, dates, and the errors every API shares. |
| [The standard library](../std_library/using.md) | Ready-made API traits for pins, serial ports and the information every device reports about itself. |
| [Keeping it working](../evolution/overview.md) | How an API changes so that old hosts and old firmware keep working. |

## Examples

The chapters use the examples of the repository's `examples/` directory. `sensor_packet` puts a sensor node's
readings on a radio link without an API. `blinky_api` and `blinky` are the smallest device and its [[client]], and `blinky_evolved` is
the same device after a change. The [examples page](../examples/examples.md) lists all of them with the firmware for
each board.

When you finish this part, the [Advanced part](../advanced/index.md) covers every option the defaults hide.
