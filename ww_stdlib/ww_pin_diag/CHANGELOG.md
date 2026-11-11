# Changelog

## Unreleased

### ⚠️ Breaking

- `PinDiag::set` and `read`'s `point` is `Index<TestPoint, u16>` (new counted marker of this crate, not named `Point`:
  that is the struct describing one entry of `POINTS`) instead of a bare `u16` (AGENTS.md "Numbers are typed").
  Wire-compatible (`Index` is exactly its `u16` on the wire).

### 🚀 Features

- `PinDiag` is `#[claim(exclusive)]` (SRV-34): a client can claim pin diagnostics through the hub. Wire compatible, only the introspection data changes.
- New crate (STD-47): `PinDiag` trait (`POINTS`, `set`, `read`, `release_all`), `Point`, `PointCaps`, `PointState`, `Drive`, `PinDiagError`. Page `docs/std_library/ww_pin_diag.md`, test pair `tests/pin_diag`.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.
