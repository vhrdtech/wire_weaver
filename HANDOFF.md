# Handoff: series-memcpy-fast (P2401#18oct-1026)

## Done
- Step 1 (commit 24c7491c): `examples_mcu/mcu_qemu/src/memcpy_columns.rs`, bins `qemu-memcpy-columns` (M3, M4F) and `stm32h725ig-memcpy-columns-cycles`. H725 warm: bare 2 949 cyc / 2 816 B, framed 8 785 cyc / 2 861 B. QEMU M3 3 911 / 10 609 insns, M4F 3 909 / 10 719. Reported to parent.
- User (18 Oct): go on with step 2, and also measure a column-wise fast DeltaPacked from the SoA columns, byte-identical to today's writer. Design note compares three points on the H725 with bandwidth at 25 kHz.

## Plan (step 2)
1. Numeric column escape `11 11` = aligned plain: 4-bit header, zero bits to a byte, each value LE at ceil(W/8) bytes, no segments. `Predictor::Aligned` (only with `Coder::Plain`), policy `AlignedPlain`. Reader in `NumericReader` (dynamic decoder uses it too). Bounds: item bits max(.., 8*ceil(W/8)), +7 padding per column.
2. `SeriesStore::store_columnar(store, rows) -> bool` + `write_store(store, rows, sink, policy, level)`: column-wise writer from the arrays, same bytes as `write_group` over the items. Leaves: slice writers (fast DeltaPacked, aligned memcpy). Derive: struct = all fields columnar; enum = tag constant over rows + its variant's fields columnar. `NumericValueStore` by hand. Else fall back to the item walk.
3. Use it in ww_stream `level.rs` encode_column and observe codegen; measure on H725 + QEMU.

## Gotchas
- nightly on omarchy-m1 lost `rustc` on 18 Oct; `rustup component remove/add rustc --toolchain nightly` fixed it.
- probe-rs needs `--connect-under-reset` on the H725 after a killed run; wrap in `timeout -k 5 30`.
- QEMU on gpd-omarchy: scratch in /tmp/memcpy-fast-qemu there (qemu_icount + elf/), remove when done.
