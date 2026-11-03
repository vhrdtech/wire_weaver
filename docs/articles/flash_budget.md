---
level: nomicon
---

# What a server costs in flash

A WireWeaver server on a small MCU pays for three things: a fixed floor (request decoding, the reply envelope, the link), the dispatcher's code for each resource of the API, and the serdes of the types those resources use. This page is the harness that measures the first two on the same firmware every time, and the numbers it gave. Every change to server codegen, `shrink_wrap` or the device crates reports its effect here, before and after.

## Running it

```sh
just flash-budget                                   # Cortex-M0+ (thumbv6m-none-eabi)
just flash-budget --targets thumbv7em-none-eabihf   # other cores
```

It builds the `qemu-flash-*` bins of `examples_mcu/mcu_qemu` and reads the flash of each ELF: every allocated section with contents (code, read-only data, vector table). Nothing runs, so no QEMU and no board are needed. Each build is done at both profiles firmware ships with, `fw-z` (fat LTO, one codegen unit, `opt-level = "z"`) and `fw-3` (the same at `opt-level = 3`), and with both server flavours: `use_async = false`, and `use_async = true` with sync handlers (`handler_model = "_=sync"`), as Donguru's firmware is built.

## How it measures

The servers are the roots of `tests/flash_budget_api`:

- `Floor`: one method, what any server costs.
- `BaseOnly`: a trait level with two resources of each kind (calls with and without arguments, a read-write property, a stream).
- `Calls8` / `Calls16`, `CallsArgs8` / `CallsArgs16`, `Props8` / `Props16`, `Streams8` / `Streams16`: that level plus 8 or 16 resources of one kind at the root. They all use the same two types, so serdes is paid once and `(size(*16) - size(*8)) / 8` is the dispatcher's own cost of one more resource of that kind.
- `Typed8` / `Typed16`: calls with an argument and an output of their own types, as in a real API, so each one's serdes is in its cost too.

Two things keep the numbers honest. The request the server gets and every value a handler returns go through `core::hint::black_box`, so the compiler can't drop an arm it would know is never taken. Each handler is `#[inline(never)]` with a body of its own (a static it touches): identical handlers would be merged by LLVM, and the arms calling them with them.

Comparing two sizes that both have resources at the root matters. Against `BaseOnly`, whose root holds only a nested level, adding resources changes the root level's code shape too, and the inliner's choices with it: the server function shrank by 560 B when 16 calls were added in one early try.

The run also checks the largest server for code a firmware should not carry unless it asks for it: the `Series` codec of observe (SRV-54), `core::fmt` `Debug` of `shrink_wrap::Error` with its variant names (SW-69), and the global trait path lookup by name (SRV-53).

## Baseline

Measured 3 Nov 2026 on wire_weaver master of that day (5e695078) plus the harness, bytes:

| thumbv6m, per resource            | z, sync | z, async | 3, sync | 3, async |
|-----------------------------------|--------:|---------:|--------:|---------:|
| floor, one method                 |   26286 |    27110 |   41158 |    41862 |
| call, no arguments or output      |      52 |       56 |     129 |       56 |
| call, an argument and an output   |     240 |       32 |     128 |       22 |
| property, read and write          |     193 |      172 |     263 |      158 |
| stream                            |     496 |      201 |     353 |      209 |
| call with types of its own        |     438 |       76 |     184 |       96 |
| mean of the four shared-type kinds |    245 |      115 |     218 |      111 |

The `Series` codec is linked in every server (36 symbols, 3796 B at `z`), though none of them observes anything. Neither the `Debug` names of `shrink_wrap::Error` nor the trait path lookup by name are in these servers: the first comes from a caller in a real firmware's stack, the second only with global traits in the API.

The per-kind numbers move with the inliner's choices, most in the `use_async` builds, where the arms of a level live inside one state machine. Compare the same cell before and after a change, and the mean for the overall trend. Compare only builds from the same checkout path: panic locations carry each source file's path, so a worktree at a shorter path is smaller by tens of bytes per file (one change looked 250 B better than it was, built in `/tmp`).

For scale: Donguru (typec_dongle, `use_async`, `fw-z` with `opt-level = "z"`, 43 resources of many types) spent about 616 B per resource under the generated dispatcher at wire_weaver 8b7b80ed, and 41.4 KB in all, measured from its ELF by inlined frames.

## SW-69: hand-written `Error` Debug

Measured 3 Nov 2026, before (commit `f8f78726`, the harness alone) against after (with SW-69's hand-written `Debug`), bytes:

| thumbv6m, floor, one method | z, sync | z, async | 3, sync | 3, async |
|------------------------------|--------:|---------:|--------:|---------:|
| before                       |   26378 |    27202 |   41210 |    41914 |
| after                        |   26346 |    27166 |   41174 |    41878 |
| change                       |     −32 |      −36 |     −36 |      −36 |

The per-resource numbers (the "8 resources" row and every "one more X" row) don't move beyond ±1 B rounding, and the SW-69 check still reports "not linked" both before and after: none of this harness's servers formats `shrink_wrap::Error` with `Debug` or `Display`, so the ~450 B of variant names the feature item describes never reaches these builds either way. What moved is a small, constant-size remnant of the derived `Debug` impl that LTO didn't fully strip even though nothing called it. The bytes SW-69 actually saves show up only where a real on-target caller formats the error (or a type nesting it) through `core::fmt::Debug`: fixed in this repo at `ww_settings::Error` / `LoadReport` (`examples_mcu/nucleo_{g0b1re,h743zi2}` `settings.rs`, both already `defmt::Format`) and `ww_stream::store::RamRingError` (`examples_mcu/rp2` `usb_log.rs`, now behind `ww_stream`'s new `defmt` feature); Donguru's own `settings.rs` has the same shape of fix to make, on its own branch.

These absolute floor numbers run a little higher than the "Baseline" table above (e.g. 26378 B against 26286 B at `fw-z, sync`), measured the same day: most likely the checkout path (see the note above this page's Baseline table), since this worktree's path (`/home/roman/git/.wt/flash-sw-debug/wire_weaver`) is longer than whatever path the baseline table was built from, and panic locations carry it per source file. The before/after pair here was measured back to back from the same checkout, so the diff between them is the number that matters regardless.
