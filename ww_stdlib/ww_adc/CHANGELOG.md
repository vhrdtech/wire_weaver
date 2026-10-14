## Unreleased

### 🚀 Features

- New crate (STD-13 IO trait family, STD-18 fill-in): `Adc` (channels with their `ww_si` unit and range, one
  calibrated reading), `AdcStream` (continuous raw sampling, configured with `configure_stream`, started and stopped
  with the stream's sideband `Open` / `Close`), `AdcCapture` (lossless triggered block capture into device RAM; a
  one-set capture is the simultaneous read). Full rustdoc on every trait and type; docs page
  `docs/std_library/ww_adc.md`; end-to-end test pair `tests/io_adc` / `tests/io_adc_api` with a mock server
  exercising every trait (busy/error paths, stream open/receive/close, polled capture trigger progression).
