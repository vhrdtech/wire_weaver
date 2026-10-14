# Errors and optional items (design)

!!! note "Design, approved by Roman on 14 Oct 2026"
    All recommendations accepted (see [Decisions](#decisions)); SW-47 is built, the rest is being built. Draft of
    14 Oct 2026, from Roman's review of the IO trait family (`docs/std_library/io_family.md` on
    `s/14oct2026-ww-io-traits`). Nothing here is built yet except where a section says so. Items: SW-47
    future-version-default, CLT-24 flat-call-errors, STD-16 common-errors, API-32 method-error-lists, DER-12
    display-from-docs, API-16 optional-items (see [Plan](#plan)).

The page answers four questions for every API, `ww_stdlib` crates and user APIs alike:

1. Can a method return only the errors it can actually produce? ([Per-method errors](#1-per-method-errors))
2. How do errors that every crate has (`Unsupported`, `InUse`, `Refused`, ...) look the same everywhere, and how
   does a generic tool recognise them? ([Common errors](#2-common-errors))
3. How does every growing enum survive a newer peer without anyone remembering `#[future_version]`?
   ([Future variants by default](#3-future-variants-by-default))
4. How does a device leave out a method its hardware can't do, without a stub and without a new trait?
   ([Optional items](#4-optional-items))

All Rust snippets of the client and downstream side were compiled in a prototype with the code codegen would
emit written by hand (plain Rust types, `anyhow` 1, `thiserror` 2); what doesn't compile is marked as such.

## Where we are

- A method returning `Result<T, E>` sends `E` as a normal `shrink_wrap` value. `E` is one enum per crate
  (`ww_gpio::Error`, `ww_dfu::Error`, `I2cError`), its variants differ from crate to crate for the same situation
  (`Unsupported` / `NotImplemented` / `UnsupportedMode`, `Busy`, `WrongMode` / `WrongState`).
- The generated Rust client returns `Result<Result<T, E>, wire_weaver_client::Error>`, so every call ends in `??`
  (`ww_gpio_hl` does that throughout). Python already flattens it: it returns `T` or raises `RemoteError` with `E`
  in `.value`.
- Framework errors (bad path, a handler returning `Unimplemented`, arguments that don't decode) are
  `ww_client_server::ErrorKind`, which the client turns into `wire_weaver_client::Error::RemoteError`.
- `#[future_version] Unknown(u32)` (SW-16) is opt-in per enum, so it is forgotten exactly where it matters.
- A method a device can't do needs a handler that answers `Unsupported`, and costs a dispatch arm and the stub.

## 1. Per-method errors

### What was explored: a narrow error type per method

WireWeaver owns the codegen, so it can give each method its own error enum. The trait names the variants a method
returns, codegen emits a narrow enum with exactly those, using **the crate enum's discriminants**, so the narrow
type is a Rust-only view: the bytes on the wire are the crate enum's, tools decode with the crate enum's model, and
a method can later return one more variant without any wire change.

```rust
// ww_i2c: the trait declares the list
#[ww_trait]
pub trait I2c {
    #[errors(Common, Nack, Arbitration, Bus, Timeout, WrongMode, BufferTooBig)]
    fn i2c_write(
        addr: u8,
        data: MaxLen<RefVec<'i, u8>, MAX_WRITE_LEN>,
        delay_after: Option<Second>,
    ) -> Result<I2cTransactionTimeRange, I2cError<'i>>;
}

// generated (narrow variant of the design): same discriminants as I2cError, plus the future variant
pub enum I2cWriteError<'i> {
    Common(CommonError<'i>), // each variant keeps its discriminant from I2cError
    Nack,
    Arbitration,
    // ...
    Unknown(u32),
}
impl From<I2cWriteError<'_>> for I2cError<'_> { /* every variant maps 1:1 */ }
impl TryFrom<I2cError<'_>> for I2cWriteError<'_> { /* the others are Err(e) */ }
```

Downstream, with a flat client error (see [one `?` per call](#one-per-call) below):

```rust
// 1. a script with anyhow: one `?`, works with either type
fn script(dev: &DeviceClient) -> anyhow::Result<()> {
    dev.i2c().i2c_write(0x50, data, None).call().await?;
    Ok(())
}

// 2. a driver crate with its own thiserror enum
#[derive(Debug, thiserror::Error)]
enum SensorError {
    #[error(transparent)]
    I2c(#[from] CallError<I2cErrorOwned>),
    #[error("bad chip id {0:#x}")]
    ChipId(u8),
}
fn init(dev: &I2cClient) -> Result<(), SensorError> {
    dev.i2c_write(0x50, data, None).call().await?;  // crate enum: compiles
    // narrow type: does NOT compile, no From<CallError<I2cWriteErrorOwned>> for SensorError,
    // and no blanket impl can provide it (overlaps with From<T> for T, orphan rules)
    dev.i2c_write(0x50, data, None).call().await.map_err(CallError::widen)?; // narrow type: this, on every call
    Ok(())
}
// ... or once per driver error, instead of #[from], a hand-written generic impl that takes every narrow type:
impl<E> From<CallError<E>> for SensorError where I2cErrorOwned: From<E> {
    fn from(e: CallError<E>) -> Self { SensorError::I2c(e.widen()) }
}

// 3. retry on Nack (an EEPROM busy with a page write): the same with both
match dev.i2c_write(0x50, data, None).call().await {
    Err(CallError::Device(I2cErrorOwned::Nack)) => continue,
    other => other?,
}

// 4. exhaustive match over one call's errors: only here the narrow type is shorter
match e {
    I2cWriteErrorOwned::Nack => ..,
    I2cWriteErrorOwned::Arbitration => ..,
    I2cWriteErrorOwned::Common(c) => ..,
    // ...
    I2cWriteErrorOwned::Unknown(_) => .., // never goes away: a newer device may return more
}
```

On the device the narrow type bites where code is shared: a helper `fn check_level() -> Result<(), I2cError>` used
by several handlers can't be `?`-ed into a handler returning `I2cWriteError` (only `TryFrom` exists); helpers have
to return `CommonError` or the narrow type of each caller.

### Which wins where

| Situation | One enum per crate | Narrow enum per method |
|---|---|---|
| Script with `anyhow` | one `?` | one `?` |
| Driver with a `thiserror` enum | `#[from] CallError<I2cErrorOwned>` once | a hand-written generic `From`, or `.map_err(CallError::widen)` on every call |
| Handling one or two errors (retry on `Nack`) | same | same |
| Exhaustive match over a call's errors | all crate variants or `_` | the method's variants, and `Unknown(_)` anyway |
| Seeing what a call can fail with | generated `# Errors` docs from the list | the type itself, in the IDE |
| Device handlers sharing helpers | `?` everywhere | helpers return `CommonError` or per-caller types |
| Device returns a variant not in its list | client warns (debug, tests: error) | compile error on the device |
| A method returns one more variant later | nothing changes for Rust users | narrow type grows: exhaustive matches break, client crate bump |
| Generic tools, Python | list from the model | list from the model |
| Device code size, wire | same | same |

Because every growing enum ends in `Unknown(_)` (section 3), a match over one call's errors needs a catch-all either
way, which takes away most of what the narrow type offers downstream. What stays is the contract on the device side
and the IDE view; what it costs is friction in every driver crate and a Rust break whenever a method's list grows.

### Recommendation

- **One error enum per crate**, the type on the wire, in server handlers and in the client (or one per group of
  traits when a crate's traits really have unrelated errors).
- **Per-method lists in the trait** (`#[errors(..)]`, API-32): the list goes into the API model and from there into
  generated client docs (`# Errors` with each variant's doc line), introspection (ww CLI, GUI, Python `help()`), the
  evolution checker (a list growing is a documented, compatible change), and a check on the **client**: a device
  returning a variant outside the method's list logs a `tracing` warning, and fails in `tests_common`. Nothing on the
  device, no flash. Without `#[errors]` a method may return every variant, as today.
- **Narrow types stay an idea** (💡 in FEATURES, API-33): if a device author wants the contract at compile time,
  `ww_codegen!(.., server = true, narrow_errors = true)` can generate narrow handler return types in the server crate
  only, where the friction is the device author's own choice. Not built until someone asks.
- `wire_weaver_client`'s `??` goes away for everyone, whatever is chosen above (CLT-24, next section).

### One `?` per call

For a method `-> Result<T, E>`, `call()`, `blocking_call()` and promises return `Result<T, CallError<E>>`:

```rust
pub enum CallError<E> {
    /// The device answered with its error: the `Err(e)` of the method.
    Device(E),
    /// The device has no implementation: its handler returned `Unimplemented` (`RpcResult`, `GetResult`,
    /// `SetResult`), the item is an omitted optional one (API-16), or the device's API doesn't have it (known locally
    /// from introspection, no round trip).
    Unimplemented { resource: String },
    /// The call didn't go through: link, timeout, disconnect, another framework error.
    Client(wire_weaver_client::Error),
}
```

**`RpcResult`, `GetResult` and `SetResult` keep their meaning.** They are neither common nor per-method errors: they
let a handler leave an implementation out (`Unimplemented`, the scaffold's default) or answer later (`Deferred`)
without `todo!()`, and the framework reports it. `CallError` keeps that visible as its own variant, so a client
tells "the device said no" (`Device`), "the device can't do this at all" (`Unimplemented`) and "the call failed"
(`Client`) apart without digging into `ErrorKind`. Methods without `Result` keep `wire_weaver_client::Error`, which
gets `is_unimplemented()` for the same question.

`Display` and `source()` are transparent (the inner error's text once, so `anyhow`'s `{:#}` doesn't print it twice);
`CallError<E>` is `std::error::Error` when `E` is. Methods without `Result` keep `Result<T, wire_weaver_client::Error>`.
Properties with a write error type (`write_err_ty`) get the same for `write()`. Python is unchanged (it already
flattens). Breaking for Rust clients: `??` becomes `?`, `Ok(Err(e))` becomes `Err(CallError::Device(e))`.

## 2. Common errors

Most crates have the same few situations; a generic tool should show them the same way, and generic host code
(`ww_gpio_hl`, a GUI) should handle them without knowing the crate. Two shapes were compared:

| | Common variants first in each crate enum | Nested `Common(CommonError)` variant |
|---|---|---|
| Rust match | `I2cError::InUse { by }` | `I2cError::Common(CommonError::InUse { by })` |
| Device code | `Err(I2cError::InUse { by })` | `Err(CommonError::InUse { by }.into())`, helpers return `CommonError` and `?` into any crate's error |
| Generic host code | per crate, by hand | `e.common()` on any crate's error (`HasCommon`) |
| Tool recognition | by variant name and shape: a convention, fragile | by type identity of `ww_error::CommonError` in the model, robust |
| Adding a common variant | in every crate, a release of each | once, in `ww_error`; every crate's errors get it |
| Wire | 4 bits per error | 8 bits per common error (two `UNib32` discriminants), 4 for crate-specific ones |

**Recommendation: nested.** The 4 extra bits per error are nothing; recognition by type identity and one place to
grow win. Every error enum that has common cases puts `Common(CommonError<'i>)` first (discriminant 0); the derive
sees the variant (marked `#[common]`) and generates `From<CommonError>` and `HasCommon` (STD-16).

```rust
/// ww_error: errors every crate can return, recognised by tools by this type (STD-16).
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), display, ww_repr = unib32)]
pub enum CommonError<'i> {
    /// Not supported by this device (a mode, a range, a feature of the call).
    Unsupported,
    /// Argument number {arg} is invalid: {reason}.
    InvalidArgument { arg: u8, reason: MaxLen<&'i str, 64> },
    /// In use by {by}.
    InUse { by: MaxLen<&'i str, 32> },
    /// Busy with another operation, try again later.
    Busy,
    /// Not possible in the current state: {reason}.
    WrongState { reason: MaxLen<&'i str, 64> },
    /// No power on {rail}.
    NoPower { rail: MaxLen<&'i str, 16> },
    /// Refused: {reason}.
    Refused { reason: MaxLen<&'i str, 64> },
    /// Timed out.
    Timeout,
    /// Hardware fault {code:#x}.
    Fault { code: u32 },
    // Unknown(u32) added by SW-47
}

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), display, ww_repr = unib32)]
pub enum I2cError<'i> {
    #[common]
    Common(CommonError<'i>),
    /// The address or a data byte was not acknowledged.
    Nack,
    /// Arbitration lost to another master.
    Arbitration,
    // ...
}
```

- Where the variants come from: the IO family (`InUse`, `NoIoLevel` → `NoPower { rail: "VIO" }`, `Refused`,
  `Unsupported`), `ww_adc` (`InvalidArgument`, `Busy`, `Timeout`, `WrongMode` → `WrongState`), `ww_dfu` (`Busy`,
  `Unsupported`, `WrongState`), `ww_gpio` and `ww_uart` (`Unsupported*`, `WrongMode`). A crate keeps its own
  variant when a tool should act on it specifically (`UnsupportedSpeed` with the range, `Nack`).
- Strings are bounded and may be empty (one byte); a device that saves flash sends `""`.
- `Unsupported` (the device knows the call and can't do this) is not `ErrorKind::Unimplemented` (no handler, or an
  [optional item](#4-optional-items) left out); the second stays a framework error.
- `ww_error` is a type crate like `ww_si`: in `ww_stdlib`, versioned by identity, in the snapshots, re-exported as
  `wire_weaver::prelude::CommonError` so user APIs use it without a new dependency.
- **Tools**: the ww CLI, GUI and Python see `ww_error::CommonError` by its global type id in the model and render
  it the same everywhere (an icon, "in use by USART2"), with each variant's doc line as the message. Python's
  `RemoteError` gets `.common` (the variant name and fields) next to `.value`.
- `display` (DER-12): the derive writes `Display` and `core::error::Error` from the first doc line of each
  variant, with `{field}` filled in (or `#[display("..")]` when the doc should say more). Unused on the device, the
  linker drops it. Hosts then need no hand-written `Display` for any API error.

## 3. Future variants by default

SW-16 landed opt-in. SW-47 makes the derive add `#[future_version] Unknown(u32)` itself (**built** on this branch,
`docs/serdes/derive.md#enum-variants-from-a-newer-version`):

| Enum | Gets `Unknown(u32)` |
|---|---|
| `Unsized` (the default) with a free discriminant in its `ww_repr` | yes, automatically |
| `final_structure`, `self_describing`, `sized` | only if written (they can read past new unit variants only, and are chosen for closed, compact sets like `Level`) |
| every discriminant of the repr taken (`ww_repr = u1` with two variants) | no |
| `no_future_version` argument | no (an enum that is complete by nature) |
| a user variant already named `Unknown` | compile error naming the fix: `future_version = Other` names the variant `Other` |
| `#[future_version] X(u32)` written by hand | kept as written, as today |

- Wire: unchanged (the variant takes no number, `ELEMENT_SIZE` and `MAX_SIZE` stay). API model and snapshots:
  unchanged (the variant is left out of the model, the dynamic decoder already reads unknown variants of `Unsized`
  enums as `<unknown N>`).
- Rust: every `Unsized` enum of every crate gets a variant: exhaustive matches need an arm, fieldless enums are no
  longer castable with `as`, `Copy` enums grow to 8 bytes. A Rust API break of `shrink_wrap_derive` and of every
  crate with such an enum: one minor bump each, in this release cycle, before anything is published with errors that
  can grow.
- Framework enums, as built: `ww_client_server::ErrorKind` gets it (a new framework error no longer breaks an old
  client's decode), `wire_weaver::ValidIndices` too (an unknown form contains no index), `DisconnectReason` gets
  `Newer(u32)` (it has an `Unknown` of its own). `MultiIndex`, the `ww_self` model enums and
  `ww_numeric::NumericAnyType` are `no_future_version`: their readers can't act on a value they don't know (INT-8
  for a model that tolerates newer variants). `RequestKind`, `PathKind`, `EventKind`, `StreamSideband` are
  `final_structure`, unchanged.
- Python and the dynamic decoder show the variant as `<unknown N>` (as since SW-16); the C generator (API-26) needs
  an unknown tag with the number.

## 4. Optional items

A method, property or stream of a trait can be marked optional; a device's server leaves it out at codegen time and
pays neither the dispatch arm nor the handler. One mark, one flavour:

```rust
#[ww_trait]
pub trait I2c {
    fn i2c_configure(mode: I2cMode, io_level: Option<Volt>) -> Result<u32, I2cError<'i>>;
    // ...
    /// Clocks SCL until SDA is released, then a stop.
    #[optional]
    fn i2c_wiggle_clock(count: u8) -> Result<I2cLinesState, I2cError<'i>>;
    /// Switchable pull-ups: both items or neither.
    #[optional(pullups)]
    fn i2c_set_pullups(enabled: bool) -> Result<(), I2cError<'i>>;
    #[optional(pullups)]
    property!(ro i2c_pullups: bool);
}

// a device without these: no handlers to write
ww_codegen!(nucleo_api::Nucleo for Server, server = true, omit = ["i2c.i2c_wiggle_clock", "i2c.pullups"]);
```

- **Only items marked `#[optional]` can be omitted**: the API author decides what is optional, so the trait stays a
  contract. An item keeps its resource index whether it is there or not; `#[optional(group)]` names a set that goes
  together.
- **Wire answer when called anyway**: `ErrorKind::Unimplemented`, the same as a handler returning
  `Unimplemented`. The trait level's fallback arm checks `id < ITEM_COUNT` (one comparison per level, only in levels
  that omit something), so there is no arm per omitted item. A second flavour answering `BadPath` was considered: it
  saves only that comparison and would tell the client a lie about the trait, so it is left out.
- **Introspection**: the trait reference in the device's model gets `omitted: RefVec<UNib32>` (`ApiItemKind::Trait`,
  a defaulted field, compatible). The client's compat check (`compat.rs`) then fails a call to an omitted item
  locally with `NotImplementedByDevice`, without a round trip; ww CLI, GUI and Python grey it out. Snapshot-known
  traits stay skipped in the bundle, only the omitted list is sent.
- **Cargo features**: `omit_if(not(feature = "pullups")) = ["i2c.pullups"]` puts a `cfg` on the arm instead, so a
  device crate ties optional items to its own features. This is API-16 feature-groups, which this design replaces.
- `ww api scaffold --omit ..` leaves the stubs out; `capabilities()` booleans that only said whether a method works
  (`monitor_supported`) can go, the model says it. A capability that depends on what is found at run time (a board
  variant read from EEPROM) stays a capability: optional is for what is known when the firmware is built.

**When optional, when a trait.** Optional: one to three items within the trait's subject that some hardware lacks
(clock wiggle, pull-ups, a hardware CRC). A trait: items with their own state, configuration or streams, a subject of their
own (a bus monitor, ADC capture), anything that a different set of devices implements, or four items and more.

## Plan

| Piece | Model | Where | Depends on |
|---|---|---|---|
| SW-47 future-version-default: the derive adds `Unknown(u32)`, `no_future_version`, `future_version = X`, every match in the repo, framework enums reviewed, Python shows `Unknown`, docs `derive.md` / `rules.md`, CHANGELOGs | opus | this session, own commits | — |
| CLT-24 flat-call-errors: `CallError<E>`, `call` / `blocking_call` / promise / property write, `ww_gpio_hl`, tests, examples, docs `methods.md` | opus | child session | — |
| DER-12 display-from-docs: `display` directive, `#[display("..")]`, `core::error::Error` | sonnet | child session | — |
| STD-16 common-errors: `ww_error` crate, `#[common]` in the derive (`From`, `HasCommon`), prelude re-export, snapshot, docs page | opus | child session | DER-12 for `display` (can land without it) |
| API-32 method-error-lists: `#[errors(..)]` parsed and checked against the enum, `ww_self` field, client docs, client-side check, dynamic client, Python `help()`, ww CLI, `ww api check` | opus | child session | STD-16 for `Common(..)` paths |
| API-16 optional-items: `#[optional]`, `omit` / `omit_if`, fallback arm, `omitted` in the model, compat, scaffold, Python / CLI | opus | child session | — |
| Stdlib migration: `ww_gpio`, `ww_uart`, `ww_dfu`, `ww_can_bus`, then `ww_i2c` / `ww_adc` of the IO family: `Common` first, `#[errors]` on every method (from the `# Errors` sections), `#[optional]` where methods answer `Unsupported` today | sonnet | after STD-16, API-32, API-16; IO crates with the ww-io-traits session | all above |
| API-33 narrow-error-types: server-only narrow handler types | — | 💡 idea, not filed as work | API-32 |

Breaking, all before anything with these APIs is published: SW-47 and CLT-24 break the Rust API (not the wire);
STD-16 and the migration break the wire of stdlib error enums (none published: `ww_gpio`, `ww_uart`, `ww_i2c`,
`ww_dfu` are not on crates.io); API-32 and API-16 add defaulted fields to `ww_self` (compatible).

## Decisions

Roman's answers of 14 Oct 2026: all recommendations accepted, `CommonError` as proposed (extended later when
needed), with one condition: `RpcResult` / `GetResult` / `SetResult` keep their semantics and `CallError` keeps
`Unimplemented` visible (done above). The questions were:


1. Per-method errors: crate enum + lists in the model (recommended), or narrow types per method after all?
2. Common errors nested as `Common(CommonError)` (recommended), or flat variants first in each enum?
3. The `CommonError` variants above: anything missing or too much (`NoPower`, `Fault`)?
4. SW-47 scope: automatic for `Unsized` enums only (recommended), or also `self_describing` / `sized`? Opt-out
   spelled `no_future_version`?
5. Optional items: one flavour (`Unimplemented`, recommended), opt-out list `omit = [..]` in `ww_codegen!`?
