# Handoff: observe-cells (P2401#18oct-0804, API-39 D8)

Design: ~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md, "D8 updated for the shared clock and SW-64" (Roman OK'd D8a-c, 18 Oct 2026).

Done:
- Step 1 (4640dfcb): shrink_wrap `series::SeriesCells` (unsafe, zero-is-empty), derive `series(cells)` (opt-in), `NumericValueCells`, ww_si types opt in. Tests + miri.
- Step 2 (0f4a4b56): ww_stream `batch::{RowCells, RowWriter, CellsReader, CellsSource, Staged, LevelRowCells, RowsSource}`; `Outbox::push_rows_from` takes a `RowsSource`. `just miri`, CI job. Miri finds mixed rows when the second release fence in `commit` is removed (checked by hand).

Next:
- Step 3: codegen `observe_cells = "foc"` in ww_codegen! (wire_weaver_derive args, wire_weaver_core codegen/server/observe.rs): per cells level `<Level>Cells<R>` newtype over `RowCells<Row, R>`, `<Level>Writer` setters (`#[inline]`, `mark(c)`), Row/Samples `SeriesCells` + `LevelRowCells` by hand next to the store; `ObserveBatches<'c>` gets a `Staged` per cells level from a `CellsReader` (no sampler, no batcher); poll path: `poll_cells` + `push_rows_from` loop, `discard_all` when no Samples. Scaffold `--observe-cells`. tests/observe_batched cells cases + same-bytes case.
- Step 4: benches (examples_mcu FOC mirror, sample() vs cells: DWT on H725 omarchy-m1 taking turns with no-u64, qemu_icount on gpd-omarchy). u64 numbers now, 32-bit after no-u64 lands (rebase).
- Step 5: motor demo FOC on cells (motor_sim repo, Arc + reader_shared), docs (observing.md "Cells: sampling from an interrupt" anchor is already linked from docs/serdes/series.md), batching.md numbers, FEATURES (claim an id or extend API-39), CHANGELOGs.

Gotchas: `SeriesCells` is opt-in (`series(cells)`) because a type with a hand-written store inside would stop compiling otherwise. `Staged::flush` also drains the ring.
