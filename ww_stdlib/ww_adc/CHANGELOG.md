## Unreleased

### 🚀 Features

- New crate (STD-13 IO trait family, draft under review): `Adc` (channels with their `ww_si` unit and range, one
  calibrated reading), `AdcStream` (continuous raw sampling, configured with `configure_stream`, started and stopped
  with the stream's sideband `Open` / `Close`), `AdcCapture` (lossless triggered block capture into device RAM; a
  one-set capture is the simultaneous read).
