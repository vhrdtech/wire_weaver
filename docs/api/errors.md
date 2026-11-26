---
level: advanced
---

# Errors in depth

[Errors](../basic/errors.md) declares an API's own error enum, returns it and a common error from a handler, and
handles both on the host. [Common errors](../std_library/ww_error.md) lists the errors every device shares. This page
covers the rest: the list of errors each item returns, error crates and retries on the host, `Unsupported` against
`Unimplemented`, where on the device a framework error came from, errors from devices of other versions, and items a
device leaves out.

## Declaring errors in an API

A method that can fail returns `Result<T, E>`, where `E` is the error enum of its crate:

```rust
#[ww_trait]
pub trait I2c {
    /// Writes `data` to the device at `addr`.
    #[errors(CommonError, Nack, Arbitration, Timeout, BufferTooBig)]
    #[effect(outbound)]
    fn i2c_write(addr: u8, data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>) -> Result<(), I2cError>;
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), display, ww_repr = unib32)]
pub enum I2cError {
    /// The address or a data byte was not acknowledged.
    Nack,
    /// Arbitration lost to another master.
    Arbitration,
    /// The bus stayed busy longer than the timeout.
    Timeout,
    /// The transfer is longer than the device can buffer ({max} bytes).
    BufferTooBig { max: u16 },
}

ww_api::api_error!(I2cError);
```

Use **one error enum per crate** (or per group of traits, when a crate's traits really fail for unrelated reasons).
The same type is used on the wire, in the device's handlers and in the client, so device code can share helpers and
`?` them anywhere, and a driver crate converts it into its own error with a single `#[from]`. `api_error!` lets a
handler `?` the enum into `wire_weaver::Error<'_, E>`, the type every handler of a `Result` method returns (see
[Common errors](#common-errors)). A method that can only fail with common errors needs no `Result` at all.

### Which errors a method returns

`#[errors(..)]` lists the errors a method can actually return: the variants of its error enum, and the common
errors as one `CommonError(InUse, Refused)` entry; `CommonError` alone means any common error. The list is checked
when the trait is parsed, so an unknown or repeated name is a compile error on that name.

The list doesn't change the type or the wire; it goes into the API model, and from there:

- into the docs, as an `# Errors` section with each variant's doc line, both in the generated client and on the
  trait's rustdoc;
- into introspection, so `ww api tree` and Python's `help()` show it;
- into the evolution checker: a changed list is reported as a compatible change;
- into a check on the client: when a device returns a variant outside the method's list, the generated and the
  dynamic client log a `tracing` warning, so a device that doesn't keep its contract is noticed early.
  `wire_weaver_client::set_strict_error_lists(true)` makes it a panic instead; the test harness
  (`tests_common`) turns that on.

A method without `#[errors]` may return any variant of its error enum and any common error. A list without a
`CommonError` entry says nothing about common errors.

Stream functions and properties take the same list, with the same outputs, also when they have no error type of
their own (then only common errors can be listed). On a stream function (`#[stream(..)] fn`, API-31) it is about
the `Err` of its `Result` and the common errors the open answers with; the client checks the open's result. Above a
`property!` with an error type (`property!(rw mode: Mode, ModeError)`) it is about that type: a property has one
error type for reading and writing, so one list covers both. The generated client checks writes (its reads return
the error as raw bytes), the dynamic client reads and writes.

```rust
#[ww_trait]
pub trait I2cMonitor {
    #[errors(CommonError(InUse, Unsupported))]
    #[stream(I2cRecord<'i>, counted)]
    fn records(io_level: Option<Volt>) -> Result<(), I2cError>;

    #[errors(CommonError(Unsupported, Busy))]
    #[effect(edit)]
    property!(rw speed_hz: u32, I2cError);
}
```

[`ww_error`](../std_library/errors_lists.md#which-errors-a-method-returns) walks through the `CommonError(..)` entry
and real examples from `ww_stdlib`.

### Messages from doc comments

The `display` directive writes `Display` and `std::error::Error` (`core::error::Error` on `no_std`) from each
variant's first doc line, as [Errors](../basic/errors.md#declaring-the-apis-own-errors) shows. Use
`#[display("...")]` on a variant when its message should differ from its doc.

## Common errors

The common errors are `ww_error::CommonError` (also in `ww_api::prelude`): [Common errors](../std_library/ww_error.md)
lists them, and [Common errors in an API](../std_library/ww_error_in_an_api.md) returns them from handlers and reads
them on the host. Every item can return them, whether or not it has an error type of its own.

On the wire a common error is an error event of the protocol (`ww_client_server::ErrorKind::Common`, STD-16) at the
item's handler site, the same for a method, a property, a stream function, and for the hub refusing a claimed
resource. `wire_weaver::Error<E>` is protocol-neutral: another dispatcher sends `Common` as its own protocol's common
error. Python's `RemoteError.common` holds the variant and its fields (`.value` is `None` then), and `ww` prints its
text.

Keep a crate-specific variant when a caller should act on exactly that case: `Nack` for I2C, an `UnsupportedSpeed`
that carries the supported range.

## On the client: error crates and retries

For a method returning `Result<T, E>`, `call()`, `blocking_call()` and promises return `Result<T, CallError<E>>`,
whose four variants [Errors](../basic/errors.md#handling-errors-on-the-host) shows.

`CallError<E>` implements `std::error::Error` when `E` does, and its `Display` is the inner error's, so it fits
`anyhow` and `thiserror` as is:

```rust
// a script
async fn script(dev: &Board) -> anyhow::Result<()> {
    dev.i2c().i2c_write(0x50, data).call().await?;
    Ok(())
}

// a driver crate with its own error
#[derive(Debug, thiserror::Error)]
enum SensorError {
    #[error(transparent)]
    I2c(#[from] CallError<I2cError>),
    #[error("unexpected chip id {0:#x}")]
    ChipId(u8),
}

// handling one case: retry while an EEPROM is busy with a page write
loop {
    match dev.i2c().i2c_write(0x50, data.clone()).call().await {
        Err(CallError::Device(I2cError::Nack)) => continue,
        other => break other?,
    }
}
```

Methods without a `Result` return `Result<T, wire_weaver_client::Error>`; `is_unimplemented()` on it answers the same
"can the device do this at all" question, `common()` gives the common error the device refused with. Python already
flattens all of this: a call returns `T` or raises.

## Unsupported is not Unimplemented

Two answers look alike and mean different things:

- `CommonError::Unsupported` comes from code that ran on the device: it knows the call, and this case isn't possible.
- `Unimplemented` means there is no code for the item at all: the handler returned `RpcResult::Unimplemented`
  (`GetResult` / `SetResult` for properties), or the item is an optional one the device left out.

`RpcResult`, `GetResult` and `SetResult` let a handler leave an implementation out without `todo!()`, or answer
later (`Deferred`), and the framework reports it properly. `ww api scaffold` generates every handler answering
`Unimplemented`, so a new device builds and answers sensibly before anything is written. On the client this is
`CallError::Unimplemented`, never confused with an error of the device.

## Where on the device an error came from

Errors of the framework itself (a path that doesn't exist, arguments that don't decode, an answer that doesn't fit)
come back as `ww_client_server::Error`: an `ErrorKind` and an `err_seq`, the number of the site in the generated
server that raised it (SRV-22). Together with the firmware build (its build id or git sha) and the request's path,
which the host knows, they pinpoint one place. The host prints them as one line:

```text
Remote device returned Unimplemented at item site 2 (handler)
Remote device returned BadPath at level site 40 (no such item)
```

The numbers are the same in every server, defined in `ww_client_server::err_seq`:

| `err_seq` | Range | Sites |
|---|---|---|
| 0..31 | item | the steps of a method, property, stream or sink, in step order and the same for every item: 0 request kind not taken, 1 decode, 2 handler (`Unimplemented`), 3 encode, 4 user error encode, 5 stream store, 6 stream resume, 7 data too long, 8 credit position (firmware before API-34, not reused), 9 deferred method in a multi request |
| 32..35 | deferred return | `<method>_ser_return_event` of a deferred method |
| 40..46 | level | a trait level's dispatch: 40 no such item, 41 item left out, 42 index not valid, 43 index expected, 44 a level doesn't take this request, 45 valid indices too big, 46 answer header too big |
| 48..63 | request | before or around the path: 48 global trait not found, 49 path too deep, 50..53 multi requests, 54..58 observe requests, 59 introspection, 60 answer over the link's message limit, 61 a request kind this firmware doesn't serve (`RequestKind::Unknown`: newer, or left out with the `multi` / `streams` features), 62 a request the shared entry doesn't take ([shared handlers](handlers.md)), 63 a request a [host server](host_server.md#limits) refused for its load (`Overloaded`), 64 a request a host server dropped at its [deadline](host_server.md#deadline-and-cancel) (`DeadlineExceeded`), 65 one it dropped for the client's cancel (`Cancelled`), 66 one its policy refused (`Denied`) |
| `u32::MAX` | generic | `ww_device` couldn't get any answer out of the server |

The item kind, the request kind and the error kind tell apart what shares a number: a decode failure of a method's
arguments and of a property's value are both 1, one comes with `ArgsDesFailed` on a call, the other with
`PropertyDesFailed` on a write. Since the same step of every item is the same constant, the firmware's error arms
are shared code (Portwright: 1.5 KiB less flash at `opt-level = "z"` than one number per site, 1.7 KiB at 3). `Error::site()` sorts a number into its
range.

## Devices and hosts of different versions

An error enum grows: a later firmware may return a variant an older host has never heard of. Every `Unsized` enum
(the default kind) therefore gets one more variant from the derive, `Unknown(u32)`, which catches any discriminant the
reader doesn't know instead of failing to decode the whole reply:

```rust
match e {
    CallError::Device(I2cError::Nack) => { /* ... */ }
    CallError::Device(I2cError::Unknown(n)) => warn!("device error {n} from a newer firmware"),
    // ...
}
```

The variant takes no number on the wire and isn't part of the API model, so adding it changes neither the bytes nor
snapshots. The dynamic decoder and Python show such a value as `<unknown N>`.

| Enum | Gets `Unknown(u32)` |
|---|---|
| `Unsized` with a free discriminant in its `ww_repr` | yes, automatically |
| `final_structure`, `self_describing`, `sized` | only when written by hand |
| every discriminant of the `ww_repr` already taken | no |
| marked `no_future_version` | no, for sets that are complete by nature |

When a variant of the enum is already called `Unknown`, name the catch-all with `future_version = Other`.

## Optional items

Not every device can do everything a trait describes. An API author marks such items `#[optional]`, and a device
that lacks them leaves them out at code generation, paying neither a dispatch entry nor a handler:

```rust
#[ww_trait]
pub trait I2c {
    /// Clocks SCL until SDA is released, then a stop.
    #[optional]
    #[effect(outbound)]
    fn i2c_wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError>;
}

// a device without it
ww_codegen!(board_api::Board for Server, server = true, omit = ["i2c.i2c_wiggle_clock"]);
```

Called anyway, an omitted item answers `Unimplemented`, and a client with the device's introspection data fails the
call locally, without a round trip. [Optional items](optional_items.md) has the details: groups, `omit_if`, what
clients see, measured code size, and when to make a trait of its own instead.
