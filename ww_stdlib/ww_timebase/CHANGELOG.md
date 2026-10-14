## Unreleased

### 🚀 Features

- New crate (STD-17, skeleton): `Clock` with `now`, `info`, sync master / slave (IRIG-B, PPS), `sync_status`, a `time_map` stream mapping device time to shared time. Timestamps are `ww_si::Second`, no own time type.
