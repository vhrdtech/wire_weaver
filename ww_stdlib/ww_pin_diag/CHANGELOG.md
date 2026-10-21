# Changelog

## Unreleased

### 🚀 Features

- `PinDiag` is `#[claim(exclusive)]` (SRV-34): a client can claim pin diagnostics through the hub. Wire compatible, only the introspection data changes.
- New crate (STD-47): `PinDiag` trait (`POINTS`, `set`, `read`, `release_all`), `Point`, `PointCaps`, `PointState`, `Drive`, `PinDiagError`. Page `docs/std_library/ww_pin_diag.md`, test pair `tests/pin_diag`.
