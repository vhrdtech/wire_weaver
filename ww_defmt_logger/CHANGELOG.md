# ww_defmt_logger changelog

## Unreleased

### 🚀 Features

- First version (STD-26): the `defmt` global logger of a WireWeaver device. `global_logger!(NAME, SIZE)` makes a
  `DefmtLogger<N>` the `#[defmt::global_logger]`: frames go into a lock-free ring (critical section on the producer
  side only, whole frames committed at once), are teed to an `rtt-target` up channel with the `rtt` feature
  (`set_rtt_channel`), and `pump(&mut Reliable<S>)` moves whole frames from the event loop into the
  `ww_log_bare_metal::LogDefmt::frames` stream's store, up to 512 bytes per item, `units` = frames. Frames the ring
  dropped go into the stream's `lost` in their place. `encoding()` for `LogDefmt::DEFMT`, `dropped_frames()`,
  `pending_bytes()`.
