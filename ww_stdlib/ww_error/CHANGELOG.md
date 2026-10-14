## Unreleased

### 🚀 Features

* New crate (STD-16): `CommonError` (`Unsupported`, `InvalidArgument { arg, reason }`, `InUse { by }`, `Busy`,
  `WrongState { reason }`, `NoPower { rail }`, `Refused { reason }`, `Timeout`, `Fault { code }`) with bounded
  strings, `Display` and `core::error::Error` from each variant's doc line, and the `HasCommon` trait the
  `#[common]` variant attribute of `#[derive_shrink_wrap]` implements. Re-exported as
  `wire_weaver::prelude::CommonError`.
