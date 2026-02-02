# High-level overview

API is defined as a collections of user-defined types and resources - methods, properties, streams and traits.
Multiple levels are supported. Common functionality (logging, GPIO, USART, etc.) can be extracted into external crates,
see [ww_stdlib](https://github.com/vhrdtech/ww_stdlib). Both kind
of levels can be defined as arrays (e.g., array of GpioPin's).
Arbitrary user data [types](../types.md) are supported as well, backed
by [ShrinkWrap](https://crates.io/crates/shrink_wrap).

Generated server code is completely IO-free (sans-IO), all communication with the USB or network is handled separately.
For example USB driver and event loop is located in a
[wire_weaver_usb_embassy](https://crates.io/crates/wire_weaver_usb_embassy) crate.
Generated std client code uses `tokio` and it's channels under the hood. Asynchronous, blocking and promise flavors are
supported.
Client generation for no_std is not yet implemented, but planned.

WireWeaver supports both backwards and forwards compatibility at the wire format level, but you need to ensure to follow
the [evolution rules](../evolution/rules.md) for it to work properly.

## Methods (RPC)

```rust
#[ww_trait]
trait MyDevice {
    fn no_args();
    fn plain_arg(value: f32);
    fn plain_ret() -> f32;
    fn user_type(state: State);
    fn user_ret() -> UserComplex<'i>;
    fn fallible() -> Result<UserComplex<'i>, UserError>;
}
```

Any number of arguments are supported. Arguments can be evolved in the same way as structs (e.g., new_arg: `Option<T>`
can
be added to the end).

Return type can also be added later in a backwards+forwards -compatible way, e.g., an `Option<T>` or `Vec<T>`.

More on [methods page](methods.md).

## Streams

More on [streams page](streams.md).

## Properties

More on [properties page](properties.md).

## Traits

More on [traits page](traits.md).

## Resource arrays

More on [arrays page](arrays.md).
