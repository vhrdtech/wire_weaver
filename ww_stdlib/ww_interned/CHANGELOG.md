# Changelog

## Unreleased

### ⚠️ Breaking

- `Interned::Id` is `ww_si::Index<InternedStr, UNib32>` instead of a bare `UNib32` (AGENTS.md "Numbers are typed"):
  same byte(s) on the wire, `InternedStr` a counted base of this crate's own declared with `ww_si::unit!`. Depends
  on `ww_si` (`counting`) now.

### 🚀 Features

- New crate (STD-48): `Interned { Id(UNib32), Text(&str) }` with `Display` (`#17` for an id not resolved).
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.
