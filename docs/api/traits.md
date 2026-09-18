# Traits

Traits in WireWeaver are used to define API blocks, as you can see from examples above, entry point for a device API is
also a trait. They carry similar meaning to Rust traits, in a sense that trait defines some functionality, that server
"implements" and client code can then interact with.

But they are not actually traits under the hood, `#[ww_trait]` macro leaves only some static checks and removes the
rest.
Rust syntax is currently used to bypass writing a whole parser from scratch.
All the magic happens through code generation in the `#[ww_api]` macro.

## Traits for API resources grouping

Trait defined in the same file as the API root itself is a way to cleanly group related resources together.

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
trait MotorControl {
    fn led_on();
    fn set_brightness(value: f32);
}
```

Note that in this case, one additional path index will be used, so in total there will be 4 valid paths here:

1. [0, 0] - `turn_on`
2. [0, 1] - `turn_off`
3. [1, 0] - `led_on`
4. [1, 1] - `set_brightness`

If preserving very small size is of big importance, try not to create too many levels. Also one can put more important
functionality higher up, in order to leverage variable length encoding (e.g. numbers `0..=7` take only 4 bits on the
wire).

TODO: splitting into multiple files

## Traits (global)

The idea behind global traits is to leverage crates.io to define a set of common traits used across many devices.
Device can then implement all the traits it needs and on the client side, common code can be used to control
similar functionality of different devices. `ww_gpio` (see [ww_gpio std library page](../std_library/ww_gpio.md)) is
the first such trait actually implemented; more generic traits are currently planned:

- FirmwareUpdate
- EmbeddedLog
- BoardInfo
- Counters
- DeviceUserInfo
- RegisterAccess
- CanBus

A device API, instead of re-implementing the same things over and over, can look like follows (`ww_impl!` just needs
a path to the trait; the crate providing it - `ww_gpio` here - is a normal Cargo dependency of the API crate, see
`examples/all_gpio_api`):

```rust
#[ww_api_root]
trait MyAwesomeDevice {
    ww_impl!(gpio[]: ww_gpio::Bank);
    // and some device specific functionality in addition to common things
}
```

Client code can be written in a completely agnostic way, e.g., only capable of interacting with a `Bank` trait,
regardless of which exact device it is implemented on, at which resource path, or how it is physically connected -
see [Trait attachments](#trait-attachments) below for how that decoupling actually works.

One can also interact with devices using trait-addressing mode, e.g., calling `set_indication_mode(Mode::Night)` on all
devices on a CAN bus, putting all boards with LEDs into night mode. More on that on
the [addressing page](./addressing.md)

## Trait attachments

Every generated client struct, at every level of the API tree (the root struct itself, or the struct returned for a
nested / array trait, whether it implements a global trait or a project-local one), gets a `.attachment()` method for
free. It returns a `wire_weaver_client::Attachment`: a self-contained handle bundling the underlying `Commander`
(already pointing at that exact resource path on that exact device), plus the source crate name + version and trait
name of whatever trait lives there. Because it carries this identity, an `Attachment` can be handed to code that has
never heard of the concrete device or its top-level API trait, as long as that code knows what to do with the global
trait underneath.

`examples/all_gpio/examples/high_level.rs` is the reference for this pattern end-to-end:

```rust
let device = AllGpio::new().connect_blocking()?;

// get a global trait "attachment" point
let attachment = device.port(0).attachment();
// init a high-level, hand-written driver with better ergonomics than generated code
let mut bank = BankBlocking::new(attachment)?;

let bank_name = bank.name()?;
let mut pins = bank.all_pins()?;
for pin in &mut pins {
    println!("{}{}: {:?}", bank_name, pin.index(), pin.mode()?);
}
```

`device.port(0)` only exists because `all_gpio_api::AllGpioApi` happens to declare `ww_impl!(port[]: ww_gpio::Bank)`

- it is `all_gpio`-specific generated code. `.attachment()` erases that specificity: `BankBlocking::new` never sees
  `AllGpio` or `AllGpioApi`, only something that claims to be a `ww_gpio::Bank`. It checks `trait_name()` and
  `source_crate()` on the attachment and returns `Error::IncompatibleTrait` if they don't match, so it will refuse an
  attachment that isn't really a `ww_gpio::Bank`, whatever device or resource path it actually came from - that runtime
  check is what makes it safe to accept an attachment from arbitrary device APIs.

The `ww_gpio_hl` crate (in `ww_stdlib/ww_gpio_hl`) is the reference for the driver side of this pattern - a
higher-level API built on top of plain generated code:

- `src/ww.rs` defines small wrapper structs (`BankClient`, `GpioClient`) and invokes `ww_impl!` against `ww_gpio::Bank`
  / `ww_gpio::Pin` directly, with `client = "trait_client"`. That generated code only depends on `ww_gpio`, never on
  `all_gpio_api` or any other specific device API crate, so it is written and generated exactly once and reused by
  every device that happens to expose a compatible bank somewhere.
- `src/blocking.rs` builds ergonomic, stateful types on top of that (`BankBlocking`, `FlexBlocking`,
  `PushPullOutputBlocking`, `InputBlocking`, `OpenDrainOutputBlocking`), each one constructed straight from an
  `Attachment` (`BankBlocking::new`, `FlexBlocking::new_ignore_mode` / `new_get_mode`). `src/promise.rs` does the same
  for immediate-mode UI use (`BankPromise`, `FlexPromise`); `src/asynchronous.rs` is the same idea for `async`, but is
  currently an empty stub.

Put together, a single `ww_gpio_hl` dependency can drive the GPIO bank of any device - regardless of what its
top-level API trait is called - as long as it implements `ww_gpio::Bank` somewhere in its resource tree; reaching it
takes exactly one `.attachment()` call on the device's own generated client.

### Attaching without generated client code

`.attachment()` is only available because the device's own API crate was run through `ww_codegen!`/`ww_impl!`, which
is what generates `device.port(0)` (and every other accessor) in the first place. When that generated code is
available, it is the convenient, recommended way to obtain an attachment.

Conceptually, an `Attachment` doesn't need to come from generated code at all: since it is just a `Commander` (already
addressed to a device), a resource path, and the source crate/trait identity, the same identity information could in
principle be discovered at runtime by introspecting a device's API (as `ww_self` is designed to do, see
[std library overview](../std_library/overview.md)) and used to build an `Attachment` by hand, without any
per-device generated code, wiring up a trait client dynamically to whatever resource path advertises a compatible
global trait.

**This dynamic-introspection path is not implemented yet** - today, obtaining an attachment always goes through
codegen-produced `.attachment()` methods.
