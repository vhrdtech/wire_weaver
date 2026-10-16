# ww_defmt_logger changelog

## Unreleased

### 🐛 Bug Fixes

- The RTT tee never blocks (STD-26, first hardware user Donguru API-10): a probe that set the channel to blocking on
  attach (`probe-rs` does) made a burst of logging stall the firmware inside the logger's critical section until it
  read, and a full channel with no reader would stall it for good. Each frame now goes to the channel in one write
  with the mode forced to `NoBlockSkip`, whole or skipped and counted (`rtt_skipped()`), so the host never decodes
  half a frame either. 512 bytes more RAM with the `rtt` feature.

### 🚀 Features

- First version (STD-26): the `defmt` global logger of a WireWeaver device. `global_logger!(NAME, SIZE)` makes a
  `DefmtLogger<N>` the `#[defmt::global_logger]`: frames go into a lock-free ring (critical section on the producer
  side only, whole frames committed at once), are teed to an `rtt-target` up channel with the `rtt` feature
  (`set_rtt_channel`), and `pump(&mut Reliable<S>)` moves whole frames from the event loop into the
  `ww_log::LogDefmt::frames` stream's store, up to 512 bytes per item, `units` = frames. Frames the ring
  dropped go into the stream's `lost` in their place. `encoding()` for `LogDefmt::DEFMT`, `dropped_frames()`,
  `pending_bytes()`.
