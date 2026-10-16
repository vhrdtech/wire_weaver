---
level: nomicon
---

# Trait attachments

Host code written for a reusable trait (a `ww_gpio::Bank`, a `ww_dfu::BootControl`) shouldn't need to know which
device it talks to, under which path the trait sits there, or how the device is connected. An **attachment** is
what such code takes instead of a device.

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

`device.port(0)` only exists because `all_gpio_api::AllGpioApi` happens to declare `ww_impl!(port[]: ww_gpio::Bank)`:
it is `all_gpio`-specific generated code. `.attachment()` erases that specificity: `BankBlocking::new` never sees
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

## Attaching without generated client code

`.attachment()` is only available because the device's own API crate was run through `ww_codegen!`, which is what
generates `device.port(0)` (and every other accessor) in the first place. When that generated code is available, it
is the convenient way to obtain an attachment.

Without it, `DynClient::global_attachment(gid, trait_name)` gives an attachment that addresses the trait by its
crate's name and version instead of a path: the device finds the trait wherever it sits in its tree. That needs no
code generated for the device, works whatever its API version, and is how `ww dfu reboot` reaches any device's
`ww_dfu::BootControl` (see [global trait addressing](addressing.md#global-trait-addressing-fullversion)). It
can't reach a trait under a resource array (`port[]`), since a global path has no index.

Finding such a trait's absolute path from the device's introspection data ([ww_self](../std_library/ww_self.md))
and building a path attachment from it, for arrays, is not implemented yet.
