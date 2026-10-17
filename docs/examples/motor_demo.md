---
level: advanced
---

# Motor controller demo

A VESC-like field oriented controller drives a simulated motor, and its ~50 variables are
[observed](../api/observing.md) by a host, each at the rate it asks for, within a byte budget per trait level. It shows what observing many fast-changing properties costs on the link, and what it would cost as one
`StreamData` event per value.

- `examples/motor_demo_api`: the API, 52 `+observe` properties in three trait levels, one per rate tier, their
  values `ww_si` quantities.
- `examples/motor_demo`: the controller, the device (in this process or on a UDP socket), the host side counting
  and the `motor_demo` CLI.
- The motor, inverter, mechanics and sensors are simulated by [motor_sim](https://github.com/vhrdtech/motor_sim) (a
  PMSM in dq, solved exactly per PWM period, with ADC noise, offsets and quantisation, an encoder and seeded faults).

```sh
cargo run --release -p motor_demo -- observe --compare        # a device in this process, 3 tiers, 1 s reports
cargo run --release -p motor_demo -- device --listen 127.0.0.1:9000
cargo run --release -p motor_demo -- observe --udp 127.0.0.1:9000 --prop i_q=every:1ms,speed_electrical,fault
cargo run --release -p motor_demo -- list                     # every property with the unit it is shown in
source <(COMPLETE=bash motor_demo)                             # completion of commands and property names
```

## The API: one trait level per rate tier

The VESC firmware (vedderb/bldc) computes its variables in three tiers, and so does this controller:

| Level | Rate | Properties | What |
|---|---|---|---|
| `foc` | 25 kHz, every PWM period | 21 | phase, αβ and dq currents, dq and αβ voltages, duties, the angle in use, the flux observer's and the encoder's angles, observer state, bus voltage, the true angle |
| `control` | 1 kHz | 18 | setpoints (`mode`, `sensor_mode`, duty, current, speed, all `rw`), PLL speed, current setpoints, filtered currents, input current, duty, torque, position, observer error, the true speed |
| `telemetry` | 50 Hz | 13 | time, charge and energy drawn and returned, tachometer, input voltage and power, averaged currents, `fault`, the real time factor |
| `sim` | | 4 + 2 methods | `time_scale`, `load`, `motor`, `profile`; `inject(fault)`, `reset()` |

```rust
#[ww_api_root]
pub trait MotorDemo {
    ww_impl!(foc: Foc);
    ww_impl!(control: Control);
    ww_impl!(telemetry: Telemetry);
    ww_impl!(sim: Sim);
}

#[ww_trait]
pub trait Foc {
    /// Phase a current as measured (offset calibrated): `Milli` `I32`.
    property!(ro+observe i_a: Ampere);
    // ..
}
```

Each level is one observe subscription, so whatever is due in a level goes out in one `PropertyUpdate`, and a host
asks for each tier at its own rate: `foc` at 1 kHz for a scope, `control` on change, `telemetry` at 10 Hz for a
dashboard.

### Values are `ww_si` quantities

Every physical value is a [`ww_si`](../std_library/ww_si.md) quantity: `Ampere`, `Volt`, `Radian`, `Weber`,
`AngularVelocity`, `Degree`, `Torque`, `Coulomb`, `Joule`, `Watt`, `Second`, and `Ratio` for duties, probabilities
and the time scale. VESC's own units become SI: erpm is `speed_electrical` in rad/s (×60/2π back to erpm), Ah is
`charge_drawn` in C (÷3600), Wh is `energy_drawn` in J (÷3600). The docs of each property give the prefix and
storage the device sends it in, three number types in all:

| Storage | For | Examples |
|---|---|---|
| `I32` at a fixed prefix | every signed physical value | currents and voltages in mA and mV, angles in µrad, speeds in mrad/s, flux in nWb, duties in millionths |
| `U32` | totals that only grow | `time` in ms, `charge_drawn` in C, `energy_drawn` in J |
| `F32` | values that span decades | `time_scale`, `real_time_factor`; setpoints the host writes |

The prefix sets the resolution, so noise below it costs nothing: the device marks a value changed only when its
quantity changed, which is why `telemetry` on change sends 311 values/s instead of the 520 it did with `f32`. Setters take whatever prefix and
storage the host sends: the CLI writes `F32` in the base unit. A firmware of this API compiles `ww_si` with only the
`i32`, `u32` and `f32` number types. The host shows values in the old units (erpm, rpm, Ah, Wh, degrees), as VESC
Tool does.

Next to `f32`, an `I32` quantity is one byte more per value (the prefix and storage nibbles): the `foc` level at
1 kHz takes 117 kB/s instead of 96. Batched observe (`Series` of `Timed` values, API-39) is where the fixed prefix
pays off: there the cost of a value is its change from the previous one, not its storage width.

## The device

The device's clock is simulated time, one PWM period (40 µs) per step; simulated time follows real time times
`sim.time_scale`. Every step:

1. the controller runs on the sensor readings (current offsets measured during the first 0.1 s, Clarke and Park,
   the Ortega flux observer and a PLL, d/q current PI loops with decoupling, space vector PWM; the speed loop and the
   filters every 25th step, the counters every 500th), the simulator steps with the duties it gives;
2. every value of a tier that ran and changed is marked, `mark_changed_<name>()`, so `change:` observers get it;
3. `poll_properties(out, (), now_us, buf)` sends what is due, with `now_us` the simulated microseconds.

Messages are up to 4 KiB (`MAX_MESSAGE_LEN`) each way, the in-process and UDP buffers sized from it: room for the
whole `foc` level batched, a `Series` of 32 values per property is about 1.3 kB.

`ObserveLimits::min_period_us` is one PWM period, so the 25 kHz tier can be observed at every step. Periods, the
timestamps and the budgets are in device time: with `time_scale` 0.01, `every:40us` arrives at 250 Hz real, and the
CLI converts `--budget` from real bytes per second. When a step can't keep up, the simulation falls behind instead of
catching up in a burst later; `telemetry.real_time_factor` says how fast it really runs.

The controller's gains come from the motor parameters, as VESC's motor detection would set them: current loops with
a 0.2 ms time constant, a 10 Hz speed loop. The PLL is faster than VESC's default (kp 2000, ki 30 000): the light
hobby outrunner accelerates at ~60 000 rad/s² electrical, which made that one slip.

## Measured

On a laptop (Ryzen AI 9 HX 370), release build, the VESC outrunner with a propeller and the built-in speed steps
running. Bytes are whole messages, serialized `Event`s, rebuilt on the host from what arrived; the framing (USB
packets, UDP datagrams) is shared by every message in a frame and left out on both sides. In-process and UDP give
the same numbers. `StreamData/s` is the same values sent as one `StreamData` event each, as `stream_data_ser()` sends a
property's change, without a timestamp.

```
$ motor_demo observe --compare
Observing sim time ×1; periods and budgets below are device (simulated) time, rates real time
  foc        21× every:10ms, budget 20.0 kB/s
  control    18× every:10ms, budget 20.0 kB/s
  telemetry  13× change:1ms, budget 20.0 kB/s
      t  level      updates/s  values/s     bytes/s   use  missed/s  StreamData/s  saving
   1.0s  foc              101      2118    11.8 kB/s   59%         0      20.4 kB/s     42%
   1.0s  control          101      1815     9.6 kB/s   48%         0      16.5 kB/s     42%
   1.0s  telemetry         51       311     2.2 kB/s   11%         0       2.9 kB/s     25%
```

| Case | Level | Updates/s | Values/s | Bytes/s | As StreamData | Saving |
|---|---|---|---|---|---|---|
| all 52, `foc` and `control` every 1 ms, `telemetry` every 20 ms, 200 kB/s each | `foc` | 1000 | 21 005 | 117.0 kB/s | 202.1 kB/s | 42 % |
| | `control` | 1000 | 18 005 | 95.0 kB/s | 164.0 kB/s | 42 % |
| | `telemetry` | 50 | 654 | 3.7 kB/s | 5.9 kB/s | 38 % |
| `foc` every PWM period, `time_scale` 0.01 | `foc` | 250 | 5 247 | 29.2 kB/s | 50.5 kB/s | 42 % |

Where the bytes go: a `PropertyUpdate` has a header of 10 bytes (sequence, result, the level's path, `t` as a `u32` of
microseconds; 4 bytes without `t`), then the update's presence header (a bit per slot, or a list of indices when
few are present) and the values. A current in mA as `I32` is 5 bytes, alone as `StreamData` 9 (10 for a resource
index above 15). An update of 21 such values takes 117 bytes, 5.6 per value.

`--time-scale 200` gives a real time factor of about 120 for the simulator and controller (one thread; ~140 with the
`f32` API, the conversion of every value to its quantity at 25 kHz costs the rest); above that, the device falls
behind and says so.

### A level short of budget

`--foc every:1ms --budget 30000` with 8 `foc` properties: 32 kB/s of values asked for, 30 kB/s granted, and
`poll_properties` called every 40 µs. The scheduler used to send whatever fit the bucket at each call, so a level
short of budget sent about one value per update, each with its own header. Now a level holds until all its due
properties fit, or its bucket is full (`ww_stream::observe`, SRV-18):

| | Updates/s | Values per update | Bytes/s | As StreamData | Missed/s |
|---|---|---|---|---|---|
| before | 1432 | 1.4 | 31.5 kB/s | 16.1 kB/s | 7280 |
| after | 666 | 8 | 31.0 kB/s | 44.0 kB/s | 2668 |

The same bytes now carry 2.7 times the values. What doesn't fit is reported in `missed`: 8000 due per second, 5331
sent. A property held back counts as missed only once it really falls a period behind, so polling often doesn't
inflate the count. Both rows were measured with the `f32` API; with quantities (a byte more per value) the same case
gives 565 updates/s of 8 values, 31.0 kB/s, 41.8 kB/s as `StreamData` and 3472 missed/s.

## Tests

`cargo nextest run -p motor_demo`:

- `tests/controller.rs`: the controller against the simulator without WireWeaver: current, speed (both
  directions) and duty control, the observer tracking the angle and running sensorless, hybrid start from
  standstill, every motor preset, the counters against the simulator's energy.
- `tests/observe.rs`: through the generated client, each tier at its rate, a budget below what is asked for, slow
  motion (the 25 kHz tier at 2.5 kHz real) and fast forward, UDP.
