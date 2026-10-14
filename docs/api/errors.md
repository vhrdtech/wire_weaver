# Errors

A call to a device can fail in three different ways, and WireWeaver keeps them apart all the way to your code:

- **The device said no.** The method ran and returned its own error: an I2C address wasn't acknowledged, a pin is in
  use. This is the `E` of a method returning `Result<T, E>`, a normal `shrink_wrap` value.
- **The device can't do this at all.** There is no implementation: the handler answered `Unimplemented`, or the
  device left an [optional item](#optional-items) out.
- **The call didn't go through.** The link dropped, the request timed out, the device was disconnected.

This page explains how to declare errors in an API, how a device returns them, what a client gets, and how errors
stay readable when the device and the host are built from different versions.

## Declaring errors in an API

A method that can fail returns `Result<T, E>`, where `E` is the error enum of its crate:

```rust
#[ww_trait]
pub trait I2c {
    /// Writes `data` to the device at `addr`.
    #[errors(Common, Nack, Arbitration, Timeout, BufferTooBig)]
    fn i2c_write(addr: u8, data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>) -> Result<(), I2cError<'i>>;
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), display, ww_repr = unib32)]
pub enum I2cError<'i> {
    #[common]
    Common(CommonError<'i>),
    /// The address or a data byte was not acknowledged.
    Nack,
    /// Arbitration lost to another master.
    Arbitration,
    /// The bus stayed busy longer than the timeout.
    Timeout,
    /// The transfer is longer than the device can buffer ({max} bytes).
    BufferTooBig { max: u16 },
}
```

Use **one error enum per crate** (or per group of traits, when a crate's traits really fail for unrelated reasons).
The same type is used on the wire, in the device's handlers and in the client, so device code can share helpers and
`?` them anywhere, and a driver crate converts it into its own error with a single `#[from]`.

### Which errors a method returns

`#[errors(..)]` lists the variants a method can actually return. For a variant holding one enum, such as
`Common(CommonError)`, the entry names the variants of that one: `Common(InUse, Refused)`; `Common` alone means any
`CommonError`. The list is checked when the trait is parsed, so an unknown or repeated name is a compile error on
that name.

The list doesn't change the type or the wire; it goes into the API model, and from there:

- into the docs, as an `# Errors` section with each variant's doc line, both in the generated client and on the
  trait's rustdoc;
- into introspection, so `ww api tree` and Python's `help()` show it;
- into the evolution checker: a changed list is reported as a compatible change;
- into a check on the client: when a device returns a variant outside the method's list, the generated and the
  dynamic client log a `tracing` warning, so a device that doesn't keep its contract is noticed early.
  `wire_weaver_client::set_strict_error_lists(true)` makes it a panic instead; the test harness
  (`tests_common`) turns that on.

A method without `#[errors]` may return any variant of its error enum.

### Messages from doc comments

With the `display` directive, the derive writes `Display` and `std::error::Error` (`core::error::Error` on
`no_std`) from the first doc line of each variant, filling in `{field}` placeholders: `BufferTooBig { max: 64 }`
reads "The transfer is longer than the device can buffer (64 bytes)": the trailing period of the doc line is left
out, as Rust error messages go. Use `#[display("...")]` on a variant when the
message should differ from its doc. On a device the code is unused and the linker drops it; hosts need no
hand-written `Display` for any API error.

## Common errors

Some situations come up in almost every crate: the device doesn't support a mode, a resource is in use by something
else, the device is busy. These are `ww_error::CommonError` (also in `wire_weaver::prelude`), nested as the first
variant of a crate's error enum and marked `#[common]`:

| Variant | Meaning |
|---|---|
| `Unsupported` | the device knows this call but can't do what was asked (a mode, a range, a feature) |
| `InvalidArgument { arg, reason }` | argument number `arg` is invalid |
| `InUse { by }` | the resource is in use, `by` names the user |
| `Busy` | busy with another operation, try again later |
| `WrongState { reason }` | not possible in the current state |
| `NoPower { rail }` | the rail the operation needs has no power |
| `Refused { reason }` | refused, e.g. by a safety rule |
| `Timeout` | timed out |
| `Fault { code }` | a hardware fault |

Strings are bounded and may be empty, so a device short on flash sends `""`.

On the device, `#[common]` gives the crate error a `From<CommonError>`, so a helper returning `CommonError` can be
`?`-ed into any crate's error:

```rust
fn check_level(&self) -> Result<(), CommonError<'static>> {
    if self.io_level.is_none() {
        return Err(CommonError::NoPower { rail: MaxLen::new_unchecked("VIO") });
    }
    Ok(())
}

fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), I2cError<'static>> {
    self.check_level()?; // CommonError becomes I2cError::Common
    // ...
    Ok(())
}

async fn i2c_write(
    &mut self,
    _cx: &mut Context<'_, impl EventOut>,
    addr: u8,
    data: MaxLen<RefVec<'_, u8>, MAX_WRITE_LEN>,
) -> RpcResult<Result<(), I2cError<'_>>> {
    Ready(self.write(addr, data.as_slice()))
}
```

On the host, `e.common()` (trait `HasCommon`, in the prelude) works on any crate's error, for example on
`call_error.device()`, so generic code (a GUI, a `_hl` crate) handles the common cases without knowing the crate.
Tools recognise `CommonError` by its type in the API model: `ww api tree -t` marks it, and in Python
`RemoteError.common` holds the variant and its fields next to `.value`.

Keep a crate-specific variant when a caller should act on exactly that case: `Nack` for I2C, an `UnsupportedSpeed`
that carries the supported range.

## On the client: one `?` per call

For a method returning `Result<T, E>`, `call()`, `blocking_call()` and promises return `Result<T, CallError<E>>`:

```rust
pub enum CallError<E> {
    /// The device answered with its error: the `Err(e)` of the method.
    Device(E),
    /// The device has no implementation of this item.
    Unimplemented { resource: String },
    /// The call didn't go through: link, timeout, disconnect or another framework error.
    Client(wire_weaver_client::Error),
}
```

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
    I2c(#[from] CallError<I2cErrorOwned>),
    #[error("unexpected chip id {0:#x}")]
    ChipId(u8),
}

// handling one case: retry while an EEPROM is busy with a page write
loop {
    match dev.i2c().i2c_write(0x50, data.clone()).call().await {
        Err(CallError::Device(I2cErrorOwned::Nack)) => continue,
        other => break other?,
    }
}
```

Methods without a `Result` return `Result<T, wire_weaver_client::Error>`; `is_unimplemented()` on it answers the same
"can the device do this at all" question. Python already flattens all of this: a call returns `T` or raises.

## Unsupported is not Unimplemented

Two answers look alike and mean different things:

- `CommonError::Unsupported` comes from code that ran on the device: it knows the call, and this case isn't possible.
- `Unimplemented` means there is no code for the item at all: the handler returned `RpcResult::Unimplemented`
  (`GetResult` / `SetResult` for properties), or the item is an optional one the device left out.

`RpcResult`, `GetResult` and `SetResult` let a handler leave an implementation out without `todo!()`, or answer
later (`Deferred`), and the framework reports it properly. `ww api scaffold` generates every handler answering
`Unimplemented`, so a new device builds and answers sensibly before anything is written. On the client this is
`CallError::Unimplemented`, never confused with an error of the device.

## Devices and hosts of different versions

An error enum grows: a later firmware may return a variant an older host has never heard of. Every `Unsized` enum
(the default kind) therefore gets one more variant from the derive, `Unknown(u32)`, which catches any discriminant the
reader doesn't know instead of failing to decode the whole reply:

```rust
match e {
    CallError::Device(I2cErrorOwned::Nack) => { /* ... */ }
    CallError::Device(I2cErrorOwned::Unknown(n)) => warn!("device error {n} from a newer firmware"),
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
    fn i2c_configure(mode: I2cMode, io_level: Option<Volt>) -> Result<u32, I2cError<'i>>;
    /// Clocks SCL until SDA is released, then a stop.
    #[optional]
    fn i2c_wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError<'i>>;
    /// Switchable pull-ups: both items or neither.
    #[optional(pullups)]
    fn i2c_set_pullups(enabled: bool) -> Result<(), I2cError<'i>>;
    #[optional(pullups)]
    property!(ro i2c_pullups: bool);
}

// a device without them
ww_codegen!(board_api::Board for Server, server = true, omit = ["i2c.i2c_wiggle_clock", "i2c.pullups"]);
```

- Only items marked `#[optional]` can be left out; `#[optional(group)]` names items that go together. Paths start at
  the API root and skip array indices.
- `omit_if(not(feature = "pullups")) = ["i2c.pullups"]` ties optional items to the device crate's cargo features.
- An omitted item keeps its index. Called anyway, it answers `Unimplemented`.
- The device's introspection lists what it left out, so clients fail such calls locally without a round trip, and
  the `ww` CLI and Python mark them "left out by this device".
- `ww api scaffold --omit ..` generates the server without those stubs.

Leaving an item out saves its dispatch arm, argument decoding and handler, about 100 bytes of code each on a
Cortex-M. [Optional items](optional_items.md) has the details: paths, `omit_if` and introspection, measured
code size.

**Optional item or a trait of its own?** Make an item optional when it is one of a few things within the trait's
subject that some hardware lacks: a clock wiggle, switchable pull-ups, a hardware CRC. Make it a trait when it has
state, configuration or streams of its own (a bus monitor, ADC capture), when a different set of devices implements
it, or when it would be four optional items or more.
