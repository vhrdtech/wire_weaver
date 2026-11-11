## Unreleased

### ⚠️ Breaking

- Calendar fields are typed and bounded (EVO-58, wire line `w1.0` -> `w2.0`, same bytes): `NaiveDate::month` is `IndexFrom1<Month, U4, Range<1, 12>>`, `day` `IndexFrom1<Day, U5, Range<1, 31>>`, `NaiveTime::secs` `Fixed<Second, One, U17, Range<0, 86_399>>`, `frac` `Option<Fixed<Second, Nano, U31, Range<0, 1_999_999_999>>>` and `OtherTimezone::FixedOffset::secs` `Fixed<Second, One, I18, Range<-86_399, 86_399>>`, from `exact_units` (a new dependency, features `shrink_wrap`, `time`, `counting`). Out of range values are refused when read (`SubtypeOutOfRange`). Read a field with `.get().value()` instead of `.value()`. Types embedding `DateTime` need a new wire line as well.

### 🐛 Fixes

- `Year` is a derived struct of one `UNib32` instead of a hand-written serdes over `i32`: the API model described it as an unsized struct of an `i32`, so dynamic clients (`ww introspect`, Python) failed to decode any `DateTime` (`<OutOfBoundsReadRawSlice>` on `FirmwareInfo::FIRMWARE`). `Year` rejects out of range values when read (`Error::SubtypeOutOfRange`, also a `UNib32` above 20 bits), and `Year::year` can't overflow. Negative years now round trip (the old code encoded 19 bits and tested bit 19 for the sign). Wire bytes for years 2025.. are unchanged; `Year::new` and `Year::year` are `const fn`. The model (and so the signature) of `DateTime` changed: a firmware built with an earlier revision of this unpublished version needs a rebuild (reflash) to introspect with this host.

### 🚀 Features

- `DateTime::from_ymd_hms_utc_opt` and `NaiveDate::from_ymd_opt` are `const fn`, for dates in `const` items.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

