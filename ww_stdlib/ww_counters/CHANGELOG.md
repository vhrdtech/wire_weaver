## Unreleased

### ⚠️ Breaking

- `Counters` carries the raw [cnt](https://crates.io/crates/cnt) buffer words, decoded on the host with the
  firmware's ELF (STD-25): `const INFO: CountersInfo` (format version, `ram` / `backup` `BufferInfo { used_words,
  capacity_words }`, `build_id`), `read(buffer: CounterBuffer, offset) -> Result<MaxLen<RefVec<UVlq32>, 64>,
  CountersError>`, `reset(buffer)`, and an optional counted stream `watch(period)` of `CounterChanges`. `len`,
  `value`, `values`, `values_compressed`, `values_changed`, `metadata`, `kind` and the types `CounterValue`,
  `CountersKind`, `CounterMeta`, `CountersFilter` are gone, and so is the dependency on `ww_log_bare_metal`: names,
  types, units and severities are only in the ELF. A device serves `INFO` from a `fn` (`const_model = "INFO=fn"`).
- `CounterValue`, `CountersKind`, `CountersFilter` got `Unknown(u32)` from `derive_shrink_wrap` (SW-47) before
  they were removed.

### 🚀 Features

- `Counters::watch` lists its errors with `#[errors(..)]` (API-32), now that stream functions and properties can carry
  one, instead of a hand-written `# Errors` note: rustdoc, introspection and the client's warning about an unlisted
  error see it.
- `read_words` and `reset_words` serve `read` / `reset` from a `cnt` buffer (`cnt::counters_ram_buffer()`),
  copying one page into a scratch at once; `BufferInfo::new` for `INFO`.
- Feature `host`: `host::WwSource`, a device's counters as a `cnt_core::Source` over a `CountersDevice` (the two
  calls on a generated client), with `check` against the ELF (format version, used words), so `cnt_core` decodes
  them and `cnt_view` shows them (`ww counters`).
