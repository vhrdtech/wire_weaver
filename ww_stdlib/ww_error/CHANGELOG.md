## Unreleased

### ⚠️ Breaking

* `CommonError::InvalidArgument::arg` is `ww_si::Index<ww_si::units::Argument, u8>` instead of a bare `u8` (AGENTS.md
  "Numbers are typed"): same byte on the wire, so every API embedding `CommonError` through `#[common]` shows the
  new type in its model with no source change of its own; a caller that builds `InvalidArgument` by hand writes
  `Index::new(n)` instead of the bare number. Depends on `ww_si` (`counting`) now.

### 🚀 Features

* New crate (STD-16): `CommonError` (`Unsupported`, `InvalidArgument { arg, reason }`, `InUse { by }`, `Busy`,
  `WrongState { reason }`, `NoPower { rail }`, `Refused { reason }`, `Timeout`, `Fault { code }`) with bounded
  strings, `Display` and `core::error::Error` from each variant's doc line, and the `HasCommon` trait the
  `#[common]` variant attribute of `#[derive_shrink_wrap]` implements. Re-exported as
  `wire_weaver::prelude::CommonError`.
* Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.
