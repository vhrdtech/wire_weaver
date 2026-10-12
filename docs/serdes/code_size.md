# Code size on small CPUs

How much code `shrink_wrap` costs on a CPU with a few kilobytes of memory, where the bytes go, what was changed
because of it and what is left. Measured in October 2026 for a softcore firmware with 8 KB of RAM for code, data and
stack (the QERV RISC-V of fpga_tools' `busgen`), whose request/reply codec is hand-written because the derived one did
not fit.

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
| `option_de`, `option_ser` | a struct of ten fields, two of them `Option` |
| `busgen_de` | the softcore firmware's receive side: `Request` (18 variants, lists of structs and of `u16`, nested structs, an `Option<Wave>`) decoded and acted on |
| `busgen_ser` | its transmit side: one of the nine `Reply` variants encoded |
| `busgen` | both: a request decoded, acted on, its reply encoded |
| `busgen_hand` | the same as `busgen` with a hand-written codec: bytes, little endian, a list is a count and its items, reading past the end gives zeros |

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

## Build settings

Not `shrink_wrap`'s to fix, but worth more than each code change:

- **Linker relaxation on RISC-V.** `riscv32i-unknown-none-elf` builds without it, so every call that is not
  inlined is `auipc` + `jalr`, 8 bytes. With `-C target-feature=+relax` the linker turns them into `jal` (4 bytes)
  or `c.jal` (2 bytes): `busgen` 9000 → 8114, `busgen_hand` 5762 → 5152. Add it to the `rustflags` of any RISC-V
  firmware that is short of space.
- **`memcpy`** from `compiler_builtins` is 338 bytes on RV32IC (316 on Cortex-M0), word-wise with alignment
  handling. It is linked as soon as a value of more than a few words is moved, which a request or reply enum is.
  `-Zbuild-std-features=optimize_for_size` does not change it. A firmware can define its own byte-wise `memcpy`
  (about 20 bytes; not tried here, the loop must be kept from being turned back into a `memcpy` call).

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

`busgen` is still 3.0 to 3.3 KB above the hand-written codec on RV32IC. In order of what they would bring:

1. **`RefVec<T>` of `Sized` structs and enums without a decode pass**: not planned. It was done for plain
   numbers only (SW-37, [above](#lists-of-plain-numbers-time-for-bytes)), where it is provably the same as the
   pass, and there it costs code instead of saving it. For other `Sized` types it would also change behaviour:
   the stride of an element is not its `size_bits` (alignment padding: `struct { a: bool, b: u8 }` is 9 bits,
   2 bytes per element), and an invalid element (an enum discriminant) would be reported when the list is walked
   instead of when it is decoded.
2. **Latched reads for the rest**: `Nibble`, `u1`..`u32` (`read_un32`), `u64`. Small for the bench types, real for
   bit-packed ones.
3. **A firmware-side `memcpy`**, see [Build settings](#build-settings): 300 bytes.
4. **`finish_for` in generated server and client code**: the code `ww_codegen!` generates and `ww_client_server`
   still end with `finish()`. Most of their messages carry arguments as bytes, a length, so the FIFO stays; a
   stream or a reply of plain values could lose it.
5. **The rest of the length FIFO** for types with lengths: `encode_len_fifo` 214, `write_nib` 130,
   `write_rev_len` 126 bytes on RV32IC. Most of what is left is byte loads and stores, which have no compressed
   form there, and two checks against the end of the buffer.

For a softcore with 8 KB in all, the hand-written codec stays the right choice today: the firmware it was measured
for is 4.65 KB as shipped, and the derived codec alone would be 7.3 KB at best. With 16 KB, or on a Cortex-M0 with
32 KB of flash, the derived codec fits with room to spare.

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
