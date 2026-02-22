# shrink_wrap

![Crates.io Version](https://img.shields.io/crates/v/shrink_wrap)
[![CI](https://github.com/romixlab/shrink_wrap/actions/workflows/rust.yml/badge.svg)](https://github.com/romixlab/shrink_wrap/actions/workflows/rust.yml)
[![codecov](https://codecov.io/github/romixlab/shrink_wrap/graph/badge.svg?token=PNRZ4BA0H3)](https://codecov.io/github/romixlab/shrink_wrap)

<p align="center">
<img src="https://github.com/romixlab/shrink_wrap/blob/main/assets/logo-256.png?raw=true" alt="logo"/>
</p>

> Compact zero-copy wire format for microcontrollers using no allocator and supporting dynamic types.

## Design goals

* Support for user-defined types - structs, enums (plain and with data fields), tuples.
* Support for dynamically sized objects (strings, vectors or any user type).
* Rich type system.
    * [Full list of supported types](https://vhrdtech.github.io/wire_weaver/types)
* Backwards and Forwards compatibility (older device with new software / old software with newer device).
* No allocation.
* 1-bit, 4-bit and 1-byte alignment.
* No buffer alignment requirements (1 byte).
* Self-referential types.
* Support for data and protocol evolution.
* Dense wire format that can fit useful things into small messages (e.g., CAN Bus frame).
* Zero-copy de-serialization.
* `StackVec` for storing an object with arbitrary max-bound size on stack.

## Where used

This crate is a core piece of [WireWeaver](https://github.com/vhrdtech/wire_weaver) - lightweight microcontroller API
code generator with support for methods (RPC), properties,
streams, [global traits](https://github.com/vhrdtech/ww_stdlib) and backward/forward compatibility.

## Recommended way to depend on this crate

### On no_std

```toml
[dependencies]
shrink_wrap = { version = "0.1.0", default-features = false }
```

To make this crate `no_std`.

### In API or data type crates for both std and no_std

```toml
[dependencies]
shrink_wrap = { version = "0.1.0", default-features = false }

[features]
default = ["std"]
std = ["shrink_wrap/std"]
```

And put `#![cfg_attr(not(feature = "std"), no_std)]` in `lib.rs`.
Then use your crate with `default-features = false` on `no_std`.

## Implementation details

* Attribute macro that generates serdes code.
    * `#[owned = "feature"]` attribute to generate TyOwned from Ty<'i> and serdes code for it as well.
* `BufWriter` and `BufReader` that do low-level work with byte buffers, can be used stand-alone as well.
* Main trick is keeping lengths of objects in the back of the buffer and read/write from both ends.
* In order to avoid temporary allocations (or two buffers) during serialization, back of the buffer is used as a stack
  as well.
* Two forms of variable length numbers are used - UNib32 and ReverseUNib32 (nibble based).
* Rust type system and const evaluation is leveraged to automatically compose objects inside of objects:
    * Sized and SelfDescribing objects are serialized as is, without storing length.
    * FinalStructure types (e.g., `Vec<T>`) pick-up `T`’s kind and become it to save even more space.
    * Unsized - size is calculated and stored, can be evolved, default kind for user structs and enums.
* `U1`..`U64` and `I2`..`I64` bit-aligned types are provided as well.
* Can use, e.g., `U1` which takes only one bit as enum discriminant!
* Can use uninitialised buffers when writing, so no need to spend time on zero-filling.
* Downside is - buffer length need to be known, usually not a problem or taken care of by transport layer.
