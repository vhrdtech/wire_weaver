//! DER-17: a `#[default = ..]` field also defaults in serde, so serde data written before the field was added
//! (a saved RON or JSON file) still reads, as shrink_wrap data does.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(
    cfg_attr(all(), derive(serde::Deserialize, serde::Serialize)),
    derive(Debug, PartialEq)
)]
struct ConfigOwned {
    rate: u16,
    #[default = 7]
    timeout: u16,
    #[default = None]
    retries: Option<u8>,
    #[default = Some(3)]
    level: Option<u8>,
    #[default = Vec::new()]
    tags: Vec<u8>,
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq, serde::Deserialize))]
struct Plain {
    #[default = true]
    on: bool,
}

#[derive_shrink_wrap(
    owned,
    cfg_attr(all(), derive(serde::Deserialize)),
    derive(Debug, PartialEq)
)]
struct Generic<T: SerializeShrinkWrap + DeserializeShrinkWrapOwned> {
    value: T,
    #[default = 5]
    count: u8,
}

#[derive_shrink_wrap(
    owned,
    ww_repr = u4,
    cfg_attr(all(), derive(serde::Deserialize)),
    derive(Debug, PartialEq)
)]
enum Shape {
    Circle {
        r: u8,
        #[default = 9]
        fill: u8,
    },
    Dot,
}

#[test]
fn missing_fields_take_their_defaults() {
    let config: ConfigOwned = serde_json::from_str(r#"{"rate": 1000}"#).unwrap();
    assert_eq!(
        config,
        ConfigOwned {
            rate: 1000,
            timeout: 7,
            retries: None,
            level: Some(3),
            tags: vec![],
        }
    );
    let plain: Plain = serde_json::from_str("{}").unwrap();
    assert_eq!(plain, Plain { on: true });
    let generic: Generic<u16> = serde_json::from_str(r#"{"value": 2}"#).unwrap();
    assert_eq!(generic, Generic { value: 2, count: 5 });
    let shape: Shape = serde_json::from_str(r#"{"Circle": {"r": 1}}"#).unwrap();
    assert_eq!(shape, Shape::Circle { r: 1, fill: 9 });
}

#[test]
fn present_fields_are_read() {
    let config: ConfigOwned = serde_json::from_str(
        r#"{"rate": 1, "timeout": 2, "retries": 3, "level": null, "tags": [4]}"#,
    )
    .unwrap();
    assert_eq!(
        config,
        ConfigOwned {
            rate: 1,
            timeout: 2,
            retries: Some(3),
            level: None,
            tags: vec![4],
        }
    );
}

#[test]
fn fields_without_a_default_stay_required() {
    assert!(serde_json::from_str::<ConfigOwned>(r#"{"timeout": 2}"#).is_err());
}
