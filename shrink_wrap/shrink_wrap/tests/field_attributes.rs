//! Attributes the derive doesn't read on fields and variants (`#[serde(..)]`, `#[cfg_attr(.., serde(..))]`) stay on
//! the generated type that has the item's own name, as written; rustc expands the `cfg_attr`s there.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(owned, derive(Debug, PartialEq, serde::Serialize))]
struct Config {
    #[serde(rename = "r")]
    rate: u16,
    #[cfg_attr(any(), serde(rename = "never"))]
    level: u8,
    #[cfg_attr(all(), serde(rename = "t"))]
    #[default = 7]
    timeout: u16,
}

#[derive_shrink_wrap(owned, ww_repr = u4, derive(Debug, PartialEq, serde::Serialize))]
enum Shape {
    #[serde(rename = "c")]
    Circle {
        #[cfg_attr(all(), serde(rename = "radius"))]
        r: u8,
    },
    Square(#[serde(serialize_with = "doubled")] u8),
}

fn doubled<S: serde::Serializer>(v: &u8, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u16(*v as u16 * 2)
}

/// The borrowed type has the item's name: the owned mirror doesn't derive serde, so it can't take the attribute.
#[derive_shrink_wrap(borrowed, owned, derive_borrowed(serde::Serialize))]
struct Named<'i> {
    #[serde(rename = "n")]
    name: &'i str,
}

#[test]
fn struct_fields() {
    let json = serde_json::to_string(&Config {
        rate: 1,
        timeout: 2,
        level: 3,
    })
    .unwrap();
    assert_eq!(json, r#"{"r":1,"level":3,"t":2}"#);
}

#[test]
fn enum_variants_and_fields() {
    let circle = serde_json::to_string(&Shape::Circle { r: 4 }).unwrap();
    assert_eq!(circle, r#"{"c":{"radius":4}}"#);
    let square = serde_json::to_string(&Shape::Square(5)).unwrap();
    assert_eq!(square, r#"{"Square":10}"#);
}

#[test]
fn only_on_the_type_with_the_items_name() {
    let json = serde_json::to_string(&Named { name: "x" }).unwrap();
    assert_eq!(json, r#"{"n":"x"}"#);
    let mut buf = [0u8; 8];
    let bytes = Named { name: "x" }.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(
        NamedOwned::from_ww_bytes_owned(&bytes).unwrap().name,
        "x".to_string()
    );
}
