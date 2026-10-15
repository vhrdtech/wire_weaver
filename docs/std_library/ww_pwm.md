# ww_pwm

Part of the [IO trait family](io_family.md). `ww_pwm` is a global WireWeaver trait pair for PWM outputs and pulse
measurement: setting frequency, duty cycle and polarity on one or more channels, and (on devices with a capture
timer) measuring frequency, duty cycle, pulse width or edge count on others. It is `no_std` by default and depends
only on `wire_weaver` and `ww_si` (for the `Hertz` and `Second` quantities).

Add it to a device API crate with:

```toml
[dependencies]
ww_pwm = "0.1"
```

## Traits

| Trait        | What it is for                                                       | Who implements it          |
|--------------|-----------------------------------------------------------------------|-----------------------------|
| `Pwm`        | `channel[]` of PWM outputs: frequency, duty, polarity.                 | Every device with PWM       |
| `PwmCapture` | `channel[]` of inputs measuring frequency, duty, pulse width, edges.   | Devices with timer capture  |

Measuring pulses is its own trait, not a mode of `Pwm`'s channels: it needs a timer in capture/input mode, which most
devices with PWM output don't have spare on every channel, so a plain PWM driver implements `Pwm` only.

### `Pwm`

```rust
#[ww_trait]
pub trait Pwm {
    ww_impl!(channel[]: PwmChannel);

    fn capabilities() -> PwmCapabilities<'i>;
}

#[ww_trait]
pub trait PwmChannel {
    fn set_output(output: PwmOutput) -> Result<PwmActual, PwmError>;
    fn set_duty(duty: u16) -> Result<u16, PwmError>;
    fn stop(idle_high: bool) -> Result<(), PwmError>;
    fn state() -> PwmState;
}
```

* `channel[]` - one PWM output per channel; channel `n` is `channels[n]` of `capabilities()`.
* `capabilities()` - the channels (name, group, whether a channel also captures), and the frequency and duty range,
  see `PwmCapabilities` under [Types](#types).
* `set_output` - starts or changes a channel's output (frequency, duty, polarity, edge- or center-aligned) and
  returns what was actually set, the nearest the hardware can do. Channels sharing a timer
  ([`PwmChannelInfo::group`](#types)) share its frequency: setting a different frequency on another channel of the
  same group fails with `PwmError::SharedTimer`.
* `set_duty` - changes only the duty, keeping the frequency, and returns the duty actually set. Fails with
  `Common(WrongState)` if the channel is currently idle.
* `stop` - stops the output, leaving the pin high or low.
* `state` - the channel's current output, or `PwmState::Idle`.

### `PwmCapture`

```rust
#[ww_trait]
pub trait PwmCapture {
    ww_impl!(channel[]: PwmCaptureChannel);
}

#[ww_trait]
pub trait PwmCaptureChannel {
    #[stream(CaptureValue, counted)]
    fn capture(config: CaptureConfig) -> Result<CaptureConfig, PwmError>;

    fn set_capture(config: CaptureConfig) -> Result<CaptureConfig, PwmError>;
    fn read_capture() -> Result<CaptureSample, PwmError>;
}
```

* `channel[]` - one capture input per channel; channel `n` is the same index as `Pwm`'s own `channel[]`, see
  `PwmChannelInfo::capture`.
* `capture` - a counted [stream function](../api/streams.md#opening-a-stream): the host opens it with what to
  measure (`CaptureMode`) and the measurement window (`CaptureConfig::gate`), gets back what was actually set, then
  one `CaptureValue` per window until it closes the stream. Its unit is a measurement; the event head carries the
  end of each window as its time and how many measurements the device dropped
  ([stream delivery](../api/stream_reliability.md)).
* `set_capture` - picks what to measure and the window without a stream, for polling with `read_capture`, and
  returns what was actually set.
* `read_capture` - the last measurement, without waiting on the stream. Fails with `Common(WrongState)` before the
  first measurement completes.

## Types

* `PwmOutput` - what to set: `freq: Hertz`, `duty: u16` (0..=65535 = 0..100 %), `inverted: bool`, `phase_correct:
  bool` (center-aligned instead of edge-aligned counting).
* `PwmActual` - what was actually set: `freq: Hertz`, `duty: u16`.
* `PwmState` - `Idle` or `Output(PwmActual)`; `unib32` discriminant.
* `PwmCapabilities<'i>` - `channels: MaxLen<RefVec<'i, PwmChannelInfo<'i>>, MAX_CHANNELS>`, `min_freq` / `max_freq:
  Hertz`, `duty_bits: u8` (the hardware's duty resolution).
* `PwmChannelInfo<'i>` - `name: MaxLen<&'i str, MAX_NAME_LEN>`, `group: u8` (channels sharing a timer share this),
  `capture: bool` (also reachable through `PwmCapture`, at the same index).
* `CaptureConfig` - `mode: CaptureMode`, `gate: Second` (the measurement window, or how long `PulseWidth` waits for
  one pulse to start).
* `CaptureMode` - `Off`, `Frequency`, `DutyCycle`, `PulseWidth`, `EdgeCount`; `sized`, `unib32` discriminant.
* `CaptureSample` - `t: Second` (end of the window), `value: CaptureValue`.
* `CaptureValue` - `Frequency(Hertz)`, `DutyCycle(u16)`, `PulseWidth(Second)`, `EdgeCount(u32)`; the variant always
  matches `CaptureConfig::mode`. `unib32` discriminant.
* `PwmError<'i>` - `Common(CommonError)` ([common errors](../api/errors.md#common-errors)), `SharedTimer`; unsized
  (`unib32` discriminant), evolvable. Each method lists the variants it returns (`#[errors]`).

All enums and structs are behind `#[derive_shrink_wrap]` with `owned(feature = "std")` and
`cfg_attr_borrowed(feature = "defmt", derive(defmt::Format))`, so owned (`*Owned`) variants and `defmt::Format` are
available by enabling the crate's `std` / `defmt` features. `Hertz` and `Second` don't derive `PartialEq` (their
`NumericValue` can carry `f32`); compare their `prefix` and `value` fields directly where needed.

## Composing into a device API

A device exposes PWM outputs and, if it can measure, capture inputs as separate resources on its API root, see the
`tests/io_pwm_api` crate:

```rust
#[ww_api_root]
pub trait IoPwmTest {
    ww_impl!(pwm: ww_pwm::Pwm);
    ww_impl!(pwm_capture: ww_pwm::PwmCapture);
}
```

Because `Pwm` and `PwmCapture` both contain their own `channel[]`, resources end up two levels deep:
`pwm.channel(0).set_output(..)`, `pwm_capture.channel(0).set_capture(..)`.

## Mock server (`tests/io_pwm`)

`tests/io_pwm` is a small in-memory PWM device used for end-to-end tests: it tracks each channel's actual output in
a `Model` shared with the test, and a capture channel's last configured mode and measurement, instead of driving real
hardware. It shows the handler shape `ww_codegen!` expects for a trait nested in an array
(`pwm_channel_set_output`, `pwm_capture_channel_read_capture`, ...) and how to send a measurement on a channel's
counted `capture` stream from the device's event loop, with the `ww_stream::tx::Counted` state the server keeps per
channel (`stream_pwm_capture_channel_capture`):

```rust
api_impl::stream_data_ser()
    .pwm_capture()
    .channel(idx)
    .capture_send_blocking(&mut self.capture[idx as usize], &value, 1, Some(t), out)?;
```

See `tests/io_pwm/src/lib.rs` for the full mock server and client-side test calls (`capabilities`, `set_output` /
`set_duty` / `stop` / `state`, `set_capture` / `read_capture`, and the `capture` stream opened with its
configuration, an invalid one refused, and closed).
