# ww_user_info

A name and notes the user gives a unit, shown by `ww list`.

```rust
#[ww_trait]
pub trait UserInfo {
    const CAPABILITIES: UserInfoCaps;
    property!(rw name: MaxLen<&'i str, 32>, UserInfoError<'i>);
    #[optional]
    property!(rw notes: MaxLen<&'i str, 256>, UserInfoError<'i>);
}
```

`CAPABILITIES` (`name_max`, `notes_max`, `persistent`) says what this device keeps: a device may keep less than the
bounds of 32 and 256 bytes, and a device without room for notes leaves them out with `omit = ["user.notes"]`
(`notes_max` is then 0). It is a [`const` item](../api/properties.md#const-items), usually a Rust const in the
server's consts impl.

## Errors

A write returns `UserInfoError`: `Common(CommonError)` (see [ww_error](ww_error.md)) or `TooLong { max }` with what
the device keeps. `Common(Busy)` means storage can't be touched right now, `Common(Fault)` that saving failed. The
property syntax has no per-property `#[errors]` list yet, so the doc comment names the variants a device returns.

## Persistence

* When `persistent`, **a write is saved before the call answers**: `Ok` means it survives a power cut.
* A device writes its storage **only when the value changed**, so a host may write the same value again freely
  (flash wear).
* A failed write leaves the old value in place.
