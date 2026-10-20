---
level: advanced
---

# Optional items

A method, property, stream or whole `ww_impl!` of a trait can be marked optional. A device's server may then leave it out when it is
built: no dispatch arm, no handler to write, no stub answering "not implemented". A client calling it gets
`Unimplemented`, and a client with the device's introspection data knows it without asking.
`tests/traits` has a worked example; [Errors](errors.md#unsupported-is-not-unimplemented) tells `Unimplemented`
apart from a device's own errors.

```rust
#[ww_trait]
pub trait I2c {
    fn i2c_configure(config: I2cConfig) -> Result<I2cConfig, I2cError<'i>>;
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

## Marking items optional

- `#[optional]` on a method, property, stream or `ww_impl!` lets a device leave it out (see
  [Optional traits](#optional-traits)). `#[optional(group)]` names a set of items
  that go together: leaving the group out leaves out all of them.
- Only marked items can be left out: the API author decides what is optional, the rest of the trait stays a contract
  every device keeps. Leaving out a required item is a compile error that lists the trait's optional items and groups.
- An item keeps its resource index whether a device has it or not, so clients address it the same way everywhere.
- The marker is part of the trait's signature: marking an item optional later changes the API model, and its
  snapshot has to be saved again.
- Changing the marker later follows the [evolution rules](../evolution/rules.md#optional-items): making an item
  optional is compatible, making it required again or moving it to another group is a break.

## Leaving items out

`ww_codegen!(.., server = true, omit = [..])` takes paths `"<trait item>.<..>.<item or group>"` from the API root,
without array indices: `"periph.channel.run"` leaves `run` out of every `periph[i].channel[j]`. Items of the API root
itself are named directly (`omit = ["reset"]`). A group may not share a name with an item of the same trait (group `format` and method `format`): `#[ww_trait]` refuses it with an error naming both, rename the group. Naming an item
twice (by name and through its group, or in `omit` and `omit_if`) is a compile error, as is an unknown name.

`omit_if(<cfg>) = [..]` ties optional items to the device crate's own Cargo features:

```rust
ww_codegen!(
    nucleo_api::Nucleo for Server,
    server = true,
    omit_if(not(feature = "pullups")) = ["i2c.pullups"]
);
```

The dispatch arm gets `#[cfg(not(<cfg>))]`; the handlers are the device crate's to `cfg` out. The introspection data
(and with it the API hash) depends on which items are left out, so it is generated for each combination of the
conditions, under `#[cfg(all(..))]`; at most 4 different conditions per `ww_codegen!`.

`ww api scaffold --omit i2c.pullups,reset` leaves the stubs of these items out and writes the same `omit = [..]` into
the generated `ww_codegen!` call; the stubs of optional items it keeps carry a comment naming their path.

## Optional traits

`#[optional]` on a `ww_impl!` makes the whole trait attachment optional, with everything below it: its items, its
nested traits, its consts. It is for a block a firmware has only with one of its features, such as pin diagnostics
served only in a debug build:

```rust
#[ww_api_root]
trait Board {
    fn ping() -> u8;
    /// Pin diagnostics, the firmware's `pin-diag` feature.
    #[optional]
    ww_impl!(diag: ww_pin_diag::PinDiag);
    /// Probes and their count: both or neither.
    #[optional(probes)]
    ww_impl!(probe[]: Probe);
    #[optional(probes)]
    fn probe_count() -> u8;
}

ww_codegen!(
    board_api::Board for Server,
    server = true,
    omit_if(not(feature = "pin-diag")) = ["diag"],
    omit = ["probes"]
);
```

- Leaving the trait out leaves out every handler below it (`diag_set`, `diag_read`, ..), the `valid_indices` of a
  trait array, and the trait's `const` items in the consts trait. With `omit_if`, give the handlers (and the
  consts' values) the same condition: `#[cfg(feature = "pin-diag")]`.
- A trait array (`probe[]`) is left out with all its indices; a group may hold traits and items together.
- Streams of a left-out trait are not sent by `poll_streams`. With `omit_if`, there is one `poll_streams` per
  combination of the conditions, all with the same signature, so the firmware calls it the same way in every build.
- Items inside an optional trait can be optional on their own (`omit = ["diag.release_all"]`), for the builds that
  keep the trait.
- On the wire it is the same as an omitted item: the trait keeps its id, a request to anything below it is answered
  `Unimplemented` by the fallback arm of the level holding the trait, and the device's model lists the trait in that
  level's omitted items, so clients fail every call below it locally. Its definition stays in the introspection
  data, so tools show what the device leaves out.
- `ww api scaffold` notes optional traits in their section of handlers, `--omit diag` leaves the section out.
- Not yet: `+observe` properties of a trait left out with `omit_if` are not observed in either build (as an optional
  property left out with `omit_if`); with `omit`, or in a trait that isn't left out, they are observed as usual.

## What a client sees

- A call to an omitted item is answered `ErrorKind::Unimplemented`, the same answer as a handler returning
  `Unimplemented`. In a generated client that is `CallError::Unimplemented` (see [Methods](methods.md)).
- The device's introspection data lists the items it leaves out (`ApiItemKind::Trait { omitted }`,
  `ApiBundle::root_omitted` for the API root, `ApiItem::optional` for the marker). A client that has it fails a call
  to an omitted item locally with `NotImplementedByDevice`, without a round trip; the compatibility check doesn't
  count it as a mismatch, it is the device's choice.
- The dynamic client (`DynResource::is_omitted()`) and Python fail such calls locally too. Python signatures end in
  `(optional)`, or `(left out by this device)`; `ww api tree` and `ww introspect` show `#[optional]` and
  `(left out by this device)`.
- A device without introspection data, and no cached copy on the client: the call goes out and is answered
  `Unimplemented`.

## Cost

The fallback arm of a trait level that leaves something out answers `Unimplemented` for an id below the trait's item
count and `BadPath` above it: one comparison per level, no arm per omitted item. Levels that leave nothing out are
generated as before.

Measured on `thumbv7em-none-eabihf` (`opt-level = "s"`, LTO, introspection off) with a
`Board { ww_impl!(i2c: I2c); fn other(..) }` API whose `I2c` has one required method and three optional items (two
methods and an `rw` property):

| Server | `.text` |
|---|---|
| Stub handlers answering `Unimplemented` (what the scaffold generates without `omit`) | 7496 bytes |
| The three items left out with `omit` | 7204 bytes |
| The three items not in the API at all | 7192 bytes |

Leaving an item out saves about 100 bytes (its arm, argument decoding and the stub); the comparison costs 12 bytes per
level that leaves something out.

## Optional item or its own trait

Optional items are for one to three items within the trait's subject that some hardware lacks: a clock wiggle,
pull-ups, a hardware CRC. Items with their own state, configuration or streams, a subject of their own (a bus monitor,
ADC capture), anything a different set of devices implements, or four items and more make a trait of their own.

Optional is for what is known when the firmware is built. A capability that depends on what is found at run time (a
board variant read from EEPROM) stays a capability the device reports, or a `CommonError::Unsupported` answer (see
[ww_error](../std_library/ww_error.md)).
