# ww_error

Errors every API can return (STD-16): `Unsupported`, `InUse`, `Busy`, ... look the same in every crate, generic host
code handles them without knowing the crate, and tools recognise them by their type.
An API crate nests them as one `#[common] Common(CommonError)` variant of its own error enum (see below).

`wire_weaver` re-exports the crate: `wire_weaver::prelude` has `CommonError`, `CommonErrorOwned` (with `std`) and
`HasCommon`, so an API crate needs no dependency of its own.

## `CommonError`

| Variant | Message (its doc line) | When |
|---|---|---|
| `Unsupported` | Not supported by this device (a mode, a range, a feature of the call). | The device knows the call, and can't do what was asked. |
| `InvalidArgument { arg: u8, reason }` | Argument number {arg} is invalid: {reason}. | `arg` counts from 0. |
| `InUse { by }` | In use by {by}. | A resource another function holds (`"USART2"`). |
| `Busy` | Busy with another operation, try again later. | |
| `WrongState { reason }` | Not possible in the current state: {reason}. | |
| `NoPower { rail }` | No power on {rail}. | A missing supply or IO level (`"VIO"`). |
| `Refused { reason }` | Refused: {reason}. | Possible, but the device won't (locked, a policy). |
| `Timeout` | Timed out. | |
| `Fault { code: u32 }` | Hardware fault {code:#x}. | Device-specific code. |

Strings are bounded (`reason` 64 bytes, `by` 32, `rail` 16) and may be empty: a device that saves flash sends `""`,
and the message leaves the part out (`Refused`). `CommonError` is an `Unsized` enum, so it ends in `Unknown(u32)`
(SW-47): a variant added in a newer `ww_error` reaches an older host as `Unknown(n)`, not as a decode error.

`Unsupported` is not the framework's `Unimplemented`: that one means the device has no implementation at all (a
handler returning `Unimplemented`, an item left out), and stays a framework error.

`Display` and `core::error::Error` are implemented for both types, the message is the variant's doc line with its
fields filled in, without the final period: `In use by USART2`.

## In an API's error enum

Put `CommonError` first, marked `#[common]`, and keep the crate's own variants for what a tool or a caller should act
on specifically (`Nack`, `UnsupportedSpeed` with the range):

```rust
use wire_weaver::prelude::*;

#[ww_trait]
trait Sensor {
    fn read() -> Result<u16, SensorError<'i>>;
    /// An error enum of its own isn't needed when only common errors can happen.
    fn calibrate() -> Result<(), CommonError<'i>>;
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
pub enum SensorError<'i> {
    #[common]
    Common(CommonError<'i>),
    /// The sensor did not answer.
    Nack,
}
```

`#[common]` makes `#[derive_shrink_wrap]` add, for the borrowed and the owned type:

- `From<CommonError<'i>> for SensorError<'i>` (`From<CommonErrorOwned> for SensorErrorOwned`): a helper returns
  `CommonError` and is `?`-ed into the error of any crate.
- `HasCommon` with `fn common(&self) -> Option<&CommonError>`: generic code gets the common part of any crate's error.

One variant per enum can be `#[common]`, and it has exactly one unnamed field. On the wire a common error takes two
discriminants (`Common`, then the `CommonError` variant), 4 bits more than a variant of the crate's own.

### Device side

```rust
fn claim(&mut self) -> Result<(), CommonError<'static>> {
    if self.uart_active {
        return Err(CommonError::InUse { by: MaxLen::new_unchecked("USART2") });
    }
    Ok(())
}

fn sample(&mut self) -> Result<u16, SensorError<'static>> {
    self.claim()?; // CommonError into SensorError
    Ok(self.adc.read())
}

fn read(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<Result<u16, SensorError<'_>>> {
    Ready(self.sample())
}
```

### Host side

```rust
fn is_retryable<E: HasCommon<Common = CommonErrorOwned>>(e: &E) -> bool {
    matches!(e.common(), Some(CommonErrorOwned::Busy | CommonErrorOwned::Timeout))
}
```

`tests/common_error` runs these through a generated server, the generated client and the dynamic client.

## Tools

Tools recognise `ww_error::CommonError` by its type in the API model (crate `ww_error`, any version), also when a
device left its definition out because it is known from the snapshots (`ww_self::TypeOwned::is_common_error`,
`ValueOwned::common_error`).

- **Python**: `RemoteError.common` holds the common error (`{"InUse": {"by": "USART2"}}`, `"Busy"`), `None` for an
  error of the API's own; `.value` the whole error as before. The message is the doc line:
  `read() returned an error: In use by USART2`.
- **ww CLI**: `ww api tree -t` and `ww introspect -t` mark the type and the variants holding it as `common errors`.
