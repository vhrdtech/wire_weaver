# Handoff: ww-io-traits (STD-13 io-trait-family, task P2401#14oct-1548)

Done:
- Design doc `docs/std_library/io_family.md` (in nav), FEATURES STD-13 item. Waiting for Roman's review of D1-D10.

Next (after review):
1. Skeleton commit in this session: all eight crates compiling no_std + std, `ww_timebase` complete, stream
   conventions in `docs/api/streams.md`. Check the risks listed in the doc (u7 in enums, nested MaxLen RefVec,
   credit sink via ww_impl!, tuples).
2. Claim STD ids per crate, file sonnet tasks under P2401, spawn fill-ins with `--from-here`.

Decisions/gotchas:
- Parent api3-design holds STD-13..15; STD-13 is this family.
- Family release waits for SW-16 future-version (ww-evolution session, WW-3).
