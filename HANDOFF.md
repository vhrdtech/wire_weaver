# HANDOFF: SRV-26 bus-slave-engine (ww_bus_slave)

Done: crate `ww_bus_slave` (map.rs, queue.rs, i2c.rs, spi.rs, ring in lib.rs), 27 host tests in `ww_bus_slave/tests/`.

Also done: QEMU counts (mcu_qemu bin `qemu-bus-slave`, in docs/api/bus_slave.md), FEATURES SRV-26 ✅, docs page, just lint + just test green. e2e task P2401#19oct-1718b spawned as session slave-e2e-tests (sonnet, from cfb75670).

Next: wait for slave-e2e-tests to say ready, then git rm HANDOFF.md and tpm work ready. docs build (just build-docs) not run: ask the user first (serve-docs).

Decisions: times are u32 µs wrapping; `&mut self` everywhere, firmware shares via a critical-section mutex; host map writes during a transaction are staged (STAGE bytes, 6 + data each) and applied at stop / CS release; SPI answer picked at the first `miso()` of a frame (FIFO preload), `rewind()` undoes it; SPI dummy bytes only before read data; word_bits 8 only; a truncated I2C transfer reports `Truncated` even if it ended in a repeated start.
