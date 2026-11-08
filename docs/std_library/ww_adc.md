---
level: advanced
---

# ww_adc

Part of the [IO trait family](io_family.md). `ww_adc` is a global WireWeaver trait family for analog inputs: a single calibrated reading, continuous sampling
streamed to the host, and lossless triggered block capture into device RAM. It is `no_std` by default and depends
only on `wire_weaver`, `ww_numeric` and `ww_si` (for `Volt`/`Hertz`/`Second` and `NumericValue`).

Add it to a device API crate with:

```toml
[dependencies]
ww_adc = "0.1"
```

## Traits

A thermometer or a battery monitor implements `Adc` only; a data logger adds `AdcStream`; a device that samples
faster than its link (e.g. 500 kS/s over USB full speed) adds `AdcCapture`. A trait a device doesn't put into its
API tree costs it nothing.

### `Adc`

The channels, their units and ranges, and one calibrated reading on demand:

```rust
#[ww_trait]
pub trait Adc {
    fn read(channel: u8) -> Result<Reading, AdcError>;
    fn capabilities() -> AdcCapabilities<'i>;
}
```

* `read` - one calibrated reading of `channel`, in the channel's unit (`NoSuchChannel`,
  `Common(Busy)` if a stream or capture can't be interrupted, `Common(NoPower)`, `Common(Timeout)`).
* `capabilities` - the channels, their units, ranges and calibration, see `AdcCapabilities` under
  [Types](#types) below.

Channels are numbered `0..n` in the order `AdcCapabilities::channels` lists them; every other method addresses a
channel by that same index. Several `Adc::read`s, even in one multi-call, are taken one after the other — a
capture of one sample set (`AdcCapture::capture` with `sets: 1`, `Trigger::Now`) is how several channels are read
at the same instant, if `CaptureCapabilities::simultaneous` says the hardware supports it.

### `AdcStream`

Continuous sampling of chosen channels, streamed while the link keeps up:

```rust
#[ww_trait]
pub trait AdcStream {
    #[stream(SampleBlock<'i>, counted)]
    fn samples(config: StreamConfig<'i>) -> Result<StreamActual, AdcError>;
    fn stream_capabilities() -> StreamCapabilities<'i>;
}
```

* `samples` - a counted [stream function](../api/streams.md#stream-functions): the host opens it with the channels,
  rate and format to sample and gets back what was actually set (the nearest rate the hardware can do), or an
  `AdcError`. Each `SampleBlock` holds whole sample sets interleaved in the order of `StreamConfig::channels`,
  packed as `StreamConfig::format`. Closing the stream stops sampling (blocks already sampled are still sent);
  opening it again restarts it with the new configuration. Only one stream runs at a time.
* `stream_capabilities` - rates, channel counts and formats the stream supports.

The stream's unit is a **sample set** (one sample of every configured channel). Positions, sets the device dropped
and the device time of a block's first set travel in the event head the framework adds
([stream delivery](../api/stream_reliability.md)), so a block is pure sample data:

```rust
// host
let (actual, mut samples) = adc.adc_stream().samples(config).call().await??;
while let Ok(ev) = samples.recv_event().await {
    match ev {
        StreamEvent::Item { pos, units, t, value, .. } => { /* `units` sets from set `pos`, the first at `t` */ }
        StreamEvent::Gap { at, units, cause, .. } => { /* `units` sets missing from `at` */ }
        _ => {}
    }
}

// device, in the event loop: two sets just sampled, the first at `t`
api_impl::stream_data_ser().adc_stream().samples_send_blocking(&mut server.samples, &block, 2, Some(t), &mut server.sink())?;
```

### `AdcCapture`

Lossless capture: samples go into device RAM at any rate the hardware can do, the host reads them out after:

```rust
#[ww_trait]
pub trait AdcCapture {
    fn capture_capabilities() -> CaptureCapabilities<'i>;
    fn capture(config: CaptureConfig<'i>) -> Result<CaptureActual, AdcError>;
    fn capture_status() -> CaptureStatus;
    fn capture_read(offset: u32, len: u16) -> Result<MaxLen<RefVec<'i, u8>, MAX_CAPTURE_READ_LEN>, AdcError>;
    fn capture_abort();
}
```

* `capture_capabilities` - rates, memory depth, which triggers work, and formats.
* `capture` - arms a capture. With `Trigger::Now` it starts at once; otherwise it samples `pre_trigger` sets
  continuously and keeps them, waits for the trigger, then takes the rest. Replaces a previous capture and its
  data.
* `capture_status` - where the capture stands: `Idle`, `Armed`, `Triggered { t }`, or `Done { t_first, t_trigger,
  sets }` once the data is ready — a host polls this until `Done`.
* `capture_read` - reads `len` bytes of the captured data from `offset`; raw samples, interleaved in the order of
  `CaptureConfig::channels`, `CaptureActual::bytes` long, with the first `pre_trigger` sets before the trigger
  (`Common(WrongState)` before a capture is done, `Common(InvalidArgument)` outside the data).
* `capture_abort` - stops an armed or running capture and drops its data.

## Types

* `Reading` - `value: NumericValue` (in the channel's unit/prefix), `raw: u32` (the converter's raw code, for
  checking calibration), `t: Second`.
* `AdcCapabilities<'i>` - `channels: MaxLen<RefVec<'i, ChannelInfo<'i>>, MAX_CHANNELS>`.
* `ChannelInfo<'i>` - `name`, `unit: SIExpr<'i>` (a unit of ww_si or the device's own, see [run-time units](ww_si.md#units-known-only-at-run-time)), `prefix: Prefix`, `min` / `max: NumericValue`, `bits`
  (converter resolution), `calibrated: bool`, `raw_to_value: Option<Linear>` (how raw stream/capture samples turn
  into values; `None` if they don't linearly).
* `Linear` - `scale: f32`, `offset: f32`; `value = raw * scale + offset`.
* `SampleFormat` - `U8`, `U16`, `Packed12` (two 12-bit samples in three bytes), `I16`, `I24`; `sized`
  (`unib32` discriminant).
* `StreamCapabilities<'i>` - `max_total_rate: Hertz` (all channels together), `max_channels: u8`,
  `formats: MaxLen<RefVec<'i, SampleFormat>, MAX_FORMATS>`.
* `StreamConfig<'i>` - `channels: MaxLen<RefVec<'i, u8>, MAX_CHANNELS>` (interleave order), `rate: Hertz`,
  `format: SampleFormat`, `flush_interval: Option<Second>` (send a partial block at least this often; `None` lets
  the device choose, around 10 ms).
* `StreamActual` - `rate: Hertz`, what opening `samples` actually set.
* `SampleBlock<'i>` - `data: MaxLen<RefVec<'i, u8>, MAX_BLOCK_LEN>`, the stream's raw bytes.
* `CaptureCapabilities<'i>` - `max_total_rate: Hertz`, `max_bytes: u32`, `simultaneous: bool` (channels of one
  sample set taken at the same instant), `level_trigger` / `external_trigger: bool`,
  `formats: MaxLen<RefVec<'i, SampleFormat>, MAX_FORMATS>`.
* `CaptureConfig<'i>` - `channels`, `rate: Hertz`, `sets: u32` (total, `pre_trigger` included), `pre_trigger: u32`,
  `trigger: Trigger`, `format: SampleFormat`.
* `Trigger` - `Now`, `Rising { channel, level: NumericValue }`, `Falling { channel, level }`, `External`; `sized`
  (`unib32` discriminant).
* `CaptureActual` - `rate: Hertz`, `bytes: u32` (size of the data once done).
* `CaptureStatus` - `Idle`, `Armed`, `Triggered { t: Second }`, `Done { t_first: Second, t_trigger:
  Option<Second>, sets: u32 }`; `sized` (`unib32` discriminant).
* `AdcError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)), `NoSuchChannel`; unsized
  (`unib32` discriminant), evolvable, shared by every trait in the crate.

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")` and
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`, so owned (`*Owned`) variants (`ChannelInfoOwned`,
`AdcCapabilitiesOwned`, `StreamConfigOwned`, `CaptureConfigOwned`, ...) and `defmt::Format` are
available by enabling the crate's `std` / `defmt` features. Types without a lifetime (`Reading`, `Hertz`, `Second`,
`StreamActual`, `CaptureActual`, `CaptureStatus`, `AdcError`, ...) are used as is on both sides.

## Composing into a device API

A device exposes whichever traits it supports directly on its API root (or nested under a group), see
`tests/io_adc_api`:

```rust
#[ww_api_root]
pub trait IoAdcTest {
    ww_impl!(adc: ww_adc::Adc);
    ww_impl!(adc_stream: ww_adc::AdcStream);
    ww_impl!(adc_capture: ww_adc::AdcCapture);
}
```

## Worked example (`tests/io_adc`)

`tests/io_adc` runs a mock server implementing all three traits against the real host event loop, through
`tests_common::start_device` (an in-process `ww_device::Server` and a generated client, see `tests/credit_sink` for
another worked example of the pattern). It is a convenient reference for wiring the traits up on the server side:

* `capabilities_and_read` - queries `Adc::capabilities`, reads a calibrated and an uncalibrated channel, and
  checks `AdcError::NoSuchChannel` for an out-of-range index.
* `stream_open_receive_close` - opens `AdcStream::samples` with an unsupported format (`AdcError::Unsupported`,
  nothing opened), then with a good configuration (the mock caps the requested rate at 500 kHz, like real hardware
  would), receives a block with its position, set count and time, and closes it; `Adc::read` answers
  `AdcError::Busy` while the stream runs.
* `capture_now_trigger_and_read` - `AdcCapture::capture` with `Trigger::Now` completes immediately
  (`CaptureStatus::Done`), `capture_read` returns the data with bounds checking
  (`Common(InvalidArgument)` past the end), and `capture_abort` drops it (`Common(WrongState)` after).
* `capture_polled_trigger_progresses_to_done` - a non-`Now` trigger (`Trigger::Rising`) arms the capture; the mock
  advances `Armed` -> `Triggered` -> `Done` one step per `capture_status` poll, standing in for the real wait on
  the trigger, the same way a host would poll a real device.
