---
level: advanced
---

# Advanced

The Advanced part covers every option behind the defaults of the Basic part, and when to pick each one. Read it when
a default is not enough. A type may need to be smaller, a value may change too fast to poll, or another process may
serve the API instead of a device.

The part assumes the Basic part: an [[API trait]], its [[server]] and [[client]], and the `ww` tool. Its chapters stand more on
their own than the Basic ones, so you can read the one you need. Every page in this part has the Advanced badge.

## Chapters

| Chapter | What it teaches |
|---|---|
| [Types in depth](../serdes/size_assumptions.md) | Size assumptions, special types, `Series`, layouts, and reading the wire format from C. |
| [The API in depth](../api/arrays.md) | Arrays, handlers, large messages, optional items, several requests at once, metadata and introspection. |
| [Streams and observing](../api/observing.md) | Observing values at rate, batching, running totals and compression, shown on a motor controller. |
| [Settings and flash](../api/settings.md) | Settings kept across a reboot, and flash that survives a power cut. |
| [In a process, a service, a slot](../api/local_trait.md) | The same API as a plain Rust trait, as a service on the PC, and mounted from elsewhere. |
| [Units in depth](../std_library/units_in_the_model.md) | Units in the API model, points, indices, rounding and magnitudes. |
| [Evolution in depth](../evolution/versions.md) | Versions, the compatibility rules, the checker, snapshots and hosts that talk to several wire lines. |
| [Transport in depth](../transport/usb_internals.md) | The link layer, the framer, following a reboot, capturing traffic, and the hub that shares a device. |
| [Standard library in depth](../std_library/ww_dfu_ab.md) | Firmware updates, recording, counters, device time, analog signals, block devices and bus slaves. |
| [Tools in depth](../python_advanced.md) | More of the Python package, and the viewer. |
| [How the code is laid out](../architecture.md) | The crates of the framework and how they depend on each other. |

When a chapter leaves a question about the bytes or the cost, the [Nomicon](../nomicon/index.md) answers it.
