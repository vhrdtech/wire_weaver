---
level: advanced
---

# I2C and SPI slave engine

A device that implements the `slave` group of [`ww_i2c`](../std_library/ww_i2c.md#slave-mode) or
[`ww_spi`](../std_library/ww_spi.md#slave-mode) stands in for a chip on another master's bus. What it answers, when
a host update takes effect, what happens when the host is too slow and what each event says is fixed by the API;
the `ww_bus_slave` crate does all of it, so a firmware only wires its peripheral's interrupts to it. It is `no_std`,
sans-IO and needs no allocator.

```toml
[dependencies]
ww_bus_slave = "0.5"
```

## Two sides

`I2cSlave` and `SpiSlave` are plain structs with `&mut self` methods of two kinds:

* **Interrupt side**, called per byte from the peripheral's interrupt handler: an address matched, a byte came, a
  byte is wanted, a stop; CS edges for SPI.
* **Task side**, called from the API handlers: `configure` when the `slave_events` / `slave_frames` stream opens,
  `disable` when it closes, `map_write` / `map_read`, `push_answer` for each item of the `slave_answers` sink
  (`free_answer_slots` is how many credits it may give), `clear_answers`, and `peek_event` / `pop_event` /
  `take_dropped` to move events into the counted stream.

The firmware decides how the two share the engine, usually a critical-section mutex in a `static`. `new` is a
`const fn`, so the engine (map, queue and event ring, a few KB) is built in place in that `static` and never
passes through the stack:

```rust
type Slave = ww_bus_slave::I2cSlave<256, 64, 4, 64, 8, 64>;
static SLAVE: Mutex<CriticalSectionRawMutex, RefCell<Slave>> = Mutex::new(RefCell::new(Slave::new()));
```

The const parameters are the sizes: register map bytes, room for staged host updates (each takes 6 bytes more than
its data), answer slots and their length, events and the bytes kept per event. They are what the device reports in
`I2cSlaveCapabilities` / `SpiSlaveCapabilities` (`map_len`, `queue_len`), together with what the peripheral can do
(the stretch limit, `Nack`, 10-bit addresses, the clock).

Times are `u32` microseconds of the device clock, wrapping, the unit the events carry.

## I2C

| Peripheral event | Call |
|---|---|
| Address matched (start or repeated start) | `address(read, now)`; it says whether to acknowledge |
| Byte written by the master | `write(byte, now)` |
| Master reads a byte | `read(now)`: `ReadByte::Byte(b)` to send, or `ReadByte::Stretch` |
| Repeated start reported on its own | `restart(now)` |
| Stop | `stop(now)`, then wake the task |
| Bus error | `bus_error(now)` |

`write` and `read` see the direction themselves when a peripheral reports no address match (DW_apb_i2c on the
RP2040/RP2350 reports bytes, `RD_REQ` and `STOP_DET`): a byte of the other direction ends the transfer going on as a
repeated start.

* **Stretching.** With `I2cSlaveUnderrun::Stretch`, `read` answers `Stretch` when no answer is queued and records
  `I2cSlaveEvent::ReadWaiting` for the task to send at once. The firmware leaves the TX register empty (DW_apb_i2c
  and STM32 hold SCL low on their own meanwhile), arms a timer for `stretch_deadline()`, and calls `resume(now)` when
  it fires and after each `push_answer`: it gives the byte to send once the answer is there or the limit passed.
* **`Nack`.** `acks_address()` says whether the address should be acknowledged now. A peripheral that acknowledges by
  itself has its slave address enabled by it after `configure`, `push_answer` and `stop`.
* **A TX FIFO loaded ahead.** Bytes `read` gave that the master didn't clock out (it NACKed earlier) are taken back
  with `unsent(n)` before `stop`, so the register pointer and the answer stay where the master left them.
* **Host updates** of the map made during a transaction wait for its stop (`map_write` stages them; `Busy` when the
  staging room is full: try again once `is_idle()`), so a master never reads half of a multi-byte value.

## SPI

| Peripheral event | Call |
|---|---|
| TX FIFO has room (also before CS goes active) | `miso()`: the byte to load |
| CS asserted | `cs_asserted(now)` |
| Byte received | `mosi(byte)` |
| RX FIFO overflowed | `overrun()` |
| CS released | `cs_released(now)`, then wake the task |

SPI has no clock stretching, so a frame's answer has to be in the TX FIFO before the master clocks: the frame picks
its answer at its first `miso()` call, usually right after the previous CS release. An answer queued after that goes
to the next frame; a firmware that can empty its TX FIFO calls `rewind()` before CS and loads again, and the new
answer goes out in this frame.

With the register map, the address arrives on MOSI while MISO already runs: a byte loaded before the address is
complete goes out as `idle`, and the master's dummy bytes (`SpiRegisters::dummy_bytes`, before read data only) are
the time the device has. With a FIFO `d` bytes deep, `dummy_bytes >= d - 1` gives every data byte right; with fewer,
the first data bytes come out as `idle` and the map data starts that many bytes in (the record shows what was sent).
The engine takes 8-bit words only; `configure` answers `Unsupported` for others.

## Cost per byte

Instructions per hook call on a Cortex-M3 under QEMU (`examples_mcu/mcu_qemu`, bin `qemu-bus-slave`, counted with
`examples_mcu/qemu_icount`), each including the bench loop and call, about 10 instructions:

| Case | `opt-level = 3` | `opt-level = "z"` |
|---|---|---|
| I2C register map, master writes, per byte | 69 | 66 |
| I2C register map, master reads, per byte | 58 | 59 |
| I2C answer queue, master reads, per byte | 76 | 82 |
| I2C address + one byte + stop | 194 | 218 |
| SPI register map read frame, `miso` + `mosi` per byte | 96 | 100 |
| SPI answer queue frame, `miso` + `mosi` per byte | 85 | 93 |
| SPI CS asserted + one byte + CS released | 232 | 240 |

At 150 MHz (RP2350) 100 instructions are under a microsecond: an I2C byte at 1 MHz takes 9 µs, so the engine is
far from the limit there; for SPI, a FIFO and the dummy bytes are what keep up with a fast clock, not the per-byte
cost alone. Instructions are not cycles: measure on the target for the exact time.
