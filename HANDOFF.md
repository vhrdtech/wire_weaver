# HANDOFF: bit-io-perf (session on omarchy-m1, branch s/18oct2026-bit-io-perf)

Task: shrink_wrap's bit writer and reader on MCUs, measure and make faster, same bytes; before/after table into the design note (`~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md`, new section after "High-rate observe, measured") and into the CPU section of `docs/articles/series.md`. FEATURES id claimed: **SW-68** (not written into FEATURES.md yet).

## Done (commits on the branch, rebased onto master 911d393 = API-52 landed)

- `bench(bit-io)` 24d0b036: `examples_mcu/mcu_qemu/src/bit_io.rs` (shared body, FNV hash per case), bins `qemu-bit-io` (M3), `qemu-m4f-bit-io` (M4F), `stm32h725ig-bit-io-cycles` (H725 DWT).
- `fix(shrink_wrap)` 344bc55d: `BufReader::bits_left()` / `nibbles_left()` double-counted the current byte at a byte boundary (Rust + C reader `sw_reader_bits_left`/`nibbles_left`), CHANGELOG 🐛 entry written. Bounds test checks both for every state.
- wip commits (to squash into one `perf(shrink_wrap): ...` commit with CHANGELOG at the end, `git reset --soft 344bc55d` then commit):
  - `BufWriter::put_low_bits`: RMW of a 4-byte BE window (`load_be32`/`store_be32` helpers, byte loads/stores with one bounds check so opt z doesn't call slice indexing), split for >32-bit spans, `#[cold]` bytewise path for the last 3 bytes. `check_room` from `bit_pos`. `write_bool` one bounds check.
  - `BitWriter`: 32-bit accumulator (`acc`, `n` 0..31, cached `room`), `push_bits` flushes a word (rev+str); `store()` flushes whole bytes first.
  - `BitSink::put_block(width_bits, width, residuals: &[u32])` (new provided method; overridden by BufWriter `write_block`, BitWriter `put_block` with register in locals, BitCounter). Used by `delta_packed_slice` (u32 low/high halves arrays, no `[W;8]::map` — at z that was an out-of-line try_map + memcpy) and `PackedWriter::finish`.
  - UNib32: `write_forward`/`write_reversed` write runs of up to 7 nibbles via `BufWriter::write_nibbles` (one `put_low_bits`), values < 8 through `write_nib`; `BufReader::take_unib32` reads in one loop (first nibble fast path), `UNib32::read_forward` calls it.
  - `BufReader::read_un32/read_un64` replace the `read_unx!` macro (removed from un.rs): `take_bits` one bounds check (`bits_left`), 4-byte window, a failing read moves nothing; `read_bit` uses `in_front()`. `read_nib` keeps `nibbles_in_byte_left` (fewer M7 cycles than `in_front`, measured).
  - Tests added: `write_un_matches_write_bool`, `unib32_matches_write_nib` (buf_writer.rs), `bit_writer_matches_buf_writer` strengthened (3000 rounds, rev lens, scratch writes, put_block).

## Measurements (raw data in /tmp/wt/bit-io-perf/bit-io-measure/out, scripts there: qrun.sh builds + runs on gpd-omarchy, cmp.py base vs new, sum.py, pcs.py)

Baselines: `b2-*` = master 911d393 + bench (worktree `$CLAUDE_JOB_DIR/tmp/base-wt` detached at 24d0b036, may be gone in a new conversation: `git worktree add --detach <dir> 24d0b036`), `base-*` = old master 2f280416. Latest after = `a3-*`. All outputs/hashes identical before/after.

QEMU M4F instructions, b2 -> a3 (opt 3 / z): biggest FOC observe poll (cells) 115.3k -> 85.3k (0.74x) / 170.4k -> 119.6k (0.70x); default poll 104.8k -> 74.8k / 157.6k -> 106.8k; high-rate 3x400 kHz encode poll 22.9k -> 18.4k / 35.5k -> 27.4k; store column 2510 -> 1601 / 4073 -> 2262.
Bit-io per case (old master base-o3 -> r11/a2, M4F, opt 3): write_un32 1-24 0.65x, 25-32 0.66x, write_bool 0.69x, write_unib32 0.69x, read_un32 0.55x / 0.37x, read_un64 0.45x, read_unib32 0.59x, read_bool 0.74x, Request decode 0.93x, Series decode 0.91x; rev-len finish 1.02x (the only one over 1.00 by >1 %, 2-nibble lengths through put_low_bits).
H725 (old master, cycles opt 3 / z): FOC from store 58.8k -> 43.3k / 98.4k -> 58.9k; per-op tables in /tmp/wt/bit-io-perf/bit-io-measure (copy $CLAUDE_JOB_DIR/tmp/h725 there too if it still exists).

Early numbers were sent to the parent (pre-rebase). The user asked (after API-52 landed): rebase (done), measure the gains on the new poll and the high-rate bench (QEMU done, H725 b2 vs a3 was running in the background: bins in `$CLAUDE_JOB_DIR/tmp/h725/{b2,a3}-o{3,z}/`, outputs `<bin>.txt`), send numbers.

## Next

Steps 1-4 done: flash measured, wip squashed into 0087864a `perf(shrink_wrap)` (CHANGELOG, FEATURES SW-68, entries.md, features page), docs in 1252717d (series article CPU + Nomicon section, serdes/series.md, api/batching.md), design note section "SW-68 word-wide bit writer and reader, measured" written in batched_streams.md (tpm auto-commits). `just build-docs` not run (ask the user first).

5. Gate: `just test`, `just lint`, `just check-mcu`, `just check-examples-mcu`, shrink_wrap fuzz targets 1 min each, then `git rm HANDOFF.md`, `tpm work ready`.

## Gotchas

- QEMU only on gpd-omarchy (ssh flaky: `-o ConnectTimeout=10`, retry); qemu_icount built at /tmp/bit-io-perf/qemu_icount there, ELFs rsynced to /tmp/bit-io-perf/elf/<variant>/.
- M4F bins need `CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` to match the notes' numbers.
- `probe-rs run` never exits (bin loops after "done"): `timeout 60 ... > file`, not piped (output is lost when killed in a pipe).
- At opt z the M7 cycles move a few % with code layout (write_nib +4-8 % with unchanged code).
- No unsafe was added (miri not needed).
