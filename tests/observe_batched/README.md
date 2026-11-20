# observe_batched

Batched observe end to end (API-39): `+observe+batched` properties observed in `Samples` mode, every sample
the device takes (or one per `every_us`) sent in batches, next to plain ones, with a level's samples sharing
one time column (`sample_times`). The device runs the generated async server polled with a fake clock; the
host runs the real event loop (`tests_common`).
