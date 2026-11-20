# wire_weaver_client_benchmarks

Benchmarks of `wire_weaver_client`. `connect_bench` times the steps of a connect to blinky devices: the `tracing`
spans named in `wire_weaver_client::connect_spans`. The devices run in-process over simulated USB, many at once, or
on a USB board with the blinky firmware. It runs a new process with an empty cache, then one with this machine's
cache. It prints a table per round: round 1 is the process's first connect, later rounds are warm.

```sh
just connect-bench                                     # one in-process client, async
just connect-bench --clients 50 --calls 20             # 50 at once, 20 calls each
just connect-bench --transport usb --client blocking   # the blinky firmware on USB
just connect-bench --transport usb --client dynamic    # DynClient: takes the device's introspection data
```

The firmware: `examples_mcu/usb_stm32h725ig` (`blinky`, high-speed USB) or `examples_mcu/nucleo_g0b1re`
(`usb_blinky`, full-speed USB).
