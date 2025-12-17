# Derive

Writing out serializing and deserializing code by hand would be very tedious and error-prone. So a procedural macro
is provided that can create all the code.

## Prerequisites

All examples below assume that wire_weaver dependency is added in Cargo.toml: `wire_weaver = "0.4.0"`.

## Structs

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

## Zero-copy and owned types

Often there is a need to serialize owned type into a buffer and deserialize it as borrowed type on `no_std` without
allocation or vice versa.
Typing out two definitions, one using borrowed data (`RefVec<'i, T>`, `&'i str`, etc.) and one owned would be very
error-prone.
Thus, `derive_shrink_wrap` attribute macro supports `#[owned = "feature-name"]` argument, that will trigger automatic
generation of owned type definition and respective serialization and deserialization code.

For example:

```rust
#[derive_shrink_wrap]
#[owned = "std"]
pub struct FullVersion<'i> {
    pub crate_id: &'i str,
    pub version: Version<'i>,
}
```

Will generate `FullVersionOwned` along with serdes code that matches borrowed variant bit-to-bit.

```rust
#[cfg(feature = "std")]
pub struct FullVersionOwned {
    pub crate_id: String,
    pub version: VersionOwned,
}
```

Pseudo-code usage example:

```rust
fn round_trip() {
    // on no_std, no allocator
    let v = FullVersion { .. };
    let bytes = v.to_ww_bytes(&mut buf).unwrap();

    // on host, with allocator
    let v_owned = FullVersionOwned::from_ww_bytes(bytes).unwrap();
    assert_eq!(v.to_owned(), v_owned);

    let bytes_from_owned = v_owned.to_ww_bytes(&mut buf2).unwrap();
    assert_eq!(bytes, bytes_from_owned);

    // again on no_std
    let v_ref = FullVersion::from_ww_bytes(bytes_from_owned).unwrap();
    assert_eq!(v, v_ref);
}
```

### Type mapping

| Borrowed type    | Owned equivalent |
|------------------|------------------|
| `RefVec<'i, u8>` | `Vec<u8>`        |
| `&'i str`        | `String`         |
| `RefBox<'i, T>`  | `Box<T>`         |
| `UserType<'i>`   | `UserTypeOwned`  |

## Non-evolvable types

final_structure, self_describing, sized

# Next step

Check out [API overview](../api/overview.md).