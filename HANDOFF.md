# Handoff: ww-io-traits (STD-13 io-trait-family, task P2401#14oct-1548)

Done:
- Design doc `docs/std_library/io_family.md` (in nav), FEATURES STD-13 item. Waiting for Roman's review of D1-D10.

Next (after review):
1. Skeleton commit in this session: all eight crates compiling no_std + std, `ww_timebase` complete, stream
   conventions in `docs/api/streams.md`. Check the risks listed in the doc (u7 in enums, nested MaxLen RefVec,
   credit sink via ww_impl!, tuples).
2. Claim STD ids per crate, file sonnet tasks under P2401, spawn fill-ins with `--from-here`.

Decisions/gotchas:
- Roman (14 Oct): evolve ww_gpio / ww_uart / ww_i2c from 0.1, keep what works; the doc has a kept/changed/added
  table per crate. ww_gpio 0.2 is additive on the wire (only Error variants new).
- SW-16 shape (ww-evolution): last variant `#[future_version] Unknown(u32)`, opt-in.
- Global-trait client (ww-cross-version): DynClient::traits_only + global_attachment; trait must not sit in an array.
- Parent api3-design holds STD-13..15; STD-13 is this family.
- Family release waits for SW-16 future-version (ww-evolution session, WW-3).
