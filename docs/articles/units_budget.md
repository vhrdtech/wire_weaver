---
level: nomicon
---

# What `Fixed` costs on a microcontroller

`ww_si::Fixed<U, P, T, B>` carries the unit, the scale and the bounds of a number in its type and is stored as exactly `T`. The claim is that a firmware pays nothing for that, apart from the range check you asked for. This page is the measurement of that claim, and the harness anyone can re-run when `Fixed` or a piece built on it (points, rounding conversions, levels) changes.

## How it is measured

The bench is `examples_mcu/mcu_qemu/src/unit_budget.rs` (TEST-27). Every case has a plain twin and a `Fixed` twin that write and read the same bytes: the FNV hash of the written buffer and the read values must be equal for the pair, so a difference is the type and not the wire. The functions under test are `#[inline(never)]` and named `plain_<name>` / `fixed_<name>`.

| Case | Plain | `Fixed` |
|---|---|---|
| struct encode / decode (16 messages) | `{ shared_us: u64, rate_hz: u32, duty: u16 }` | `{ Fixed<Second, Micro, u64>, Fixed<Hertz, One, u32>, Fixed<Ratio, One, u16> }` |
| `Series` encode / decode (32 items, `DeltaPacked`) | `Series<u32>` | `Series<Fixed<Second, Micro, u32>>` |
| range check (64 values) | `if v <= 1000` | `Fixed<Ratio, One, u32, Range<0, 1000>>::try_new(v)` |

What is measured is at the profile the firmware ships with. `fw-z` in `examples_mcu/mcu_qemu/Cargo.toml` is the release profile of `examples_mcu/usb_stm32h725ig`: fat LTO, one codegen unit, `opt-level = "z"`; `fw-3` is the same at `opt-level = 3`. A per-package `opt-level` for the library crates would not do, generic code is instantiated in the firmware crate.

Four things are measured:

- **Instructions** retired between two marker calls on a Cortex-M3 under QEMU (`qemu_icount`, bounded by `--max-insns` and `--timeout`). They are exact and repeatable, but they are not cycles.
- **Flash** is the whole image (every allocated section but `.bss`) of a build with only the plain cases against one with only the `Fixed` cases, plus the own code of each function. A function with no symbol of its own is *folded*: the linker found its code identical to its twin's and kept one copy.
- **Stack** is the frame of each function (`-Z emit-stack-sizes`).
- **Cycles** are the DWT `CYCCNT` of an STM32H725 with caches on at 540 MHz, `cargo run` of `stm32h725ig-unit-budget-cycles` or `just units-budget --cycles`. Instructions are not cycles; the two are reported as what they are.

```sh
just units-budget --qemu-host gpd-omarchy              # instructions, flash, stack at fw-z
just units-budget --qemu-host gpd-omarchy --profile fw-3
just units-budget --qemu-host gpd-omarchy --cycles     # and the STM32H725, replaces the board's firmware
```

`--qemu-host` is for a PC without `qemu-system-arm` (omarchy-m1 has none): the ELF and `qemu_icount` are copied to `/tmp/ww-units-budget` on the host and run there. Later pieces add their cases to `Case` in `unit_budget.rs` as a `<group>: plain` / `<group>: Fixed` pair and the same command prints them.

## Baseline: `Fixed` on 27 October 2026

Instructions per operation (a message, an item, a value), Cortex-M3 under QEMU, `Fixed` against plain:

| Case | `fw-z` plain | `fw-z` `Fixed` | `fw-3` plain | `fw-3` `Fixed` |
|---|---|---|---|---|
| struct encode | 212.8 | 202.8 | 134.8 | 126.8 |
| struct decode | 201.1 | 186.1 | 186.2 | 174.3 |
| `Series` encode | 100.7 | 100.7 | 82.5 | 82.5 |
| `Series` decode | 424.0 | 424.0 | 324.0 | 324.0 |
| range check | 10.2 | 10.2 | 6.5 | 6.5 |

Flash of the whole image: 13 714 bytes with the plain cases only and 13 714 with the `Fixed` cases only at `fw-z` (+0), 20 150 and 20 218 at `fw-3` (+68). Stack frames are equal in every pair (72 / 80 / 136 / 256 bytes at `fw-z`); the range check's frame is 16 bytes at `fw-z`.

What the numbers say:

- `Fixed` costs no instructions where it only changes the type: `Series` and the range check are the same count, because the generated code is the same. At `fw-z` the linker folded `plain_check` and `fixed_check` into one function: the `Range<0, 1000>` check *is* the `if` you would have written.
- The struct cases are 5-7 % fewer instructions with `Fixed` on the M3 at `fw-z`, but the ELF proof below shows the code is not the same there: `Fixed` fields take the derive's per-field checked write, plain fields the latched one. Treat anything within ±10 % as equal; the wire bytes and the hashes are identical.
- A bounded `try_new` is a compare and a branch per value. A range you did not write by hand is the one cost `Fixed` can add, and it replaces the check a setter needed anyway.
- No wide arithmetic is linked for the `u64` of the first field: `Fixed` has its own `Storage`, it does not turn on `ww_numeric`'s `u64`.

## The ELF proof: identical instructions on every core

There was no hardware for this baseline, so the claim is proved from the binaries instead: the same bench is built for Cortex-M0/M0+ (`thumbv6m-none-eabi`), M4F/M7 (`thumbv7em-none-eabihf`) and M33 (`thumbv8m.main-none-eabihf`), each at `fw-z` and `fw-3`, and every `plain_*` / `fixed_*` pair is disassembled with `llvm-objdump` and compared instruction by instruction. Identical instructions cost identical cycles on any silicon, so a board is needed only for a pair that differs.

```sh
just units-elf                                  # six builds, one table each, all pairs
just units-elf --targets thumbv7em-none-eabihf  # one core
```

A line is compared as its mnemonic and operands with addresses removed: a branch keeps its relative offset, a call its callee (a callee with another name, such as `PlainStatus::ser_shrink_wrap` against `FixedStatus::ser_shrink_wrap`, is followed and compared the same way), a `pc`-relative literal the word it loads. A literal or `movw` immediate that points into the image (a panic location in read-only data) counts as "a pointer" and not by its value; the pointee is not compared. The tool is `examples_mcu/fw_budget/src/elf_diff.rs`, with unit tests for the normalization.

Result of 27 October 2026 (M4F and M33 give the same code and the same table):

| Pair | M0+ `fw-z` | M0+ `fw-3` | M4F / M33 `fw-z` | M4F / M33 `fw-3` |
|---|---|---|---|---|
| range check | identical (folded) | identical (folded) | identical (folded) | identical (folded) |
| `Series` encode | identical (99 instr.) | identical (405) | identical (68) | identical (354) |
| `Series` decode | differs, 145 / 133 instr. | identical (231) | differs, 83 / 76 | identical (188) |
| struct encode | differs, 65 / 69 | differs, 111 / 113 | differs, 49 / 51 | differs, 80 / 84 |
| struct decode | differs, 60 / 64 | differs, 94 / 99 | differs, 48 / 54 | differs, 60 / 75 |

(plain / `Fixed` instructions in the function body.) Where the pair is identical, `Fixed` is free on every core, no measurement needed. Where it differs the cause is not the integer: a plain `u32` field of a derived struct is written with the derive's built-in `write_le32_latch` (the error is latched and checked once per struct), while a `Fixed` field goes through `Fixed`'s own `ser_shrink_wrap` and `write_le32`, which checks per field. The bytes are the same (the hashes match), the work is a few instructions more or fewer per field: at `fw-z` the `Fixed` encode and decode are 2-6 instructions and 4-12 bytes larger, at `fw-3` the decode is 15 instructions and 42 bytes larger on M4F/M33. Making the derive treat a `transparent` `Fixed` like its plain field would close that, and this recipe will show it when it does.

Cycles on the STM32H725 for the pairs that differ: **pending**, the board was not behind the probe when this was measured. Run `just units-budget --qemu-host gpd-omarchy --cycles` with the H725 attached to fill in this section.

## Limits of this baseline

- The ELF proof compares code, not the cost of a pointer's pointee or of memory: it says nothing about cache or wait states, which only matter where the code differs.
- QEMU counts instructions, not cycles, and models no cache or wait states; the H725 (Cortex-M7, caches on) will differ in ratio, which is why both units are reported.
- Only today's `Fixed` is measured: `Point`, rounding conversions and `Level` are not in the crate yet. `Fixed` moves into the crate `exact_units` (STD-61); the bench follows by changing the `use` lines in `unit_budget.rs`.
