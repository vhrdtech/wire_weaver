---
level: nomicon
---

# Code size on small CPUs

How much code `shrink_wrap` costs on a CPU with a few kilobytes of memory, where the bytes go, what was changed
because of it and what is left. Measured in October 2026 for a softcore firmware with 8 KB of RAM for code, data and
stack (the QERV RISC-V of fpga_tools' `busgen`). Its request/reply codec was hand-written because the derived one did
not fit; since fpga_tools 0.9.0 (BUS-12) the firmware runs on `head_tail` views and variant writers (SW-52, SW-53,
SW-54), and no hand-written codec is left.

Summary:

- The derived codec of that firmware went from 11.3 KB to 9.0 KB on RV32IC (15.1 → 12.5 KB on RV32I, 9.3 → 7.3 KB
  on Cortex-M0) with five changes that keep the wire format as it is. With `sized` / `final_structure` types and
  linker relaxation it is 7.3 KB. The same codec written by hand is 5.2 to 5.8 KB.
- Half of what was saved came from one thing: plain fields are no longer checked for an error one by one.
- An encoder whose types have no lists, strings or nested evolvable types does not link the code for lengths at
  all: a struct of eight fields is 964 bytes on RV32IC instead of 1476 (1934 before any of this).
- No cargo feature was needed, and none is proposed: what a firmware does not call is not linked, and a feature
  that changes the wire format would be a trap (see [Feature flags](#feature-flags)).
- A build setting found on the way is worth more than any of the code changes on RISC-V: linker relaxation was off,
  [`-C target-feature=+relax`](#build-settings) saves 9 to 11 % for any firmware, hand-written ones too.

## The bench

`just size-bench` (the `size_bench` crate in `shrink_wrap/size_bench`) builds one bare firmware image per test case
from `shrink_wrap/size_bench/fw` and prints `.text` + `.rodata` in bytes. `just size-bench -c busgen -t rv32ic -s 30`
prints the 30 largest symbols of an image instead.

The build profile is the one of the softcore firmware: nightly, `build-std = ["core"]`, `panic = "immediate-abort"`,
`opt-level = "z"`, fat LTO, one codegen unit, `-Zlocation-detail=none -Zfmt-debug=none`. Targets:

| name | target | flags |
| --- | --- | --- |
| `rv32i` | `riscv32i-unknown-none-elf` | |
| `rv32ic` | `riscv32i-unknown-none-elf` | `-C target-feature=+c` (what the softcore runs) |
| `rv32ic-relax` | `riscv32i-unknown-none-elf` | `-C target-feature=+c,+relax` |
| `thumbv6m` | `thumbv6m-none-eabi` | Cortex-M0, for comparison |
| `thumbv7em` | `thumbv7em-none-eabihf` | Cortex-M4F / M7 (Thumb-2), for comparison with larger MCUs |

Every case is a `run(rx, tx)` that decodes from one buffer and encodes into another. Inputs and outputs go through
`core::hint::black_box`, so nothing is computed at compile time and every decoded field counts as used. The images
are measured, not run.

| case | what it holds |
| --- | --- |
| `empty` | no `shrink_wrap` code: 38 bytes on RV32IC, the floor of every other case |
| `rw_de`, `rw_ser` | `BufReader` / `BufWriter` alone: a `u8`, a `bool`, a `u16`, a `u32` (`rw_ser` ends with `finish_no_lens()` since step 4) |
| `struct_de`, `struct_ser` | a struct of eight integers and flags (`Wave`) |
| `enum_de`, `enum_ser` | an enum of five small variants |
| `vec_de`, `vec_ser` | a struct with a `u16` and a `RefVec<u16>`, the list walked after decoding |
| `bits_de`, `bits_ser` | a bit-packed struct: `U3`, `bool`, `U4`, `U12`, `I10`, `U20`, `U5`, `u8` (`Sample`, since SW-38) |
| `option_de`, `option_ser` | a struct of ten fields, two of them `Option` |
| `busgen_de` | the softcore firmware's receive side: `Request` (18 variants, lists of structs and of `u16`, nested structs, an `Option<Wave>`) decoded and acted on |
| `busgen_ser` | its transmit side: one of the nine `Reply` variants encoded |
| `busgen` | both: a request decoded, acted on, its reply encoded |
| `busgen_hand` | the same as `busgen` with a hand-written codec: bytes, little endian, a list is a count and its items, reading past the end gives zeros |
| `reply` | a method's reply as `ww_codegen!` server code encodes it: a `Wave` return value alone, then the `ww_client_server::Event` around its bytes, and the error event when that fails |

The cases with derived types are built twice: `evolvable` (the default, every type `Unsized`) and `final` (`sized`
where the fields allow it, else `final_structure`). The two differ only where a type is nested in another, so the
small cases give the same number for both.

`busgen_hand` is the yardstick, not `empty`: the handler that acts on a request, the enum values it builds and the
bench's own `black_box` calls are in both `busgen` and `busgen_hand`. What the two differ by is the codec.

## Results

`.text` + `.rodata` in bytes, before (as of `f661ec9c`) and after the five [changes](#what-was-changed):

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m |
| --- | --- | ---: | ---: | ---: | ---: |
| `rw_de` | | 796 → 648 | 512 → 444 | 472 → 404 | 430 → 364 |
| `rw_ser` | | 2540 → 688 | 1806 → 462 | 1690 → 428 | 1460 → 384 |
| `struct_de` | | 944 → 840 | 638 → 554 | 568 → 478 | 530 → 440 |
| `struct_ser` | | 2620 → 1348 | 1934 → 964 | 1746 → 872 | 1596 → 830 |
| `busgen_de` | evolvable | 10748 → 8468 | 7718 → 6040 | 7104 → 5442 | 6316 → 5044 |
| `busgen_ser` | evolvable | 6776 → 5284 | 4762 → 3650 | 4346 → 3256 | 4054 → 3224 |
| `busgen` | evolvable | 15148 → 12532 | 11308 → 9000 | 10338 → 8114 | 9302 → 7320 |
| `busgen` | final | 13672 → 11148 | 10320 → 8062 | 9496 → 7296 | 8370 → 6512 |
| `busgen_hand` | | 7824 | 5762 | 5152 | 4408 |

All cases after the changes:

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m |
| --- | --- | ---: | ---: | ---: | ---: |
| `empty` | | 56 | 38 | 34 | 44 |
| `rw_de` | | 648 | 444 | 404 | 364 |
| `rw_ser` | | 688 | 462 | 428 | 384 |
| `struct_de` | | 840 | 554 | 478 | 440 |
| `struct_ser` | | 1348 | 964 | 872 | 830 |
| `enum_de` | | 1004 | 678 | 586 | 512 |
| `enum_ser` | | 1420 | 1012 | 920 | 864 |
| `vec_de` | | 908 | 592 | 548 | 536 |
| `vec_ser` | | 1634 | 1142 | 1074 | 1030 |
| `option_de` | | 980 | 658 | 554 | 520 |
| `option_ser` | | 1452 | 1048 | 936 | 888 |
| `busgen_de` | evolvable | 8468 | 6040 | 5442 | 5044 |
| `busgen_de` | final | 7588 | 5452 | 4920 | 4496 |
| `busgen_ser` | evolvable | 5284 | 3650 | 3256 | 3224 |
| `busgen_ser` | final | 4464 | 3180 | 2820 | 2796 |
| `busgen` | evolvable | 12532 | 9000 | 8114 | 7320 |
| `busgen` | final | 11148 | 8062 | 7296 | 6512 |
| `busgen_hand` | | 7824 | 5762 | 5152 | 4408 |

`vec_de`, `busgen_de` and `busgen` are 50 to 140 bytes larger since SW-37, which is not in these tables: see
[Lists of plain numbers](#lists-of-plain-numbers-time-for-bytes).

Read as: on RV32IC the derived codec is now 1.55 times the hand-written one with evolvable types and 1.4 times with
final ones (it was 1.95 and 1.8 times), and Cortex-M0 code is about 20 % smaller than RV32IC for the same source.

The encoders, before and after steps 4 and 5 alone (the length FIFO, SW-36; the decoders did not change):

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m |
| --- | --- | ---: | ---: | ---: | ---: |
| `rw_ser` | | 1456 → 688 | 996 → 462 | 920 → 428 | 794 → 384 |
| `struct_ser` | | 2092 → 1348 | 1476 → 964 | 1336 → 872 | 1240 → 830 |
| `enum_ser` | | 2160 → 1420 | 1536 → 1012 | 1396 → 920 | 1298 → 864 |
| `option_ser` | | 2220 → 1452 | 1572 → 1048 | 1412 → 936 | 1286 → 888 |
| `vec_ser` | | 1958 → 1634 | 1378 → 1142 | 1286 → 1074 | 1180 → 1030 |
| `busgen_ser` | evolvable | 5496 → 5284 | 3820 → 3650 | 3424 → 3256 | 3344 → 3224 |
| `busgen_ser` | final | 4708 → 4464 | 3356 → 3180 | 2972 → 2820 | 2944 → 2796 |
| `busgen` | evolvable | 12744 → 12532 | 9170 → 9000 | 8260 → 8114 | 7400 → 7320 |
| `busgen` | final | 11364 → 11148 | 8232 → 8062 | 7446 → 7296 | 6660 → 6512 |

`rw_ser`, `struct_ser`, `enum_ser` and `option_ser` have no lengths and lose the FIFO code (step 4). `vec_ser` and
the `busgen` cases have lists and keep it, smaller (step 5).

## Where the bytes went

The largest symbols of `busgen` (evolvable) on RV32IC before the changes, 11100 bytes of `.text`:

| bytes | symbol | what |
| ---: | --- | --- |
| 6958 | `_start` | everything inlined into the caller: `Request` decode, the handler, `Reply` encode |
| 506 | `BufWriter::encode_len_fifo` | the lengths at the end of the buffer, `u16` slots turned into nibbles |
| 338 | `compiler_builtins::mem::memcpy` | moves of the request and reply values, and every `write_u16` / `write_u32` |
| 326 | `BufReader::read::<Wave>` | a nested evolvable struct of 8 fields |
| 270 | `BufReader::read::<UartFormat>` | 6 fields |
| 238 | `BufWriter::write::<RxFrame>` | 2 fields, with the size slot of an `Unsized` type |
| 156 | `BufReader::read_rev_len` | |
| 138 + 132 | `BufReader::read::<RefVec<Wave>>`, `<RefVec<u16>>` | one pass over the list to find its end |
| 138 | `BufWriter::write_nib` | |
| 132 + 32 + 8 | `u32_div_rem`, `u32_normalization_shift`, `__udivsi3` | one division by 3, see below |
| 124 + 76 | `RefVecIter<Wave>::next`, `RefVecIter<u16>::next` | |
| 122 | `BufWriter::write_rev_len` | |
| 118, 118, 114 | `BufReader::split`, `read_bool`, `read_raw_slice` | |
| 108, 98 | `BufWriter::write_bool`, `write_raw_slice` | |
| 92, 88, 78 | `BufReader::read_u32`, `read_u8`, `read_u16` | three copies of the same bounds checks |
| 82, 64 | `BufWriter::align_nibble`, `align_byte` | |

By kind:

| part | bytes | notes |
| --- | ---: | --- |
| `BufReader` primitives | 810 | |
| `BufWriter` primitives | 1260 | 850 of them only for the length FIFO |
| compiler builtins | 560 | `memcpy` 346, division 180, multiplication 32 |
| generic code per type, not inlined | 1300 | `read::<T>`, `write::<T>`, `RefVec<T>`, `RefVecIter<T>` |
| `_start` | 6960 | the hand-written codec has 4400 here |

So 2.6 KB did not depend on the types at all, where the hand-written codec has 1.15 KB outside `_start` (`memcpy`
included). The code that does depend on the types, the generic instances and `_start`, was 8.3 KB against 4.4 KB
by hand.

**Per field.** A derived `des_shrink_wrap` is a run of `let x = rd.read_u32()?;`. On RV32IC that was 22 to 26
bytes per field: `Result<u32, Error>` came back through memory, because `Error` had variants with a payload, so
every call passed a pointer to a stack slot, loaded the tag, compared it, loaded the value, and had an error exit
of its own (store the error, jump). The hand-written reader returns the value in a register and has no error
exits: 6 to 10 bytes per field. `Request` has about 80 fields and `Reply` about 40.

**The length FIFO.** Lengths of lists, strings and nested `Unsized` values are collected as `u16` slots at the end
of the buffer and turned into nibbles by `finish()`. `encode_len_fifo`, `write_nib`, `align_nibble`,
`write_rev_len` and the nibble counting are 850 bytes on the encode side, `read_rev_len` 156 on the decode side.
`finish()` was always called, so an encoder paid for this even when its types had no lengths: `rw_ser` (four
integer writes) was 1806 bytes. Steps 4 and 5 below are about this.

**A division.** `UNib32::len_nibbles` was `(32 - leading_zeros).div_ceil(3)`. RV32I has neither a divider nor a
count-leading-zeros instruction, so this one line linked 172 bytes of division and a software `clz`: 430 bytes in
every image that encodes (`rw_ser` 1806 → 1378 with a three line loop instead).

**Evolvable nesting.** Every use of an `Unsized` type inside another is a size slot, a `split()` and an encode of
the slots the value used. `busgen` has ten such uses (three types, used in lists, in an `Option` and directly):
`final` is 0.9 to 1.0 KB smaller, and the messages are shorter too. This is the only saving that
changes the wire format, and it is chosen per type, as before.

**Lists.** `RefVec<T>` is decoded twice: once when its parent is decoded, to find where the list ends, and again
when it is walked. Each `T` costs a `read::<RefVec<T>>` (130 to 140 bytes) and a `RefVecIter<T>::next` (75 to 125
bytes). Encoding a `RefVec<T>` also contains the decoder of `T`, for its `Buf` variant (a list taken from another
buffer); the optimizer removes it only when it can prove the list is a slice.

**Decode only, encode only.** Nothing to switch off: the linker keeps what is called. `busgen_de` is 6.0 KB,
`busgen_ser` 3.7 KB, and neither contains the other's primitives.

**Bounds checks and panics.** With `panic = "immediate-abort"` no panic code or message is left, a failed check is
one `unimp`. The checks themselves stay: `BufReader` and `BufWriter` compare against their own limits and then index
a slice, which checks again. It is 4 to 8 bytes per access and was three times in three functions for
`read_u8` / `read_u16` / `read_u32`.

**Cortex-M0.** The same picture with smaller numbers: `_start` 4864 of 7292 bytes after the changes, `memcpy`
316, `encode_len_fifo` 174. Thumb has no linker relaxation to forget, and a call is 4 bytes.

## What was changed

All five keep the wire format: every byte-level test of the workspace passes unchanged.

| step | `busgen` evolvable | `busgen` final | `rw_ser` | `struct_de` |
| --- | ---: | ---: | ---: | ---: |
| before | 11308 | 10320 | 1806 | 638 |
| 1. no division in `UNib32::len_nibbles` | 10872 | 9870 | 1378 | 638 |
| 2. `Error` without payloads, shared integer read / write | 10184 | 9076 | 984 | 532 |
| 3. latched errors for plain fields | 9170 | 8232 | 996 | 554 |
| 4. no FIFO code for types without lengths | 9170 | 8232 | 466 | 554 |
| 5. smaller length FIFO | 9000 | 8062 | 462 | 554 |

(RV32IC, bytes. The other targets move the same way, see [Results](#results).)

1. **`UNib32::len_nibbles` is a shift loop.** No division routine, no software `clz`.
2. **`Error` has no payloads and is `#[repr(u32)]`**, and `read_u8` / `read_u16` / `read_u32` / `read_i16` /
   `read_i32` / `read_f32` are one function taking a length (`write_*` likewise, `read_bool` through one bit
   reader). `Result<u32, Error>` is now two registers, `Result<(), Error>` one with `Ok` as zero, and the bounds
   checks exist once. Writing an integer no longer calls `memcpy`. **Breaking**: `Error::OutOfBoundsWriteUN`,
   `OutOfBoundsReadUN` and `OutOfBoundsSplit` lost their `UNib32` payload (the bit count or length asked for).
   An `Error` of one byte was tried first and gained nothing: `Result<u32, Error>` stays in memory then.
3. **Latched errors.** `BufReader` and `BufWriter` have `read_*_latch` / `write_*_latch` methods for `bool` and the
   integers up to 32 bits: no `Result`, an error is kept in the reader or writer and the read gives 0.
   `rd.latched()` / `wr.latched()` return the first error and clear it; `BufWriter::finish()` returns it too.
   `#[derive_shrink_wrap]` uses them for plain fields, `Option` / `Result` flags and `u8` / `u16` / `u32`
   discriminants, and checks once per struct or enum (and before a `TailSize` slot). A field is then a call and a
   move: 12 to 14 bytes on RV32IC, 6 to 8 with relaxation. Fields with `#[default = ..]`, lists, strings and nested types are
   read as before, so evolution is untouched. The price: `BufReader` is 24 bytes instead of 20 (so is every
   `RefVec`), and the smallest cases grow by 10 to 30 bytes.

   The rule for hand-written code that uses the `_latch` methods: don't use a value before `latched()` returned
   `Ok`, in particular not as a loop bound.
4. **No FIFO code for types without lengths** (SW-36). `SerializeShrinkWrap` has an associated constant
   `PUSHES_LEN`: whether `ser_shrink_wrap` can leave a length for `finish()` to encode. `to_ww_bytes` ends with
   `wr.finish_for::<Self>()`, which is `finish()` when it is `true` and `finish_no_lens()` (latched error, align
   to a byte, done) when it is `false`. The choice is made at compile time, so `encode_len_fifo`, `write_nib` and
   the rest are not linked: 510 to 530 bytes less on RV32IC for `rw_ser`, `struct_ser`, `enum_ser` and
   `option_ser`, about 750 on RV32I, 400 to 440 on Cortex-M0.

   The default is `true` for everything that is not `Sized`, so a hand-written impl stays correct without knowing
   about it. `#[derive_shrink_wrap]` computes it from the fields: `false` only when no field is a string, a list, a
   `RefBox`, a nested `Unsized` type (its size slot is a length) or a user type that pushes one itself (asked
   through its own constant, also inside `Option`, `Result`, arrays and tuples). Evolvable (`Unsized`) root types
   made of plain fields qualify, they need no `sized`. A type that says `false` and pushes a length anyway panics
   in a debug build and gets `Error::LenFifoNotEmpty` in a release one, never a message without its lengths.

   Code that calls `ser_shrink_wrap` and `finish()` itself gets the same with `wr.finish_for::<T>()`, where `T` is
   the type of everything written into that writer.
5. **A smaller length FIFO** (SW-36), for the types that do have lengths. `write_nib` aligns and writes in one
   step (140 → 130 bytes on RV32IC) and `encode_len_fifo` no longer needs `align_nibble`
   (82 bytes, not linked any more); the nibbles are counted in one pass over a slice, as a parity, and written by
   a loop that does not ask for the length of each number first (276 → 214 bytes; 216 → 174 on Cortex-M0).
   `vec_ser` 1378 → 1142, `busgen` 9170 → 9000.

   Fixed on the way: aligning to a nibble from the last three bits of a byte (`align_nibble()`, `write_nib()`, a
   length after 5 to 7 bits) left those bits as they were in the buffer instead of zero. A reader skips them, so
   nothing on the other end changes, but the bytes depended on what the buffer held before.

## Lists of plain numbers: time for bytes

Done after the five steps (SW-37), and the one change here that makes the code larger. Decoding a `RefVec<T>`
reads every element once just to find where the list ends. For `u8`..`u128`, `i8`..`i128`, `f32` and `f64` that
is now one bounds check: reading such a value is "align to a byte, take N bytes" and nothing else, so `count`
reads succeed exactly when one read of `count * N` bytes does. A list that is too short, or a count whose size
overflows, still goes through the pass, so the error and the reader state are the ones it always gave. The wire
format does not change.

| | `RefVec<u8>` of 16 | of 256 | of 4096 |
| --- | ---: | ---: | ---: |
| decode pass | 43 ns | 631 ns | 10254 ns |
| one bounds check | 5 ns | 5 ns | 6 ns |

(`from_ww_bytes` on an x86-64 host, for the proportion. On a small CPU the pass is a call of `read_le32`, about 30
instructions, per element; on a bit-serial softcore each of them takes 32 clock cycles or more.)

It was expected to save code as well, and does not. Since step 2 the pass over a list of integers is a loop of
about 22 bytes around the shared `read_le32`, nothing like the 130 byte `read::<RefVec<u16>>` of the first
profile. The check comes on top of it (the pass stays for the error case), and `read_raw_slice` is no longer
inlined into its only caller:

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m |
| --- | --- | ---: | ---: | ---: | ---: |
| `vec_de` | | 908 → 1004 | 592 → 652 | 548 → 596 | 536 → 582 |
| `busgen_de` | evolvable | 8468 → 8604 | 6040 → 6126 | 5442 → 5504 | 5044 → 5112 |
| `busgen_de` | final | 7588 → 7708 | 5452 → 5524 | 4920 → 4982 | 4496 → 4556 |
| `busgen` | evolvable | 12532 → 12664 | 9000 → 9086 | 8114 → 8180 | 7320 → 7384 |
| `busgen` | final | 11148 → 11268 | 8062 → 8134 | 7296 → 7356 | 6512 → 6576 |

Every other case is unchanged, to the byte: the choice is a constant per element type
(`DeserializeShrinkWrap::PLAIN_BYTES`, hidden, `None` by default), so a list of anything else compiles to the pass
alone. Two other shapes were measured and were larger: one shared function that computes the end and the reader
state after a failure (`vec_de` 714, `busgen` 9082 on RV32IC), and the same on top of `read_raw_slice` (670, 9114).

Only these twelve types, on purpose. `ElementSize::Sized { size_bits }` is not used to skip other elements:

- It is the size of the value without alignment padding, so it is not the stride of an element:
  `struct { a: bool, b: u8 }` is 9 bits and takes 2 bytes. `bool`, `u1`..`uN` and `Nibble` are bit-packed, their
  elements share bytes with each other and the last one with the lengths read from the back.
- The pass is also where an element is validated: an enum's discriminant, a string's UTF-8, a nested length.
  Without it those errors would move from decode time to the iteration, where code written against today's
  behaviour does not expect them.

`shrink_wrap/tests/ref_vec_skip.rs` holds the two against each other, result and whole reader state: every type,
empty to long lists, every truncation, counts up to `u32::MAX`, lists that start in the middle of a byte or end
where the lengths begin, inside a struct with every byte damaged in turn, and 40 000 random buffers read from
random reader states. `just fuzz-shrink-wrap-ref-vec` does the same under libFuzzer.

## Latched bit fields

Done after SW-37 (SW-38). `U1`..`U64` and `I2`..`I64` fields are read like the integers of step 3: no error check
each, one `rd.latched()?` per type. The derive cannot tell `U4` from a nested struct by its name, so the type says
so itself: their `DeserializeShrinkWrap::des_shrink_wrap_latch` (hidden, `read` by default) calls
`BufReader::read_un32_latch` / `read_un64_latch`, which are `read_un32` / `read_un64` with the error kept, and the
derive reads every field of a user type through it. The wire format does not change.

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m |
| --- | --- | ---: | ---: | ---: | ---: |
| `bits_de` | | 1364 → 1276 | 886 → 846 | 786 → 746 | 700 → 666 |

Every other case is unchanged, to the byte: none of them has such a field.

As first landed, this was limited to types whose every field is latched: since step 3 a type that mixed latched
fields with lists or nested types could report a later error instead of the first one, and the `U`/`I` fields
were not to join that. The mixed types were fixed right after (SW-41, [Fixed on the way](#fixed-on-the-way)), and
the limit went with it.

**Not latched: `Nibble`, `u64` / `i64` / `f64`, `u128` / `i128`, the varints.** Tried, and one field of a kind costs
more than it saves: the read is a function of its own then, where with `?` it was inlined into its only caller. A
struct of five `U`/`I` fields, a `bool`, a `Nibble`, a `UNib32` and a `u64` (every field latched, before the
rule above):

| latched | rv32i | rv32ic | thumbv6m |
| --- | ---: | ---: | ---: |
| nothing (before) | 1628 | 1086 | 900 |
| `U`/`I` | 1580 | 1060 | 872 |
| `U`/`I`, `Nibble`, `u64`, `UNib32` | 1628 | 1090 | 908 |
| the same with three more `Nibble`, `UNib32` and `u64`: nothing | 2232 | 1452 | 1204 |
| `U`/`I` | 2172 | 1424 | 1180 |
| `U`/`I`, `Nibble`, `u64`, `UNib32` | 2036 | 1358 | 1100 |

From about two fields of a kind it would pay.

Discriminants other than `u8` / `u16` / `u32` (`Nibble`, `u1`..`u31`, `UNib32`) and `TailSize` slots are read with
`?` as before: the code decides with them before `latched()` is checked. (`u8` / `u16` / `u32` discriminants and
`Option` / `Result` flags have been latched since step 3: a failed one reads as 0 / `false`, i.e. variant 0 or
`None` / `Err`, and since SW-41 the kept error is what the type returns in every case.)

`shrink_wrap/tests/latched_reads.rs` holds each of the 126 types against its read with a `Result` (value, error,
whole reader state, from every bit offset and from 20 000 random reader states, an earlier kept error stays
first) and derived structs and enums against the same fields read one by one with `?`; six hand-made mutants
(a wrong value after an error, an overwritten, lost or wrong error, a wrong value, no sign extension) each fail it. `just fuzz-shrink-wrap-latched` does the same under libFuzzer.

### Transparent fields

DER-20. A [`transparent`](derive_reference.md#transparent) wrapper (`exact_units::Fixed<Second, Micro, u32>`, a
`Count(u32)`) as a field of a derived type takes the path of its plain field on both sides. The read side had it
through `des_shrink_wrap_latch`; the write side gets the same: a hidden `SerializeShrinkWrap::ser_shrink_wrap_latch`
(`ser_shrink_wrap` by default, `ser_shrink_wrap_owned_latch` for owned types), called through
`BufWriter::write_field` for every user-type field. `bool`, `u8`..`u32`, `i8`..`i32` and `f32` override both sides with
their `_latch` methods, and a `transparent` struct without `validate` forwards both to its field. Before it, such a
field went through the wrapper's own `ser_shrink_wrap` with an error check per field: TEST-27's ELF comparison found
+2 to 15 instructions and +4 to 42 bytes per struct encode or decode on Cortex-M0, M4F and M33. The bytes and the
errors are the same; `tests/transparent.rs` (`fields_are_latched_like_the_plain_field`) checks both against the
plain struct, `tests/latched_reads.rs` and the `shrink_wrap_latched` fuzz target hold the plain types' latched
writes against their checked ones.

## Fixed on the way

**Bit reads and the lengths in the last byte** (SW-40). The last byte of a message can hold forward data in its
high nibble and the first length nibble in its low one. `read_un32` / `read_un64` read to the end of that byte
(with 4 bits left, `read_un8(6)` returned 2 bits of the length), and a length was read from a nibble the forward
side had read past. Both are errors now, for malformed input only. In bytes: the bit loop is shorter, the reverse
read has one check more:

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m |
| --- | --- | ---: | ---: | ---: | ---: |
| `vec_de` | | 1004 → 1040 | 652 → 664 | 596 → 608 | 582 → 622 |
| `bits_de` | | 1276 → 1168 | 846 → 774 | 746 → 686 | 666 → 622 |
| `busgen_de` | evolvable | 8604 → 8696 | 6126 → 6182 | 5504 → 5564 | 5112 → 5168 |
| `busgen_ser` | evolvable | 5284 → 5296 | 3650 → 3676 | 3256 → 3282 | 3224 → 3228 |
| `busgen` | evolvable | 12664 → 12756 | 9086 → 9142 | 8180 → 8244 | 7384 → 7440 |
| `busgen` | final | 11268 → 11360 | 8134 → 8190 | 7356 → 7416 | 6576 → 6632 |

(`busgen_ser` grows because encoding a `RefVec<RxFrame>` contains its decoder, see [Lists](#where-the-bytes-went).)

**The first error in types that mix latched and other fields** (SW-41). Since step 3, a type with latched plain
fields and fields read with `?` (a list, a string, a nested evolvable type, an `Option` of one) could return a
later error instead of the first: in a `u32` followed by a `RefVec<u8>`, decoded from 0 to 5 bytes, 11 of 24
cases gave `OutOfBoundsRev` or `OutOfBoundsReadU8` where reading field by field gives `OutOfBoundsReadRawSlice`. It
was an error in every case, but not the one it was before step 3. Now an error kept in the reader stops it: every
read with a `Result` returns that error, a `_latch` read gives 0, and no read moves; an enum without a variant 0
checks `latched()` before it reports an unknown discriminant (a discriminant read after an error is 0). So every
error exit of a derived decoder reports the first error, and nothing read after it decides anything.

Four forms were measured, RV32IC, bytes over the code before the fix:

| form | `struct_de` | `vec_de` | `busgen` evolvable | `busgen` final |
| --- | ---: | ---: | ---: | ---: |
| map the error that leaves a derived decoder to the kept one (`latch.take().unwrap_or(e)`, per type) | +26 | | +124 | |
| the check at the start of every read | +16 | +48 | +68 | +116 |
| the same, the reads with a `Result` only, the `_latch` ones go on | 0 | +34 | +30 | +90 |
| the check at the start of every read, the byte reads sharing one bounds check (chosen) | +48 | +28 | +20 | +20 |

The last one is the cheapest for a real decoder; the small cases pay for a function call the first one inlines.
All targets, with the chosen form:

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m |
| --- | --- | ---: | ---: | ---: | ---: |
| `rw_de` | | 648 → 732 | 444 → 492 | 404 → 446 | 364 → 402 |
| `struct_de` | | 840 → 924 | 554 → 602 | 478 → 520 | 440 → 478 |
| `enum_de` | | 1004 → 1088 | 678 → 726 | 586 → 628 | 512 → 550 |
| `vec_de` | | 1040 → 1060 | 664 → 692 | 608 → 628 | 622 → 632 |
| `bits_de` | | 1168 → 1256 | 774 → 822 | 686 → 728 | 622 → 652 |
| `option_de` | | 980 → 1064 | 658 → 706 | 554 → 596 | 520 → 558 |
| `busgen_de` | evolvable | 8696 → 8736 | 6182 → 6202 | 5564 → 5596 | 5168 → 5196 |
| `busgen_ser` | evolvable | 5296 → 5348 | 3676 → 3718 | 3282 → 3324 | 3228 → 3296 |
| `busgen` | evolvable | 12756 → 12800 | 9142 → 9162 | 8244 → 8276 | 7440 → 7468 |
| `busgen` | final | 11360 → 11388 | 8190 → 8210 | 7416 → 7438 | 6632 → 6652 |

`shrink_wrap/tests/latched_errors.rs` holds a request enum with each of these shapes against the same fields read
one by one with `?`: the result (error included) and the reader of every truncation, every byte damaged in turn
and 20 000 random buffers; an enum without a variant 0 after a failed field. Removing any one of the checks makes
a test fail.

## Fixed-layout fast path

A type whose encoding is a fixed byte layout (SW-44, [derive](layouts.md#fixed_layout-fields-at-fixed-offsets))
does not need `BufReader` or `BufWriter` at all: each field is a load or store at a constant offset. The cases
`header_de` / `header_ser` read and write `Header` (a `[u8; 4]`, two `u8`, a `u16`, four `u32` and a `u64`) through
the reader and writer, `fixed_de` / `fixed_ser` the same type with `from_fixed` / `to_fixed`. `bits_fixed_*` is the
bit-packed `Sample` of `bits_*` (SW-45: shifts and masks at constant bit offsets instead of the bit loop of the
reader and writer):

| case | rv32i | rv32ic | rv32ic-relax | thumbv6m | thumbv7em |
| --- | ---: | ---: | ---: | ---: | ---: |
| `header_de` | 884 | 566 | 484 | 484 | 410 |
| `fixed_de` | 580 | 406 | 354 | 228 | 120 |
| `header_ser` | 1408 | 1022 | 918 | 888 | 856 |
| `fixed_ser` | 788 | 612 | 548 | 462 | 452 |
| `bits_de` | 1256 | 822 | 728 | 652 | 650 |
| `bits_fixed_de` | 464 | 308 | 292 | 276 | 268 |
| `bits_ser` | 1740 | 1204 | 1098 | 1034 | 716 |
| `bits_fixed_ser` | 616 | 458 | 430 | 278 | 310 |

A third to a half less, and the gap grows with the number of types: the fast path has no shared runtime, each type
is its own straight-line code. The bytes left in `fixed_ser` are mostly `memcpy` (see [Build settings](#build-settings)).

Since SW-53 every multi-byte number goes through `fixed_layout::Le` at the bytes of its field: on RISC-V (no
unaligned loads, so a `u32` is four byte loads and three shifts and ors, some 28 bytes) `ld` and `st` are one
function per type and a field is a call with a pointer, elsewhere they are inlined. That costs a type read or
written once: `fixed_de` / `fixed_ser` above grew from 378 / 498 bytes on RV32IC (the two functions per type are
more than the eight fields save). It pays as soon as fields are read and written in many places, as in a
firmware: `busgen_ht` 6004 → 5658 on RV32IC, busgen's own firmware 8.1 KB → 7.7 KB before its views. On a
Cortex-M0, which has no unaligned loads either, the inlined byte loads of 2-byte instructions take less than the
calls (`busgen_view` 3894 inlined, 4196 out of line), so it stays inline there. The bit fields' bytes are now
cleared once per head or struct instead of per field (`bits_fixed_ser` 698 → 458 on RV32IC, 560 → 278 on
Cortex-M0).
On a real format, chunky_mcu_updater's plan reader (header, signed descriptor, steps) takes 892 bytes on Cortex-M0 with
the fast path, 840 with its hand-written parser and 1854 through `BufReader` (its `just size-bench`).
Since chunky 0.9.0 its steps are a `head_tail` enum read through `Step::view` (a WRITE's data is a `List<u8>`, the
same wire bytes), and the reader is 734 bytes on Cortex-M0 and 912 on RV32IMC: below format 1's hand-written 840 /
1252 and below the fast path's 892 / 1086 (chunky_mcu_updater `FEATURES.md`, PROTO-3).

## Head and tail

`busgen_ht` is `busgen` with `head_tail` types (SW-46, [derive](layouts.md#head_tail-a-fixed-head-and-a-sequential-tail)):
the plain structs `fixed_layout`, `Utx`, `I2c`, `I2cSlave`, `Request` and `Reply` `head_tail`, lists as
`head_tail::List`, the same handler. Every fixed field is a load at a constant offset, the lists, the string and
the `Option<Wave>` are taken from the tail by lengths already read from the head:

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m | thumbv7em |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `busgen` | final | 11388 | 8210 | 7438 | 6652 | 6816 |
| `busgen_hand` | | 7824 | 5762 | 5152 | 4408 | 4318 |
| `busgen_ht` | | 7752 | 5658 | 5234 | 4590 | 3992 |
| `busgen_view` | | 6572 | 4988 | 4546 | 3894 | 3496 |

`busgen_view` is the same types and handler through views and the variant writers (SW-52): `Request::view(rx)`
checks the lengths and walks the tail, the handler reads each field at its offset when it uses it, and each reply
is written by `Reply::write_<variant>(tx, fields..)`, without making a `Request` or a `Reply` first. It is the
smallest of all: 13 % below the hand-written codec on RV32IC, 12 % on Cortex-M0, 19 % on Thumb-2. In busgen's real
firmware (fpga_tools BUS-12), where the handler also drives the engines, the views take 5608 bytes of `.text` for
the UART image and 5090 for the I2C one on RV32IC, against 4552 / 4716 hand-written: the hand-written firmware reads
a field as one small call and checks no length, the views check every length before the first field is used.

`busgen_ht` (values: `from_head_tail`, `to_head_tail`) is 2 % below the hand-written codec on RV32IC and 4 % above
on Cortex-M0, 31 % below the derived one; on Thumb-2 it is smaller. Unlike the hand-written one it checks every length (a short message is an error, not
zeros) and the UTF-8 of the string. Linker relaxation helps it less than the others: its calls are fewer, the code
is mostly inline loads. Two things were worth taking out of the runtime on the way: a checked multiplication for
the length of a list (on a Cortex-M0 its overflow check links a 200-byte division routine) and `chunks_exact_mut`
(divides by the chunk length); the list iterator walks a cursor instead of computing `i * len`. On the wire it is
a little smaller than busgen's own format too, since the bools of the head are bits.

## Replies of generated servers

Measured after SW-41, not changed. The code `ww_codegen!` generates for a server ends every message with
`finish()`: the request's reply, a stream's data, a deferred return, an error. All but one of them write a
`ww_client_server::Event`, and `Event` pushes lengths whatever it holds (`PUSHES_LEN` is `true`: its `ErrorKind` is
evolvable and has a size slot, `StreamData` has a `RefVec` path), so for them `finish_for` *is* `finish`. The one
that is not is the return value of a method, serialized alone into a scratch buffer before it goes into the event
as `TailBytes`. `reply` is that code, and with `--fw-features finish_for` the same with `finish_for::<Wave>()` for
the return value:

| return value ends with | rv32i | rv32ic | rv32ic-relax | thumbv6m |
| --- | ---: | ---: | ---: | ---: |
| `finish()` (as generated) | 5120 | 3632 | 3236 | 3144 |
| `finish_for::<Wave>()` | 5144 (+24) | 3646 (+14) | 3250 (+14) | 3160 (+16) |

The same for both flavors (a root `Wave` pushes no length either way). `Event::ser_shrink_wrap` is one function
for every variant of the event, so `encode_len_fifo`, `write_rev_len` and `write_nib` are linked in any firmware
with a generated server, even one whose methods all return plain values; `finish_for` at the return value only adds
`finish_no_lens` next to `finish`. The generated code keeps `finish()`. For the length FIFO to drop out of a
server, `Event` itself would have to lose its lengths: a `final_structure`/`sized` event, which is a wire change of
`ww_client_server`.

## Nesting depth

The [nesting depth limit](wire_format.md#nesting-depth) (P2401) is a check in `split()`, one copy, and at every
`read::<T>()` of a `final_structure` type in place (`Vec`, `RefVec`, `Option`, a `final_structure` user type): a
compare with zero, a store before and one after. The counter itself fits in the padding of `BufReader`, which keeps
its size. Before and after, the cases that changed by more than 4 bytes:

| case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m | thumbv7em |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `header_de` | | 884 → 900 | 566 → 582 | 484 → 496 | 484 → 488 | 410 → 414 |
| `vec_de` | | 1060 → 1092 | 692 → 724 | 628 → 660 | 632 → 648 | 580 → 620 |
| `option_de` | | 1064 → 1072 | 706 → 712 | 596 → 602 | 558 → 558 | 560 → 560 |
| `busgen_de` | evolvable | 8800 → 8924 | 6238 → 6340 | 5628 → 5718 | 5168 → 5176 | 4932 → 5028 |
| `busgen_de` | final | 7828 → 7996 | 5600 → 5660 | 5058 → 5116 | 4636 → 4756 | 4320 → 4400 |
| `busgen_ser` | evolvable | 5372 → 5432 | 3734 → 3762 | 3338 → 3370 | 3288 → 3336 | 3394 → 3350 |
| `busgen` | evolvable | 12816 → 12928 | 9166 → 9246 | 8286 → 8362 | 7420 → 7488 | 7576 → 7668 |
| `busgen` | final | 11388 → 11572 | 8210 → 8396 | 7438 → 7616 | 6652 → 6804 | 6816 → 6908 |
| `reply` | | 7140 → 7140 | 5178 → 5180 | 4534 → 4536 | 4202 → 4230 | 3862 → 3866 |

About 1 to 2.5 % of a whole codec. `busgen` final is 6804 on thumbv6m and 8396 on RV32IC with the saved depth stored
back after the read; a decrement before and an increment after is 6768 and 8408, the check in one shared function
called at every such read 6816 and 8408. There is no switch to turn the limit off: decoding a recursive type from
untrusted data without it overflows the stack.

## Larger MCUs: Cortex-M4F / M7

The `thumbv7em` column is there to see whether Thumb-2 changes the picture. It does not: with the same profile the
images are about as large as on Cortex-M0, and the changes above saved about as much. Before them (`925a29b1`) and
now:

| case | types | thumbv6m before | thumbv6m now | thumbv7em before | thumbv7em now |
| --- | --- | ---: | ---: | ---: | ---: |
| rw_ser | | 1460 | 384 | 1224 | 412 |
| struct_ser | evolvable | 1596 | 830 | 916 | 516 |
| enum_ser | evolvable | 1606 | 864 | 1374 | 572 |
| vec_ser | evolvable | 1472 | 1030 | 844 | 684 |
| option_ser | evolvable | 1710 | 888 | 1414 | 608 |
| busgen_de | evolvable | 6316 | 5196 | 6356 | 4836 |
| busgen_ser | evolvable | 4054 | 3296 | 3902 | 3386 |
| busgen | evolvable | 9302 | 7468 | 9058 | 7612 |
| busgen | final | 8370 | 6652 | 8426 | 6816 |
| busgen_hand | | 4408 | 4408 | 4318 | 4318 |

The small serializers are 30 to 40 % smaller on Thumb-2 than on Cortex-M0, the whole codec is not (7.6 against
7.5 KB): 16 % smaller than before on Thumb-2, 20 % on Cortex-M0. In a real firmware the share is what makes the
difference small: `examples_mcu/usb_stm32h725ig`'s `all_gpio` (embassy, USB, defmt; 66 KB of flash) went from
67412 to 66172 bytes, 1.8 %.

## Build settings

Not `shrink_wrap`'s to fix, but worth more than each code change:

- **Linker relaxation on RISC-V.** `riscv32i-unknown-none-elf` builds without it, so every call that is not
  inlined is `auipc` + `jalr`, 8 bytes. With `-C target-feature=+relax` the linker turns them into `jal` (4 bytes)
  or `c.jal` (2 bytes): `busgen` 9000 → 8114, `busgen_hand` 5762 → 5152. Add it to the `rustflags` of any RISC-V
  firmware that is short of space.
- **A byte-wise `memcpy`, for very small targets only** (SW-39): a measurement and advice, nothing in
  `shrink_wrap` depends on it or asks for it. The `memcpy` of `compiler_builtins` is 338 bytes on RV32IC (316 on
  Cortex-M0), word-wise with alignment handling. It is linked as soon as a value of more than a few words is
  moved, which a request or reply enum is, and an encoder's value is; `-Zbuild-std-features=optimize_for_size`
  does not change it. A firmware can define its own, a byte at a time: 22 bytes on RV32IC and Cortex-M0, 36 on
  RV32I. Its symbol wins over the weak one of `compiler_builtins`. Only worth it where 300 bytes decide whether the
  firmware fits (the 8 KB softcore above), and only with the downsides in mind:

    - **Speed**: one byte per iteration instead of a word, about four times slower for long copies. Fine for moves
      of a few dozen bytes, not for a firmware that copies buffers.
    - **It replaces `memcpy` for the whole firmware**, every crate in it: HAL, drivers, buffer copies of the
      transport, not only `shrink_wrap`'s moves.
    - **The loop can be turned back into a call of `memcpy`**: LLVM recognizes a byte copy loop and replaces it with
      `memcpy`, which here is the function itself, an endless recursion. `compiler_builtins` is built with
      `#![no_builtins]` against this; the code below uses a volatile store. With the nightly of October 2026 LLVM
      leaves the loop alone without it as well, in `memcpy` and in `__aeabi_memcpy`, but nothing promises that.
      Check the disassembly when changing it.
    - **`memmove`, `memset`, `memcmp` are separate** and stay those of `compiler_builtins` (none of them is linked
      in the bench images).
    - On Arm, code calls `__aeabi_memcpy`, whose `compiler_builtins` version calls its own `memcpy` directly, so that
      symbol has to be defined too (and `__aeabi_memcpy4` / `__aeabi_memcpy8` if the code uses them).


  ```rust
  mod byte_memcpy {
      use core::ffi::c_void;

      #[inline(always)]
      unsafe fn copy(dest: *mut c_void, src: *const c_void, n: usize) {
          let (dest, src) = (dest.cast::<u8>(), src.cast::<u8>());
          let mut i = 0;
          while i < n {
              // volatile: LLVM must not turn the loop back into a call of `memcpy`, i.e. of itself
              unsafe { dest.add(i).write_volatile(*src.add(i)) };
              i += 1;
          }
      }

      #[unsafe(no_mangle)]
      pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
          unsafe { copy(dest, src, n) };
          dest
      }

      #[cfg(target_arch = "arm")]
      #[unsafe(no_mangle)]
      pub unsafe extern "C" fn __aeabi_memcpy(dest: *mut c_void, src: *const c_void, n: usize) {
          unsafe { copy(dest, src, n) };
      }
  }
  ```

  `just size-bench --fw-features byte_memcpy` builds the bench with it (`shrink_wrap/size_bench/fw/src/main.rs`,
  off by default, the tables above are without it); every image that copies a value is 300 to 410 bytes smaller,
  the others do not change:

  | case | types | rv32i | rv32ic | rv32ic-relax | thumbv6m |
  | --- | --- | ---: | ---: | ---: | ---: |
  | `struct_ser` | | 1348 → 936 | 964 → 640 | 872 → 554 | 830 → 526 |
  | `vec_ser` | | 1634 → 1222 | 1142 → 818 | 1074 → 756 | 1030 → 726 |
  | `busgen_de` | evolvable | 8604 → 8192 | 6126 → 5802 | 5504 → 5182 | 5112 → 4806 |
  | `busgen_ser` | evolvable | 5284 → 4872 | 3650 → 3326 | 3256 → 2938 | 3224 → 2922 |
  | `busgen` | evolvable | 12664 → 12252 | 9086 → 8762 | 8180 → 7848 | 7384 → 7082 |
  | `busgen` | final | 11268 → 10856 | 8134 → 7810 | 7356 → 7030 | 6576 → 6270 |
  | `busgen_hand` | | 7824 → 7412 | 5762 → 5438 | 5152 → 4834 | 4408 → 4104 |
  | `rw_de`, `struct_de`, `vec_de`, `bits_de`, .. | | unchanged | | | |

  The other `_ser` cases lose the same 412 / 324 / 318 / 304 bytes. With both settings the derived `busgen` codec
  is 7848 bytes on RV32IC (evolvable) and 7030 (final), the hand-written one 4834.

## Feature flags

The options that were on the table, and why none became a cargo feature:

| idea | verdict |
| --- | --- |
| decode-only / encode-only | nothing to do: unused code is not linked (6.0 KB and 3.7 KB of the 9.0 KB) |
| no `u4` / nibble / varint support | nothing to do for the types (`UNib32`, `UVlq32`, `Nibble` are linked only when used); the nibble code of the length FIFO is linked only for types with lengths (step 4) |
| no evolution support | exists per type as `sized` / `final_structure` (0.9 to 1.0 KB here). As a cargo feature it would change the wire format of every type at once, and cargo unifies features across a build: one crate in the tree turning it on would silently change the bytes of all the others |
| fixed-size-only mode, no length FIFO | done without a flag (step 4): decided per type at compile time |
| cheaper errors | done (step 2), for everybody |
| `#[inline(never)]` shared helpers, non-generic paths | done for the integer reads and writes (steps 2 and 3) |

## What is left

With views and writers (SW-52, [Head and tail](#head-and-tail)) the derived codec fits the 8 KB softcore: busgen's
firmware is 5608 bytes (UART) and 5090 (I2C) on RV32IC, against 4552 / 4716 hand-written, with a ceiling of about
6.3 KB. What is left below is for the value path (`BufReader`, `from_ww_bytes`) and evolvable types, which the views
do not use; the sizes quoted in the list are of `busgen` through that path, still 3.0 to 3.3 KB above the
hand-written codec on RV32IC. In order of what they would bring:

1. **`RefVec<T>` of `Sized` structs and enums without a decode pass**: not planned. It was done for plain
   numbers only (SW-37, [above](#lists-of-plain-numbers-time-for-bytes)), where it is provably the same as the
   pass, and there it costs code instead of saving it. For other `Sized` types it would also change behaviour:
   the stride of an element is not its `size_bits` (alignment padding: `struct { a: bool, b: u8 }` is 9 bits,
   2 bytes per element), and an invalid element (an enum discriminant) would be reported when the list is walked
   instead of when it is decoded.
2. **Latched reads for the rest**: done for `U1`..`U64` / `I2`..`I64` (SW-38, [above](#latched-bit-fields)):
   40 bytes less for a bit-packed struct on RV32IC, nothing for the busgen types. Not done: `Nibble`, `u64` and
   the varints pay only from about two fields of a kind. Fixed on the way: mixed types report the first error
   again (SW-41, 20 bytes on `busgen`).
3. **A firmware-side `memcpy`**: measured and documented as advice for very small targets, not a code change
   (SW-39, [Build settings](#build-settings)): 324 bytes less on RV32IC for a firmware that moves a value, at the
   price of speed and of replacing `memcpy` for the whole firmware.
4. **`finish_for` in generated server and client code**: measured, not done ([Replies of generated
   servers](#replies-of-generated-servers)): every server message is a `ww_client_server::Event`, which always
   pushes lengths, so the FIFO stays; `finish_for` for a return value costs 14 bytes on RV32IC instead of saving
   any. The client side runs on `std` only. What would remove it is a size-assumed `Event`, a wire change.
5. **The rest of the length FIFO** for types with lengths: `encode_len_fifo` 214, `write_nib` 130,
   `write_rev_len` 126 bytes on RV32IC. Most of what is left is byte loads and stores, which have no compressed
   form there, and two checks against the end of the buffer.

For a softcore with 8 KB in all the derived codec is the right choice now, through views: the firmware it was
measured for is 4.55 KB (UART image) with the hand-written codec and 5.6 KB with views. The value path alone (derived
`BufReader` codec, 7.3 KB at best) would not fit, and nothing in the list above changes that. With 16 KB, or on a
Cortex-M0 with 32 KB of flash, either fits with room to spare.

**Where the views' last kilobyte goes.** In the real UART image the views are about 1 KB above the hand-written
codec (symbol diff of fpga_tools v0.8.0 and v0.9.0, 14 October 2026, RV32IC):

- `main` is 802 bytes larger: the handler reads each field inline, so the byte loads and stores of `u8` fields, tags
  and headers (`lbu` 33 → 50, `sb` 47 → 58) are in the dispatch itself, where the hand-written one calls a small
  function. The reply buffers are built with a few `memcpy` and `memset` calls (4 and 2).
- A few `__mulsi3` calls: this RISC-V has no multiplier, so every multiplication by a non-constant is a call. They
  do not come from walking a list (`ListIter` advances a cursor and divides nothing). The remaining places are
  `List::get(i)`, which multiplies the index by the item length, and `take_items`, a count times an item length
  (a constant per list type, so mostly shifts). Which of them produced the four calls was not looked at.
- `ListIter<Wave>` and `ListIter<u16>` together are 244 bytes; `Wave::from_fixed` is 150 against 92 for the
  `BufReader` read of the same struct, since every multi-byte field goes through `Le` (SW-53).

Ideas to win some of it back (fewer inline byte accesses, an index without the multiplication, replies written
straight into the output buffer) are tracked in the project's task list, estimated at 300 to 500 bytes; the firmware
fits without them.

## A small UART transport

The softcore talks over a UART with its own framing: `COBS(seq, message, crc16) 0x00`, CRC-16/XMODEM a nibble at a
time from a 16-entry table, streaming on both sides (no second buffer), a reply carries the `seq` of its request.
That module of `busgen_proto`, built alone with the same profile:

| | rv32i | rv32ic | thumbv6m |
| --- | ---: | ---: | ---: |
| encoder | 472 | 326 | 242 |
| decoder | 376 | 254 | 226 |
| both | 724 | 500 | 418 |

plus 32 bytes of CRC table in `.rodata`, 8 bytes of decoder state and the receive buffer.

As a crate of its own (`ww_uart_lite` or similar: a framing layer below `ww_link`, with the host side as a
`Transport` in `wire_weaver_client`) it would cost about that, 0.5 KB on RV32IC. Not built, and not measured: what
`ww_link`'s link setup and `ww_client_server`'s request/response types add on top on such a CPU. The bench has a
place for that case when the transport is built.
