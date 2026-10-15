## Unreleased

### ⚠️ Breaking

- `OneWireConfig` gets `io_level: Option<Volt>` and `OneWire::configure` answers the actual `OneWireConfig`
  instead of `()` (STD-13); `OneWireCapabilities` lists `io_levels`, `io_level_min`, `io_level_max`.

### 🚀 Features

- New crate (STD-20): `OneWire` with addressed transactions and strong pull-up, reset, paged ROM search, bit-level `triplet`. Full rustdoc, docs page (`docs/std_library/ww_onewire.md`), API snapshot, and an in-process test (`tests/io_onewire`) with a mock bus running the real Dallas/Maxim search algorithm.
