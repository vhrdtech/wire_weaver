# sensor_packet

A weather station's sensor node and its gateway on a radio link, with `shrink_wrap` alone: no API trait and no
framework. The node writes a `Reading` without an allocator; the gateway reads it back borrowed or owned. `v2` is the
next firmware, with a field and an enum variant more, and both versions keep reading each other's bytes.

```sh
cargo run -p sensor_packet --example sensor_packet_radio
```

Depends on `shrink_wrap` and `ww_si` only; the `std` feature (default) adds the owned types. The book's chapters
"Your data on the wire" use it, and `tests/it/book.rs` writes the bytes they show into `docs/generated/sensor_packet/`.
