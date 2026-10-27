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
- The struct cases are 5-7 % fewer instructions with `Fixed`. That is a different instruction schedule of identical work (the pair runs in a different inlining context), not a saving to rely on: treat anything within ±10 % as equal. The wire bytes and the hashes are identical.
- A bounded `try_new` is a compare and a branch per value. A range you did not write by hand is the one cost `Fixed` can add, and it replaces the check a setter needed anyway.
- No wide arithmetic is linked for the `u64` of the first field: `Fixed` has its own `Storage`, it does not turn on `ww_numeric`'s `u64`.

Cycles on the STM32H725: **pending**, the board was not behind the probe when this was measured (the J-Link on omarchy-m1 was on an RP2350). Run `just units-budget --qemu-host gpd-omarchy --cycles` with the H725 attached to fill in this section.

## Limits of this baseline

- QEMU counts instructions, not cycles, and models no cache or wait states; the H725 (Cortex-M7, caches on) will differ in ratio, which is why both units are reported.
- Only today's `Fixed` is measured: `Point`, rounding conversions and `Level` are not in the crate yet. `Fixed` moves into the crate `exact_units` (STD-61); the bench follows by changing the `use` lines in `unit_budget.rs`.
