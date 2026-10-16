## Unreleased

### 🐛 Fixes

- `Year` is a derived struct of one `UNib32` instead of a hand-written serdes over `i32`: the API model described it as an unsized struct of an `i32`, so dynamic clients (`ww introspect`, Python) failed to decode any `DateTime` (`<OutOfBoundsReadRawSlice>` on `FirmwareInfo::FIRMWARE`). `Year` rejects out of range values when read (`Error::SubtypeOutOfRange`, also a `UNib32` above 20 bits), and `Year::year` can't overflow. Negative years now round trip (the old code encoded 19 bits and tested bit 19 for the sign). Wire bytes for years 2025.. are unchanged; `Year::new` and `Year::year` are `const fn`. The model (and so the signature) of `DateTime` changed: a firmware built with an earlier revision of this unpublished version needs a rebuild (reflash) to introspect with this host.

### 🚀 Features

- `DateTime::from_ymd_hms_utc_opt` and `NaiveDate::from_ymd_opt` are `const fn`, for dates in `const` items.

