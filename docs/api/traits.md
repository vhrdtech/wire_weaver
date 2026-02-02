# Traits (levels)

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

# Traits (global)

The idea behind global traits is to leverage crates.io to define a set of common traits used across many devices.
Device can then implement all the traits it needs and on the client side, common code can be used to control
similar functionality of different devices.

Traits generic enough to be global and currently planned are:

* FirmwareUpdate
* EmbeddedLog
* BoardInfo
* Counters
* Gpio
* DeviceUserInfo
* RegisterAccess
* CanBus

Device API, instead of re-implementing the same things over and over, can the look like follows:

```rust
#[ww_trait]
trait MyAwesomeDevice {
    ww_impl!(firmware_update: ww_firmware_update "0.1.0" :: FirmwareUpdate);
    ww_impl!(board_info: ww_board_info "0.1.0" :: BoardInfo);
    // and some device specific functionality in addition to common things
}
```

Client code can be written in a completely agnostic way, e.g., only capable of interacting with `FirmwareUpdate` trait,
regardless of which exact device it is implemented on or how it is physically connected.

One can also interact with devices using trait-addressing mode, e.g., calling `set_indication_mode(Mode::Night)` on all
devices on a CAN bus, putting all boards with LEDs into night mode. More on that on
the [addressing page](./addressing.md)
