# HANDOFF: SRV-26 bus-slave-engine (ww_bus_slave)

Done: crate `ww_bus_slave` (map.rs, queue.rs, i2c.rs, spi.rs, ring in lib.rs), 27 host tests in `ww_bus_slave/tests/`.

Next: QEMU per-byte instruction counts (mcu_qemu bin `qemu-bus-slave`, qemu_icount, z and 3), FEATURES SRV-26 + STD-35, docs (ww_i2c.md / ww_spi.md mention of the engine), spawn e2e task P2401#19oct-1718b, wait, tpm work ready.

Decisions: times are u32 µs wrapping; `&mut self` everywhere, firmware shares via a critical-section mutex; host map writes during a transaction are staged (STAGE bytes, 6 + data each) and applied at stop / CS release; SPI answer picked at the first `miso()` of a frame (FIFO preload), `rewind()` undoes it; SPI dummy bytes only before read data; word_bits 8 only; a truncated I2C transfer reports `Truncated` even if it ended in a repeated start.
