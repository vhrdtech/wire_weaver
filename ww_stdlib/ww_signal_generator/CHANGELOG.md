## Unreleased

### ⚠️ Breaking

- `PgError<'i>` and `WgError<'i>`: the first variant is `#[common] Common(CommonError<'i>)` (STD-16): `Unsupported`,
  `InvalidArgument`, `InUse`, `Busy`, `WrongState`, `NoPower`, `Refused`, `Timeout` come from `ww_error::CommonError`;
  `NoSuchChannel` stays. It is no longer `self_describing` (newer variants read as `Unknown(n)`), carries a lifetime
  (`PgError` / `WgErrorOwned` on hosts), has `Display` from its doc lines, and every method lists its variants with
  `#[errors(..)]`. Devices return `CommonError::Unsupported.into()` (or `?` a helper returning `CommonError`).
### 🚀 Features

- New crate (STD-22): `PatternGenerator` (digital channels from a pattern loaded through a `#[credit]` sink in
  `ww_logic_analyzer`'s chunk types, so captures replay as they are; once / repeated / forever, start now, on the
  external input or on the analyzer's trigger) and `WaveformGenerator` with `channel[]: WaveformChannel` (DAC:
  sine, square, triangle, sawtooth, DC, arbitrary tables in `ww_adc::SampleFormat`, levels as `ww_si::Volt`,
  phase-aligned `start_together`).
- Docs page `docs/std_library/ww_signal_generator.md` (STD-13).
