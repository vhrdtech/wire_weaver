---
level: basic
---

# ww_error

Errors every API can return (STD-16): `Unsupported`, `InUse`, `Busy`, ... look the same in every crate, generic host
code handles them without knowing the crate, and tools show them without knowing the crate either. A common error is
not part of any API's error enum: a handler returns it beside the API's own error, the protocol carries it as an
error event, and the client gets it as `CallError::Common` (see [Errors](../api/errors.md#common-errors)).

`ww_api` re-exports the crate: `ww_api::prelude` (and `wire_weaver::prelude`, which includes it) has `CommonError`
and `CommonErrorOwned` (with `std`), so an API crate needs no dependency of its own.

## `CommonError`

| Variant | Message (its doc line) | When |
|---|---|---|
| `Unsupported` | Not supported by this device (a mode, a range, a feature of the call). | The device knows the call, and can't do what was asked. |
| `InvalidArgument { arg: ww_si::Index<ww_si::units::Argument, u8>, reason }` | Argument number {arg} is invalid: {reason}. | `arg` counts from 0. |
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
fields filled in, without the final period: `In use by USART2`. `CommonError::DOCS` holds the names and doc lines
in discriminant order, and `name()` gives a value's variant name: tools print a common error from it without the
crate's definition.

## Beside an API's error enum

An API's error enum holds only what a tool or a caller should act on specifically (`Nack`, `UnsupportedSpeed` with
the range). The common errors need no variant, and an item that can only fail with a common error needs no `Result`:

```rust
use ww_api::prelude::*;

#[ww_trait]
trait Sensor {
    fn read() -> Result<u16, SensorError>;
    /// Fails only with common errors: `Busy` while a reading runs.
    #[errors(CommonError(Busy))]
    fn calibrate();
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
pub enum SensorError {
    /// The sensor did not answer.
    Nack,
}

ww_api::api_error!(SensorError);
```

`api_error!` gives `wire_weaver::Error<'_, SensorError>` a `From<SensorError>`, one line per error enum (and one for
its `Owned` type where the crate makes one, under `#[cfg(feature = "std")]`). A method declared `-> Result<T,
CommonError>` is refused when the trait is parsed: the common errors come with every item already.

### Device side

A handler of a method returning `Result<T, E>` returns `Result<T, wire_weaver::Error<'_, E>>`:
`Error::Common(CommonError)` or `Error::Api(E)`, and `?` takes both. A handler of a method without a `Result` returns
`T`, or `Result<T, CommonError<'_>>` when it can refuse:

```rust
fn claim(&mut self) -> Result<(), CommonError<'static>> {
    if self.uart_active {
        return Err(CommonError::InUse { by: MaxLen::new_unchecked("USART2") });
    }
    Ok(())
}

fn sample(&mut self) -> Result<u16, wire_weaver::Error<'static, SensorError>> {
    self.claim()?; // a CommonError
    if !self.acked {
        Err(SensorError::Nack)?; // the API's own error, through api_error!
    }
    Ok(self.adc.read())
}

fn read(
    &mut self,
    _cx: &mut Context<'_, impl BlockingEventOut>,
) -> RpcResult<Result<u16, wire_weaver::Error<'_, SensorError>>> {
    Ready(self.sample())
}

fn calibrate(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<Result<(), CommonError<'_>>> {
    Ready(if self.reading { Err(CommonError::Busy) } else { Ok(()) })
}
```

Properties follow the same rule: `GetResult<T, wire_weaver::Error<'_, E>>` and `SetResult<wire_weaver::Error<'_,
E>>` with an error type, `GetResult<T, CommonError<'_>>` and `SetResult<CommonError<'_>>` without one. A deferred
method answers with `<method>_send_return(out, reply_to, output)`, where `output` is the same `Result`. `ww api
scaffold` prints the signatures.

On the wire the API's own error is still the `Err` of the method's `Result` in the value; a common error is an error
event of the protocol (`ww_client_server::ErrorKind::Common`) at the item's handler site, the same event for a
method, a property, a stream function or a refused claim at the hub. The handler's `Ok` costs nothing new: the
reply is written through the same path as before.

### Host side

The generated client answers a method `-> Result<T, E>` with `Result<T, CallError<E>>`: `CallError::Device(E)` for
the API's own error, `CallError::Common(CommonErrorOwned)` for a common one, and `common()` on both call error types
gives the common error without a match:

```rust
fn is_retryable<E>(e: &CallError<E>) -> bool {
    matches!(e.common(), Some(CommonErrorOwned::Busy | CommonErrorOwned::Timeout))
}
```

A method without a `Result` answers `Result<T, wire_weaver_client::Error>`, and `Error::common()` holds the common
error the device refused with. The dynamic client, Python and `ww` read the same event, no crate's definition needed.

`tests/common_error` runs these through a generated server, the generated client and the dynamic client;
`wire_weaver_py`'s `test_errors.py` reads them from Python through the hub.

## Which errors a method returns

`#[errors(..)]` on a method lists the errors it actually returns, checked against the error enum when the trait is
parsed: an unknown or repeated name is a compile error pointing at that name, not the whole attribute (see
[Errors](../api/errors.md#which-errors-a-method-returns) for the full mechanics — the model, the client check, the
evolution checker). The common errors are one entry, `CommonError`: name just the cases that apply, or leave them
out for all of them:

```rust
#[errors(CommonError(Busy, NoPower, Timeout), NoSuchChannel)]
fn read(channel: u8) -> Result<Reading, AdcError>;

#[errors(CommonError)]
fn calibrate();
```

- `CommonError(Busy, NoPower, Timeout)` — only those three common errors, plus the crate's own `NoSuchChannel`.
- `CommonError` alone — any common error.
- A list without a `CommonError` entry says nothing about common errors: `#[errors(WrongAddress, Refused)]` lists
  the crate's own variants, and any common error passes the client's check.
- An item without an error type (`fn calibrate();`, a plain property) lists only common errors; its own names are
  a compile error.

### What it does

The list changes neither the type nor the wire: it is metadata in the API model, and from there:

- a `# Errors` section, one line per entry with the variant's own doc comment (its first line, without the final
  period; a common error's from `CommonError::DOCS`; `any common error` for `CommonError` alone) — on the trait's
  own rustdoc (`ww_api::api_doc`) and on the generated client's method;
- introspection: `ww api tree -t`, `ww introspect` and Python's `help()` show the same list next to the method;
- the evolution checker: a changed `#[errors(..)]` list is reported as a *compatible* change (nothing on the wire
  moves, and the device never checks it), but worded so the reviewer sees which variants a client of the old list
  would warn about;
- a check on the client: a device replying with an error outside the method's list logs a `tracing` warning
  (`'<resource>' returned <value>, which is not in its #[errors(..)]`), a common error against the list's
  `CommonError(..)` entry (`returned CommonError(Timeout)`).
  `wire_weaver_client::set_strict_error_lists(true)` turns it into a panic instead, for the whole process; the
  `tests_common` end-to-end harness turns it on, so a device that breaks its own contract fails in CI, not in the
  field.

A method without `#[errors(..)]` may return any variant of its error enum and any common error; nothing checks it,
and clients get no `# Errors` section for it.

The same list goes on the two other items that return errors, with the same meaning and outputs:

- a stream function (`#[stream(..)] fn`, API-31): the list is about the `Err` of its `Result` and the common errors
  the open can answer with (`ww_adc::AdcStream::samples`, `ww_i2c::I2cMonitor::records`);
- a `property!`: one list above the `property!`, about its error type if it has one. A property has one error type
  for reading and writing, so one list covers both (`ww_gpio::Pin::pull`, `ww_log::LogText::level`,
  `ww_user_info::UserInfo::name`).

```rust
#[errors(NoSuchChannel, CommonError(InvalidArgument, Unsupported, Busy, InUse, NoPower))]
#[stream(SampleBlock<'i>, counted)]
fn samples(config: StreamConfig<'i>) -> Result<StreamActual, AdcError>;

#[errors(TooLong, CommonError(Busy, Fault))]
property!(rw name: MaxLen<&'i str, MAX_NAME_LEN>, UserInfoError);
```

A plain `stream!`, a `sink!`, a `const` and `ww_impl!` have no error to list, `#[errors(..)]` on them is a compile
error.

### Real examples

`ww_adc::Adc::read` lists three common errors plus the crate's own `NoSuchChannel`:

```rust
#[errors(CommonError(Busy, NoPower, Timeout), NoSuchChannel)]
fn read(channel: u8) -> Result<Reading, AdcError>;
```

`ww_adc::AdcCapture::capture` lists five:

```rust
#[errors(
    CommonError(InvalidArgument, Unsupported, Busy, InUse, NoPower),
    NoSuchChannel
)]
fn capture(config: CaptureConfig<'i>) -> Result<CaptureActual, AdcError>;
```

`ww_memory::Memory::write_many` lists the crate's own variant and one common error:

```rust
#[errors(WrongAddress, CommonError(Refused))]
fn write_many(addr: u8, data: RefVec<'i, u8>) -> Result<(), MemoryError>;
```

`ww_log::LogText::level` has no error type: only common errors can be listed:

```rust
#[errors(CommonError(Unsupported))]
property!(rw level: LevelFilter);
```

## Tools

A common error reaches a tool as the protocol's error event, so no tool matches a crate or a type name; its text
comes from `CommonError`'s `Display`, its name and doc line from `CommonError::DOCS`.

- **Python**: `RemoteError.common` holds the common error (`{"InUse": {"by": "USART2"}}`, `"Busy"`) with `.value`
  `None`, and the message is its text (`In use by USART2`); an error of the API's own is in `.value` with `.common`
  `None`.
- **ww CLI**: `ww call` and the other commands print the common error's text; `ww api tree -t` shows the
  `#[errors(..)]` lists with `CommonError(..)` entries.
