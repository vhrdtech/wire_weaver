# ww_adc

`ww_adc` is a global WireWeaver trait family for analog inputs: a single calibrated reading, continuous sampling
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

* `read` - one calibrated reading of `channel`, in the channel's unit (`AdcError::NoSuchChannel`,
  `AdcError::Busy` if a stream or capture can't be interrupted, `AdcError::NoIoLevel`, `AdcError::Timeout`).
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
    stream!(samples: SampleBlock<'i>);
    fn stream_capabilities() -> StreamCapabilities<'i>;
    fn configure_stream(config: StreamConfig<'i>) -> Result<StreamActual, AdcError>;
    fn stream_state() -> StreamState<'i>;
}
```

* `samples` - raw samples, whole sample sets interleaved in the order of `StreamConfig::channels`, packed as
  `StreamConfig::format`. Only one stream runs at a time.
* `stream_capabilities` - rates, channel counts and formats the stream supports.
* `configure_stream` - sets `config.channels` at `config.rate` sample sets per second and returns what was
  actually set (the nearest rate the hardware can do); applies at once to a running stream. The stream itself
  starts and stops with its own sideband commands, not a method: `samples`' `Open` starts sampling, `Close` stops
  it (already-sampled blocks are still sent). This keeps `AdcStream` free of a separate `start_stream` /
  `stop_stream` pair.
* `stream_state` - the current configuration and whether sampling runs, for a host that joins a running stream.

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
  (`AdcError::WrongMode` before a capture is done, `AdcError::InvalidArgument` outside the data).
* `capture_abort` - stops an armed or running capture and drops its data.

## Types

* `Reading` - `value: NumericValue` (in the channel's unit/prefix), `raw: u32` (the converter's raw code, for
  checking calibration), `t: Second`.
* `AdcCapabilities<'i>` - `channels: MaxLen<RefVec<'i, ChannelInfo<'i>>, MAX_CHANNELS>`.
* `ChannelInfo<'i>` - `name`, `unit: BaseUnit<'i>`, `prefix: Prefix`, `min` / `max: NumericValue`, `bits`
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
* `StreamActual` - `rate: Hertz`, what `configure_stream` actually set.
* `StreamState<'i>` - `config: StreamConfig<'i>`, `actual: StreamActual`, `running: bool`.
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
* `AdcError` - `Unsupported`, `InUse`, `NoIoLevel`, `Refused`, `InvalidArgument`, `WrongMode`, `NoSuchChannel`,
  `Busy`, `Timeout`; `self_describing` (`unib32` discriminant), shared by every trait in the crate.

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")` and
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`, so owned (`*Owned`) variants (`ChannelInfoOwned`,
`AdcCapabilitiesOwned`, `StreamConfigOwned`, `StreamStateOwned`, `CaptureConfigOwned`, ...) and `defmt::Format` are
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
* `stream_configure_open_receive_close` - `AdcStream::configure_stream` (the mock caps the requested rate at
  500 kHz, like real hardware would), opens the `samples` stream with its sideband `Open`, receives a block, and
  closes it; `Adc::read` answers `AdcError::Busy` while the stream runs.
* `capture_now_trigger_and_read` - `AdcCapture::capture` with `Trigger::Now` completes immediately
  (`CaptureStatus::Done`), `capture_read` returns the data with bounds checking
  (`AdcError::InvalidArgument` past the end), and `capture_abort` drops it (`AdcError::WrongMode` after).
* `capture_polled_trigger_progresses_to_done` - a non-`Now` trigger (`Trigger::Rising`) arms the capture; the mock
  advances `Armed` -> `Triggered` -> `Done` one step per `capture_status` poll, standing in for the real wait on
  the trigger, the same way a host would poll a real device.
