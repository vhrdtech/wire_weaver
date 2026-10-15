## Unreleased

### ⚠️ Breaking

- Reworked for the device contract (STD-26). Same crate version: never published, nothing used it.
    - `LogDefmt`: `stream!(data)`, `property!(ro format)`, `counters()` and `reset_counters()` go. Now
      `const DEFMT: DefmtInfo` (`encoding: DefmtEncoding`, `build_id` of the ELF with the format strings) and
      `#[stream(DefmtFrames, reliable)] fn frames() -> Result<(), LogError>`: whole defmt frames, unit one frame, so
      `lost` is the exact number of dropped log messages and logs from before the host connects arrive.
      `DefmtFormat` is renamed `DefmtEncoding`, `DefmtCounters` goes (the stream's status counts).
    - `LogText`: `stream!(message)`, `enable_level` / `enabled_levels` and the counters go. Now
      `#[stream(LogRecord, reliable)] fn records()` and `property!(rw level: LevelFilter, LogError)`.
      `Message` becomes `LogRecord` (`severity`, `target` up to 48 bytes, `message` up to 256 bytes, `truncated`),
      timestamps move to the stream head's `t`.
    - `Severity::User` goes, `Severity` is `no_future_version` (`Unknown` goes too).
    - New `LogError` with `#[common] Common(CommonError)`; `ww_si` is no longer a dependency.
