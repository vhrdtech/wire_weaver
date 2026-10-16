---
level: advanced
---

# Motor controller demo

A VESC-like field oriented controller drives a simulated motor, and its ~50 variables are
[observed](../api/observing.md) by a host, each at the rate it asks for, within a byte budget per trait level. It shows what observing many fast-changing properties costs on the link, and what it would cost as one
`StreamData` event per value.

- `examples/motor_demo_api`: the API, 52 `+observe` properties in three trait levels, one per rate tier.
- `examples/motor_demo`: the controller, the device (in this process or on a UDP socket), the host side counting
  and the `motor_demo` CLI.
- The motor, inverter, mechanics and sensors are simulated by [motor_sim](https://github.com/vhrdtech/motor_sim) (a
  PMSM in dq, solved exactly per PWM period, with ADC noise, offsets and quantisation, an encoder and seeded faults).

```sh
cargo run --release -p motor_demo -- observe --compare        # a device in this process, 3 tiers, 1 s reports
cargo run --release -p motor_demo -- device --listen 127.0.0.1:9000
cargo run --release -p motor_demo -- observe --udp 127.0.0.1:9000 --prop i_q=every:1ms,erpm,fault
cargo run --release -p motor_demo -- list                     # every property with its unit
source <(COMPLETE=bash motor_demo)                             # completion of commands and property names
```

## The API: one trait level per rate tier

The VESC firmware (vedderb/bldc) computes its variables in three tiers, and so does this controller:

| Level | Rate | Properties | What |
|---|---|---|---|
| `foc` | 25 kHz, every PWM period | 21 | phase, αβ and dq currents, dq and αβ voltages, duties, the angle in use, the flux observer's and the encoder's angles, observer state, bus voltage, the true angle |
| `control` | 1 kHz | 18 | setpoints (`mode`, `sensor_mode`, duty, current, erpm, all `rw`), PLL speed, current setpoints, filtered currents, input current, duty, torque, position, observer error, the true speed |
| `telemetry` | 50 Hz | 13 | time, Ah and Wh drawn and charged, tachometer, input voltage and power, averaged currents, `fault`, the real time factor |
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
    /// Phase a current as measured (offset calibrated), A.
    property!(ro+observe i_a: f32);
    // ..
}
```

Each level is one observe subscription, so whatever is due in a level goes out in one `PropertyUpdate`, and a host
asks for each tier at its own rate: `foc` at 1 kHz for a scope, `control` on change, `telemetry` at 10 Hz for a
dashboard.

## The device

The device's clock is simulated time, one PWM period (40 µs) per step; simulated time follows real time times
`sim.time_scale`. Every step:

1. the controller runs on the sensor readings (current offsets measured during the first 0.1 s, Clarke and Park,
   the Ortega flux observer and a PLL, d/q current PI loops with decoupling, space vector PWM; the speed loop and the
   filters every 25th step, the counters every 500th), the simulator steps with the duties it gives;
2. every value of a tier that ran and changed is marked, `mark_changed_<name>()`, so `change:` observers get it;
3. `poll_properties(out, (), now_us, buf)` sends what is due, with `now_us` the simulated microseconds.

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
   1.0s  foc              101      2119     9.7 kB/s   48%         0      18.3 kB/s     47%
   1.0s  control          101      1816     8.0 kB/s   40%         0      14.9 kB/s     47%
   1.0s  telemetry         51       520     2.7 kB/s   13%         0       4.3 kB/s     37%
```

| Case | Level | Updates/s | Values/s | Bytes/s | As StreamData | Saving |
|---|---|---|---|---|---|---|
| all 52, `foc` and `control` every 1 ms, `telemetry` every 20 ms, 200 kB/s each | `foc` | 1000 | 21 007 | 96.0 kB/s | 181.1 kB/s | 47 % |
| | `control` | 1000 | 18 006 | 79.0 kB/s | 148.0 kB/s | 47 % |
| | `telemetry` | 50 | 654 | 3.1 kB/s | 5.3 kB/s | 42 % |
| `foc` every PWM period, `time_scale` 0.01 | `foc` | 250 | 5 241 | 24.0 kB/s | 45.2 kB/s | 47 % |

Where the bytes go: a `PropertyUpdate` has a header of 10 bytes (sequence, result, the level's path, `t` as a `u32` of
microseconds; 4 bytes without `t`), then the update's presence header (a bit per slot, or a list of indices when
few are present) and the values. One `f32` alone as `StreamData` takes 8 bytes (9 for a resource index above 15).
An update of 21 `f32` takes 96 bytes, 4.6 per value.

`--time-scale 200` gave a real time factor of 80 for the simulator and controller (one thread); above that, the
device falls behind and says so.

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
inflate the count.

## Tests

`cargo nextest run -p motor_demo`:

- `tests/controller.rs`: the controller against the simulator without wire_weaver: current, speed (both
  directions) and duty control, the observer tracking the angle and running sensorless, hybrid start from
  standstill, every motor preset, the counters against the simulator's energy.
- `tests/observe.rs`: through the generated client, each tier at its rate, a budget below what is asked for, slow
  motion (the 25 kHz tier at 2.5 kHz real) and fast forward, UDP.
