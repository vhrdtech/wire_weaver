# counted_stream_api

Framework-only fixture for the counted-stream dispatch bench (R10, `examples_mcu/mcu_qemu` `qemu-counted-stream`): a 2-level indexed resource (`group[]` -> `item[]`), each item with one counted event stream.

Mirrors the shape `ww_gpio::Bank`/`Pin` had before the bench moved onto a framework-only fixture; API-20's `qemu-global-paths` is the sibling bench for global trait paths.
