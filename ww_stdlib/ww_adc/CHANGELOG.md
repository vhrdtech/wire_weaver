## Unreleased

### 🚀 Features

- New crate (STD-13 IO trait family, draft under review): `Adc` (channels with their `ww_si` unit and range, one
  calibrated reading), `AdcStream` (continuous raw sampling with the `seq` / `lost` / `t` stream head), `AdcCapture`
  (lossless triggered block capture into device RAM; a one-set capture is the simultaneous read).
