---
level: basic
---

# How to read this book

The book has four parts, and each part goes one level deeper than the one before. This chapter explains the parts
and their [[reading level]], suggests a path for a few kinds of reader, and shows how the code and the outputs on
the pages stay true.

## Four parts

| Part | What it gives you |
|---|---|
| Introduction | What WireWeaver is, a first run of a device, and why it has a wire format of its own. |
| [Basic](../basic/index.md) | Everything a device and its host need: your types on the wire, a first device on your PC and on a board, a sensor, an instrument, transports, the standard library, and changing an API. |
| [Advanced](../advanced/index.md) | Every option behind the defaults of the Basic part, each chapter on its own. |
| [Nomicon](../nomicon/index.md) | The internals: exact bit layouts, the protocol, and measurements of what each feature costs. |

The [Reference](../reference/index.md) and the Articles follow the four parts. They hold pages to look things up in,
such as every directive of the derive macro and every `ww` command, and essays that go deep into one topic.

## Reading levels

Every page carries a badge above its title that names its level.

<div class="grid cards ww-level-cards" markdown>

-   <span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span>

    ---
    What you need to build a device and talk to it: what a feature is for, the common case and a working example.
    Start here.

-   <span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span>

    ---
    Every option behind the defaults and when to pick it: compact layouts, observing at rate, services on the PC,
    the evolution rules in full. For when a default is not enough. Assumes the Basic part.

-   <span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span>

    ---
    The internals, as in the Rustonomicon: exact bit layouts, how the reader and writer work inside, measurements
    and the reasons behind design decisions. For writing your own serializer, a reader in another language, or
    changing WireWeaver itself.

</div>

A level says how much a page assumes, not how important it is. A Nomicon page can be the one you need for a
bootloader that has to fit in 2 KB.

The Basic part is a course: read its chapters in order. The Advanced and Nomicon parts are shelves: read the chapter
you need.

## Paths through the book

- **You build a device and its host software.** Read the Basic part in order. Come back to the Advanced part when a
  default is not enough.
- **You only need a compact format for files or packets.** Read [Your data on the wire](../basic/types_on_the_wire.md)
  and the chapters after it up to [Types that change](../basic/types_that_change.md). The format needs no API and no
  device.
- **You write test scripts for an existing device.** Read [A first look](first_look.md), then
  [From the command line](../basic/command_line.md) and [From Python](../basic/python.md).
- **You want to know whether WireWeaver fits your project.** Read [What WireWeaver does](what_it_does.md) and
  [Why a wire format of its own](wire_format.md), then the Nomicon's
  [What a server costs in flash](../articles/flash_budget.md).

## Code, files and transcripts

The pages are built so that what you read is what you get when you run it:

- The name of a file, a Rust item or a term of the [glossary](../glossary.md) is a link with a card. Hover over it,
  or tap it once, to see the file's code, the item's signature and docs, or the term's definition.
- A click on a file or an item opens its lines on GitHub, at the commit the book was built from.
- Every code block in the Basic and Advanced parts is cut from a source that builds and runs.
- Every transcript of a command is the output of a test.

[For writers](../reference/for_writers.md) explains how the pages are made.
