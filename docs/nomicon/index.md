---
level: nomicon
---

# Nomicon

The Nomicon describes the internals of WireWeaver. It shows the exact bit layouts and how the reader and the writer
work. It measures what each feature costs in flash and cycles, and gives the reasons behind the design. Read it to write a reader in another language, to fit
a bootloader into a few kilobytes, or to change WireWeaver itself.

The part assumes the Basic and Advanced parts. Every page in this part has the Nomicon badge.

## Chapters

| Chapter | What it teaches |
|---|---|
| [Inside shrink_wrap](../serdes/wire_format.md) | How values are laid out in bytes, the corpus that keeps every implementation identical, and code size. |
| [What it costs](../articles/flash_budget.md) | The flash a server takes, and what `Fixed` costs on a microcontroller, measured. |
| [Protocol and model](../std_library/ww_client_server.md) | Requests and events, the API model, addressing, the stream protocol and the framer's bytes. |
| [Inside the tools](../evolution/inside_the_checker.md) | How the evolution checker decides, and how a firmware update works inside. |

After the Nomicon, the [Reference](../reference/index.md) holds the pages you look things up in.
