---
icon: lucide/signal
level: basic
---

# Reading levels

Pages carry a small badge above their title: how deep the page goes. Read the Basic pages first and stop when you
have what you need. The deeper levels are there for when you hit a limit, need the last bit of size or speed, or
want to know why something works the way it does.

<div class="grid cards ww-level-cards" markdown>

-   <span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span>

    ---
    What you need to use WireWeaver: what a feature is for, the common case, a working example. Enough to define
    types, put them on the wire and read them back. Start here.

-   <span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span>

    ---
    Every option and when to pick it: compact layouts, special types, the full list of macro directives, worked
    byte-level examples. For when the defaults are not enough. Assumes the Basic pages.

-   <span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span>

    ---
    The internals, as in the Rustonomicon: exact bit layouts, how the reader and writer work inside, measurements
    and the reasons behind design decisions. For writing your own serializer, a reader in another language, or
    changing WireWeaver itself.

</div>

A level says how much a page assumes, not how important it is: a Nomicon page can be the one you need for a
bootloader that has to fit in 2 KB.

!!! note "For writers"
    The badge comes from the page's front matter, `level: basic`, `level: advanced` or `level: nomicon`, and is
    drawn by `overrides/partials/level.html`. The navigation reads as one book in four parts, Introduction, Basic,
    Advanced and Nomicon, each holding pages of one level, so reading on never steps down a level; the Reference and
    Articles follow, outside that order. `just book-check` checks it from `book_checks.toml`: a page needs a level,
    is listed once, and sits in the part of its level.

!!! note "For writers: code and output"
    Outside the Nomicon, a `rust` fence holds only an include of a marked section of a compiled source:
    `// --8<-- [start:NAME]` and `// --8<-- [end:NAME]` in the source, `--8<-- "path/to/file.rs:NAME"` in the fence.
    A Nomicon page may also show a sketch, a fence titled `title="sketch: ..."`. What a reader would see when running
    an example (a transcript, bytes, a chart's data) is written by a golden test into `docs/generated/` through the
    `book_outputs` crate and included the same way; `just book-outputs` writes them again, and a stale one fails
    `just test`. The pages not rewritten yet keep their bare fences, counted in `book_checks.toml`.

!!! note "For writers: bit layouts"
    A Nomicon or Advanced page that shows how bits are laid out uses a `bits` fence: one field per line, its kind,
    its bits (groups of `0` and `1`), then its label. The site draws every bit in its byte, coloured by kind, with
    each byte's hex below and a table of the fields with their bit offsets. A `bytes` line checks the fields: the
    build fails when they don't make those bytes (MSB first, as `shrink_wrap` writes them). Take the bytes from a
    real run (a test or a scratch example, see `AGENTS.md`), never from the bits by hand.

    ````text
    ```bits
    head  0100              n = 4
    run   0001 0            `v.prefix` run: `Milli`, end
    value 0000110011100100  3300
    width 00010             block width 2
    delta 10 00 11          +1 0 -2 (zigzag 2 0 3)
    pad   00                padding
    bytes 42 11 ..
    ```
    ````

    Kinds: `head` (counts, field counts, codec headers), `run` (tag runs), `value` (full values), `delta` (residuals
    and other coded data), `width` (block widths), `pad` (padding). The label starts at the first word that isn't
    only `0` and `1` and may have `code`. [The column layout](articles/series.md#the-column-layout) is a worked one.
    The fence is `docs_ext/ww_docs_ext/bits.py` (tests next to it, in `just test`), its colours `.ww-bits` in
    `docs/stylesheets/vhrd.css`.
