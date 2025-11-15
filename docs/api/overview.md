# High-level overview

API is defined as a collections of user-defined types and resources - methods, properties, streams and traits.
Intended crate organisation is as follows:

![API crates diagram](../assets/api_crates.svg)

* `my_device_api` crate - contains user-defined types and resources, common to firmware and it's driver (server and
  client).
  This crate must support `no_std` and optionally `std`.
* MCU firmware depends on the API crate, uses common data types and implements a server. WireWeaver generates
  serdes and dispatch code, while user provides actual implementation.
* Rust driver also depends on the API crate (optionally with std feature). WireWeaver generates serdes and client side
  code, user can optionally provide a higher-level client implementation on top of the generated one.
* CLI, GUI and other applications depend on the Rust driver crate in order to communicate with the device.
* Python wrapper is also automatically generated and uses Rust driver code.

Name of the API crate (from Cargo.toml) is assumed to be a globally unique identifier (see `ww_version::FullVersion`),
hence it is advised to eventually publish it to crates.io if you are working on an open-source project or ensure to use
unique enough name for internal use.

Version of the API crate is used for compatibility checks upon connection to the device. You can use it in your
code as well, to show proper messages to user when interacting with an older or newer firmware from the perspective of
the
driver. Normal [SemVer rules](https://semver.org) apply.

WireWeaver supports both backwards and forwards compatibility at the wire format level, but you need to ensure to follow
the [evolution rules](../evolution/rules.md) for this to work properly.

## Methods

Methods are defined using standard Rust syntax.
Any number of arguments are supported and they can be of any type (supported by SerDes).

```rust
#[ww_trait]
trait MyDevice {
    fn led_on();
    fn set_brightness(value: f32);
    fn temperature() -> f32;
    fn user_type(state: LedState);
}

#[derive_shrink_wrap]
#[ww_repr(unib32)]
pub enum LedState {
    Off,
    On,
}
```

On the server side, this is how generated server code is tied with user provided implementation:

```rust
struct ServerState {}

impl ServerState {
    async fn set_brightness(&mut self, value: f32) {
        // do things
    }
}

ww_api!(
    "../../api/src/lib.rs" as api::MyDevice for ServerState,
    server = true, no_alloc = true, use_async = true,
);
```

`ww_api` proc-macro invocation will implement `process_request_bytes` function, which takes in request bytes,
deserializes and processes them and eventually calls `set_brightness` on self.

Note that you can request blocking implementation by setting `use_async = false`. And there is also a possibility to
return values later, via a provided request id (for example if executing a method and getting a result takes a long
time).
More on that on the [detailed page](detailed.md).

## Streams

Two types of streams are supported - from server to client (`stream!`) and from client to server (`sink!`).
I.e., naming is from the perspective of the device (node) - stream out, sink in.

```rust
#[ww_trait]
trait MyDevice {
    stream!(byte: u8);
    sink!(word: u32);
    stream!(slice: Vec<u8>);
    sink!(user_defined: Vec<LedState>);
}
```

Any type supported by the SerDes system works with streams as well. Streams can be used for many things, e.g., sending
status updates or bytes from USART, frames to be transmitted on CAN bus, etc.

Streams can have a beginning and an end, for example to implement a file IO or firmware update, to deal with small
chunks
at a time and yet be able to signal a completion event. It is also possible to send a user defined delimiter, to be
delivered in order with stream data, that can be used to implement frame synchronisation.

Another useful property of streams is that they work on object level. For the `slice` stream in the example above,
each individual array size is guaranteed to be preserved, even if multiple stream updates are transferred together at
transport level. Sending `[1, 2, 3]`, `[4]`, `[5, 6]` will result in the same arrays received on the other end, in the
same order.

You can subscribe to stream updates, in an asynchronous or blocking manner, see more on
the [detailed page](detailed.md).

## Properties

Properties of any type can be defined as follows:

```rust
#[ww_trait]
trait MyDevice {
    property!(ro button_pressed: bool);
    property!(rw speed: f32);
}
```

Properties have access mode associated with them:

* Const (`const`) - property is not going to change, observe not available
* Read only (`ro`) - property can only be read, can change and be observed for changes
* Write only (`wo`) - property can only be written
* Read/Write (`rw`) - property can be read, written and observed for changes

There are two supported way of implementing properties on the server side:

* get / set - user code provides `get_speed` and `set_speed` implementation.
* value / on_changed - generated code directly reads and writes `speed` field and calls user provided `speed_changed`
  implementation.

## Traits

Traits in WireWeaver are used to define API blocks, as you can see from examples above, entry point for a device API is
also a trait. They carry similar meaning to Rust traits, in a sense that trait defines some functionality, that server
"implements" and client code can then interact with.

But they are not actually traits under the hood, `#[ww_trait]` macro leaves only some static checks and removes the
rest.
Rust syntax is currently used to bypass writing a whole parser from scratch.
All the magic happens through code generation in the `#[ww_api]` macro.

### Traits for API resources grouping

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

### Traits (global) for extracting common functionality

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
    ww_impl!(firmware_update: "firmware_update:0.1.0 :: FirmwareUpdate");
    ww_impl!(board_info: "board_info:0.1.0 :: BoardInfo");
    // and some device specific functionality in addition to common things
}
```

Client code can be written in a completely agnostic way, e.g., only capable of interacting with `FirmwareUpdate` trait,
regardless of which exact device it is implemented on or how it is physically connected.

One can also interact with devices using trait-addressing mode, e.g., calling `set_indication_mode(Mode::Night)` on all
devices on a CAN bus, putting all boards with LEDs into night mode. More on that on
the [addressing page](./addressing.md)

## Resource arrays

Any resource can also be an array - method, property, stream and even a trait implementation:

```rust
#[ww_trait]
trait ArrayOf {
    fn run<N: u32>();
    stream!(adc[]: u16);
    property!(led[]: bool);
    ww_impl!(motor[]: ww_motor_control::Motor);
}
```

TODO: size bounds

Traits inside other traits can also contain arrays, all the indices leading up to them are accumulated and passed as
Rust array `[u32; N]` argument into a corresponding user handler.

That way generated code can be kept efficient and simple, because the whole API tree is essentially flattened and
simple function calls are used to interface with user provided implementation. At least that is the case for now on
`no_std` targets.

### Array of resources vs resource of array

Here, resource led is itself an array, when accessing it - an index will be added to the resource path.
Each one of three bool's is accessed separately from each other.

```rust
#[ww_trait]
trait ArrayOfResources {
    property!(led[3]: bool);
}
```

On the other hand, here led is not an array, but its type is. All three boolean's are accessed in one go.

```rust
#[ww_trait]
trait ResourceOfArrays {
    property!(led: [bool; 3]);
}
```

Both can be used together as well, for example:

```rust
#[ww_trait]
trait ArrayOfArrays {
    property!(rgb_led[3]: [u8; 3]);
}
```