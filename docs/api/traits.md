---
level: basic
---

# Traits

A trait is a block of an API: a set of methods, properties, streams and nested traits. The API root itself is one
(`#[ww_api_root]`), and `ww_impl!(name: Trait)` puts another trait into it, once or as an array. A trait can come
from the same file, to group related items, or from another crate, to reuse an API many devices share, such as a
GPIO bank or a firmware updater from the [standard library](../std_library/overview.md).

```rust
#[ww_api_root]
trait MyDevice {
    ww_impl!(motor: MotorControl);   // a trait from this crate
    ww_impl!(gpio[]: ww_gpio::Bank); // an array of a trait from another crate
}
```

WireWeaver traits are written in Rust syntax but not implemented like Rust traits: `ww_codegen!` reads the trait
from its source file and generates the server's dispatch and the client's functions. What `#[ww_trait]` leaves in
place is a Rust trait of the same name that only describes the API, so that `cargo doc` and docs.rs show it, see
[In rustdoc](#in-rustdoc) below.

## Grouping items

A trait defined in the same file as the API root groups related items:

```rust
#[ww_trait]
trait MyDevice {
    ww_impl!(motor_control: MotorControl);
    ww_impl!(led_control: LedControl);
}

#[ww_trait]
trait MotorControl {
    fn turn_on();
    fn turn_off();
}

#[ww_trait]
trait LedControl {
    fn led_on();
    fn set_brightness(value: f32);
}
```

On the server, handlers of nested items carry the trait item's name as a prefix (`motor_control_turn_on`), and the
generated client reaches them through it (`device.motor_control().turn_on()`). On the wire, each level adds one
index to an item's path ([Resources addressing](addressing.md)), so there are 4 paths here:

1. [0, 0] - `turn_on`
2. [0, 1] - `turn_off`
3. [1, 0] - `led_on`
4. [1, 1] - `set_brightness`

When every byte counts, keep the tree shallow and put the most used items first: indices `0..=7` take 4 bits on the
wire.

## Optional trait attachments

A `ww_impl!` marked `#[optional]` (or `#[optional(group)]`) is a block some builds of a firmware leave out, e.g. one
behind a Cargo feature. A server leaves it out with `ww_codegen!(.., omit = ["diag"])` or
`omit_if(not(feature = "pin-diag")) = ["diag"]`: no handlers to write for it, and clients see in the device's
introspection data that it is left out instead of getting `Unimplemented` from every item. See
[Optional items](optional_items.md#optional-traits).

```rust
#[ww_api_root]
trait Board {
    #[optional]
    ww_impl!(diag: ww_pin_diag::PinDiag);
}
```

TODO: splitting into multiple files

## Reusing traits from other crates

A trait published in a crate is defined once and implemented by many devices: GPIO banks, I2C, UART, firmware
updates, logs, counters, board and firmware info, memory access, CAN bus and more are in the
[standard library](../std_library/overview.md). A device API puts such a trait in with `ww_impl!` and the trait's
path; the crate providing it (`ww_gpio` here) is a normal Cargo dependency of the API crate, see
`examples/all_gpio_api`:

```rust
#[ww_api_root]
pub trait AllGpioApi {
    ww_impl!(port[]: ww_gpio::Bank);
    // and device specific items next to the common ones
}
```

The dependency can be a path, git or crates.io one (`ww_gpio = { git = ".." }`, `ww_gpio = "0.2"`). The macros
read the trait's source where Cargo put it: a path dependency in place, a git or crates.io one at the exact version
the `Cargo.lock` of the crate being built pins, in Cargo's own copy under `CARGO_HOME` (`git/checkouts/`,
`registry/src/`). Cargo fetches them before it compiles anything, so this just works in a build; a tool reading an
API crate that was never built (`ww api ...`) asks for `cargo fetch` first. Inside an API crate that Cargo fetched
itself (one your project depends on, or a docs.rs build) only its publisher's `Cargo.lock` is at hand, so its
crates.io dependencies are read at the newest matching version Cargo has.

Host code written for `ww_gpio::Bank` then works with any device that has a bank somewhere in its API, whatever the
device, the path or the transport: see [Trait attachments](trait_attachments.md). A host can also call a trait by
its crate's name and version instead of its path, on a device whose API it doesn't know, for example to reboot any
device into its updater: see [global trait addressing](addressing.md#global-trait-addressing-fullversion).

## In rustdoc

`#[ww_trait]` and `#[ww_api_root]` turn the trait into a Rust trait of the same name and visibility, with every
resource and its doc comment, so an API crate's documentation on docs.rs or from `cargo doc` shows the whole API.
Methods keep their signature, the other resources become functions returning a marker from `wire_weaver::api_doc`:

```rust
pub trait Uart {
    fn rx<'i>() -> Stream<RxChunk<'i>>;               // stream!(rx: RxChunk<'i>);
    fn tx() -> Sink<[u8]>;                            // sink!(tx: [u8]);
    fn configure(config: UartConfig) -> Result<UartConfig, Error>;
}
pub trait Bank {
    fn pin(index: UNib32) -> impl Pin;                // ww_impl!(pin[]: Pin);
}
pub trait Pin {
    fn pull() -> Property<Pull, Error>;               // property!(rw pull: Pull, Error);
}
```

Access mode, array index, flow control and `#[since]` are added as the last paragraph of each resource's docs.
`reserved!` doesn't show up. The trait is never implemented, handlers are generated by `ww_codegen!`
(see [server side](methods.md#server-side)), but it can be linked from docs (`[Uart]`, `[Uart::rx]`).

Because it is a real trait, the compiler checks it in the API crate: every type must be valid Rust (`RefVec<'i,
&'i str>`, not `RefVec<'i, str>`, lifetimes written out), and a trait used from another crate through `ww_impl!` must
be `pub`.

More: [Trait attachments](trait_attachments.md), host code for a reusable trait on any device.
