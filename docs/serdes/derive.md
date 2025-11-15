# Derive

Writing out serializing and deserializing code by hand would be very tedious and error-prone. So a procedural macro
is provided that can create all the code.

## Prerequisites

All examples below assume that wire_weaver dependency is added in Cargo.toml: `wire_weaver = "0.4.0"`.

#### Structs

Simple example on how to automatically get serializing and deserializing code generated for a struct:

```rust
use wire_weaver::prelude::*;

#[derive_shrink_wrap]
#[derive(Debug, PartialEq)]
struct CoordV1 {
    x: u8,
    y: u8
}

fn derive_on_struct() {
    let mut buf = [0u8; 64];
    let coord = CoordV1 { x: 0xAA, y: 0xCC };
    let bytes = to_ww_bytes(&mut buf, &coord).unwrap();
    assert_eq!(bytes, &[0xAA, 0xCC]);
}
```

Let's evolve the type and try out the compatibility features:

```rust
#[derive_shrink_wrap]
#[derive(Debug, PartialEq)]
struct CoordV1_1 {
    x: u8,
    y: u8,
    #[default = None]
    z: Option<u8>
}

fn evolved_struct() {
    let mut buf = [0u8; 64];
    let coord = CoordV1_1 { x: 0xAA, y: 0xCC, z: Some(0xFF) };
    let bytes = to_ww_bytes(&mut buf, &coord).unwrap();
    assert_eq!(bytes, &[0xAA, 0xCC, 0x80, 0xFF]);
    // newer type from older data
    let coord: CoordV1_1 = from_ww_bytes(&[0xAA, 0xCC]).unwrap();
    assert_eq!(coord, CoordV1_1 { x: 0xAA, y: 0xCC, z: None });
    // older type from newer data
    let old_coord: CoordV1 = from_ww_bytes(bytes).unwrap();
    assert_eq!(old_coord, CoordV1 { x: 0xAA, y: 0xCC });
}
```

#### Non-evolvable types

final_structure, self_describing, sized

# Next step

Check out [API overview](../api/overview.md).