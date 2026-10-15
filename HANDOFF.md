# Handoff: EX-11 motor-observe-demo

## Done (committed on s/15oct2026-motor-observe-demo)
- examples/motor_demo_api: 52 +observe props in foc (25 kHz) / control (1 kHz) / telemetry (50 Hz), sim level.
- examples/motor_demo: controller.rs (VESC-like FOC, tested vs motor_sim), device.rs (sim-time device, in-process
  and UDP), stats.rs (bytes from the device's own <Level>Update types), host.rs (observe sessions), args.rs, CLI.
- motor_sim: public, https git dep pinned by rev in root [workspace.dependencies].

## Next
1. SRV-18 scheduler fix in ww_stream::observe (parent asked, own commit fix(stream)): hold when not all due fit and
   bucket not full; a hold within one period/min isn't a miss; PropertyUpdate.t as Second{Micro, U32} like stream
   heads. Measure before/after with `motor_demo observe --foc every:1ms --budget 30000 --prop
   foc.i_a,foc.i_b,foc.i_c,i_d,i_q,v_d,v_q,phase --compare` (before: 1432 upd/s, 1.4 values/upd, 31.5 kB/s vs
   16.1 kB/s StreamData).
2. docs/examples/motor_demo.md with measured numbers + zensical nav; CHANGELOGs (root [Unreleased], ww_stream,
   wire_weaver_cli for clap_complete? no user-visible change), FEATURES EX-11 ✅; just test, just lint, build-docs.

## Gotchas
- Device clock = simulated µs; budgets are per simulated second (CLI converts --budget by time scale).
- derive_shrink_wrap eats #[default]: API enums have no Default derive; evolvable enums have Unknown(_).
