---
level: basic
---

# ww_i2c_hl

`ww_i2c_hl` runs a stock [embedded-hal](https://docs.rs/embedded-hal/1) 1.0 I2C driver, blocking or async, on the
host against the I2C bus of a remote device serving [`ww_i2c::I2c`](ww_i2c.md): a USB adapter, a dev board, an
instrument. The driver crate doesn't change: it gets an `I2cMaster`, which implements `embedded_hal::i2c::I2c` and
`embedded_hal_async::i2c::I2c` for 7-bit and 10-bit addresses.

```toml
[dependencies]
ww_i2c_hl = "0.1"
```

```rust
let device = MyAdapter::new().connect().await?;
// reads the device's limits once (I2c::i2c_capabilities)
let i2c = I2cMaster::new(device.i2c().attachment()).await?.urgent();
let mut sensor = tmp1x2::Tmp1x2::new(i2c, tmp1x2::SlaveAddr::default());
let t = sensor.read_temperature().await?;
```

`I2cMaster::new_blocking` is the same for sync code, `I2cMaster::with_limits` skips the capabilities call when the
limits are known.

## One transaction, one round trip

Every embedded-hal call (`write`, `read`, `write_read`, `transaction`) is one `ww_i2c` call, so one round trip to
the device: a driver's transaction is never split, and its timing is the device's, not the link's.

embedded-hal runs contiguous operations of the same direction without a repeated start between them, while
`ww_i2c` puts one before every operation ([Transactions](ww_i2c.md#transactions)). `I2cMaster` merges them first:
adjacent writes into one write, adjacent reads into one read whose answer is split back into the driver's buffers.
Then it picks the call:

| Operations after merging | Call |
|---|---|
| one write | `i2c_write` |
| one read | `i2c_read`, `I2cReadKind::Plain` |
| a write, then a read | `i2c_read`, `I2cReadKind::RepeatedStart`, while the write fits `max_write_read_len` |
| anything else (a longer write before a read, three operations or more) | `i2c_transaction` |

A transaction the device doesn't take in one call is refused before anything is sent, never split, since a split
would put a stop and a new start where the driver expects none:

* more merged operations than `max_ops`: `Error::TooManyOps`
* a merged write longer than `max_write_len`: `Error::WriteTooLong`
* more bytes read in the transaction than `max_read_len`: `Error::ReadTooLong`

The limits are the device's `I2cCapabilities`, capped at what the wire format carries (`I2cLimits`,
`I2cMaster::limits`).

## Priority

A latency-bound driver sets its requests urgent: `I2cMaster::urgent()` or `set_priority(Priority::URGENT)`. An
urgent request leaves the host at once and the device answers at once, instead of after the accumulation window of
the link ([latency and priority](../transport/ww_framer.md#latency-and-priority)). The priority belongs to the handle: other handles on the same
client keep theirs. A USB PD sink driving a FUSB302 has 15 ms (tReceiverResponse) to answer a message, and an
urgent I2C round trip over USB is around 2 ms, so the driver's requests have to be urgent.

## Errors

`Error` implements `embedded_hal::i2c::Error`:

| `ww_i2c` error | `ErrorKind` |
|---|---|
| `Nack { at: Some(0), .. }` | `NoAcknowledge(Address)` |
| `Nack { at: Some(n), .. }` | `NoAcknowledge(Data)` |
| `Nack { at: None, .. }` | `NoAcknowledge(Unknown)` |
| `Arbitration` | `ArbitrationLoss` |
| `Bus`, `BusStuck` | `Bus` |
| `Overrun` | `Overrun` |
| anything else (`ClockStretch`, `Timeout`, a refused transaction, a lost link) | `Other` |

The full error stays available: `Error::I2c` holds the device's `I2cErrorOwned`, whose `Nack::op` counts the
merged operations. `I2cMaster::last_transaction_time` gives when the last transaction started and ended on the bus,
in device time.

## Testing (`tests/io_i2c`)

The mock server in `tests/io_i2c` (see [ww_i2c](ww_i2c.md#composing-into-a-device-api-and-testing-tests-io_i2c))
has register-pointer devices to run real drivers against:

* `eh_async_driver_runs_unchanged` - the async `tmp1x2` driver, urgent requests.
* `eh_blocking_driver_runs_unchanged` - the blocking `eeprom24x` driver.
* `eh_transaction_is_merged_into_one_call` - contiguous operations merged, one call per transaction.
* `eh_error_kinds` - the error mapping above.

## GPIO

The same for pins is in `ww_gpio_hl`: blocking pins implement `embedded_hal::digital`, async pins
`embedded_hal_async::digital::Wait`, see [ww_gpio](ww_gpio.md#embedded-hal-ww_gpio_hl).
