---
level: basic
---

# ww_date_time

`ww_date_time` gives device APIs a compact ISO 8601 date and time, nibble-packed instead of a fixed-width
timestamp: `DateTime`, built from a `NaiveDate`, a `NaiveTime`, and an optional `Timezone`. It is `no_std` by
default and depends only on `shrink_wrap`; an optional `chrono` feature adds conversions to and from the `chrono`
crate's own date/time types for host-side code.

Add it to a device API crate with:

```toml
[dependencies]
ww_date_time = "0.1"
```

## Types

* `DateTime` - `date: NaiveDate`, `time: NaiveTime`, `timezone: Timezone`; `self_describing`.
* `NaiveDate` - an ISO 8601 calendar date without timezone: `year: Year`, `month: U4` (1-12), `day: U5` (1-31);
  `self_describing`.
* `Year` - a signed year, `-262_142..=262_141`, stored as a `UNib32` shifted by 2025 so that years close to now take
  the fewest nibbles; a year before 2025 is masked into the low 19 bits instead, so it always costs the same extra
  nibbles regardless of how far back it is (see [sizes](#sizes-on-the-wire) below). `Year::new` constructs one
  (bounds-checked), `Year::year()` reads it back as `i32`.
* `NaiveTime` - an ISO 8601 time without timezone: `secs: U17` (seconds since midnight, 0-86399), `frac: Option<U31>`
  (sub-second part in nanoseconds, omitted entirely when zero); `self_describing`.
* `Timezone` - `UTC` (preferred, costs only 1 bit) or `Other(OtherTimezone)`; `self_describing`, `ww_repr = u1`.
* `OtherTimezone` - `Naive` (no timezone information at all) or `FixedOffset { secs: I18 }` (offset from UTC in
  seconds); `self_describing`, `ww_repr = u3` leaves room to add up to 6 more kinds later without breaking
  compatibility.

All of the above derive `Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash` (`Timezone`/`OtherTimezone` also
`Debug`), and get owned (`*Owned`) counterparts when the `std` feature is enabled.

## Sizes on the wire

`DateTime`'s size depends on how far the year is from 2025 and on which fields are present, since `Year`, the
`Option<U31>` fraction and the `Timezone` enum are all self-describing rather than fixed-width:

| Year range          | UTC, no nanoseconds | naive/fixed-offset, no nanoseconds | UTC, with nanoseconds | naive/fixed-offset, with nanoseconds |
|----------------------|:---:|:---:|:---:|:---:|
| 2025-2032             | 32 bits (minimum) | 53 bits | 63 bits | 84 bits (maximum) |
| 2033-2088 and so on  | +4 bits per range | +4 bits per range | +4 bits per range | +4 bits per range |
| before 2025           | +24 bits | +24 bits | +24 bits | +24 bits |

`NaiveDate` alone is 13 bits minimum (2025-2032), 17 bits for 2033-2088, and up to 37 bits for the widest years.
`NaiveTime` alone is 18 bits without a fractional part, 49 bits with one.

A real `DateTime` for 2025-05-30 16:20:00 UTC (no fractional seconds) serializes to exactly 4 bytes (`05 F3 96 C0`),
matching the 32-bit minimum above - `ww_date_time`'s own test suite checks this against a real `ser_shrink_wrap()`
call.

## Construction

```rust
use ww_date_time::{DateTime, Timezone};

let utc = DateTime::from_ymd_hms_utc_opt(2025, 5, 30, 14, 0, 0, 0).unwrap();
assert_eq!(utc.timezone, Timezone::UTC);

let naive = DateTime::from_ymd_hms_naive_opt(2025, 5, 30, 14, 0, 0, 0).unwrap();
```

`from_ymd_hms_utc_opt` is a `const fn` (so it can build a `DateTime` in a `const` item, e.g. a compile-time build
timestamp), `from_ymd_hms_naive_opt` is not, since it always picks `Timezone::Other(OtherTimezone::Naive)`. Both
return `None` on an out-of-range month, day, hour, minute, second or nanosecond, same as `NaiveDate::from_ymd_opt`
and `NaiveTime::from_hms_opt` they build on.

`Debug` is implemented by hand for compact, human-readable output rather than deriving it: a `NaiveDate` prints as
`2025-M5-D30`, a `NaiveTime` as `14:0:0` (`.123` appended when a fractional part is present), and `DateTime` as the
two joined with the timezone.

## `chrono` interop

With the `chrono` feature (on by default), `ww_date_time` re-exports the `chrono` crate and adds `From`/`Into`
conversions in both directions: `chrono::NaiveDate` ↔ `NaiveDate`, `chrono::NaiveTime` ↔ `NaiveTime`,
`chrono::NaiveDateTime` ↔ `DateTime` (as `Timezone::Other(OtherTimezone::Naive)`), and
`chrono::DateTime<Utc>` / `chrono::DateTime<FixedOffset>` → `DateTime`. This is what lets host-side code build a
`DateTime` from `chrono::Utc::now()` without touching the wire representation directly.

## Features

| Feature  | What                                                                          |
|----------|--------------------------------------------------------------------------------|
| `std`    | (default) owned (`*Owned`) type variants, via `shrink_wrap/std`                |
| `chrono` | (default) the `chrono` interop above                                           |
| `defmt`  | `defmt::Format` on the borrowed types, via `shrink_wrap/defmt`                 |
