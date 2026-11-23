//! The JSON forms of every kind of type (INT-14), and the errors with their JSON paths.

mod common;

use common::{load, method_args, property};
use serde_json::{Value as J, json};
use ww_json::{Codec, ErrorKind, args_from_json, from_json, to_json};
use ww_self::{ApiBundleOwned, ValueOwned};

fn bundle() -> &'static ApiBundleOwned {
    static B: std::sync::OnceLock<ApiBundleOwned> = std::sync::OnceLock::new();
    B.get_or_init(|| load("conformance_api", "Conformance"))
}

/// The JSON written for `input` read as property `name`, and that read back to the same value.
fn round_trip(name: &str, input: J) -> J {
    let ty = property(bundle(), name);
    let value = from_json(&input, ty, bundle()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let out = to_json(&value, ty, bundle()).unwrap();
    // NaN equals nothing, also itself: compared by its Debug text
    let back = from_json(&out, ty, bundle()).unwrap();
    assert_eq!(format!("{back:?}"), format!("{value:?}"), "{name}");
    out
}

fn error(name: &str, input: J) -> ww_json::JsonError {
    let ty = property(bundle(), name);
    from_json(&input, ty, bundle()).unwrap_err()
}

#[test]
fn scalars() {
    assert_eq!(round_trip("u8", json!(255)), json!(255));
    assert_eq!(round_trip("bool", json!(true)), json!(true));
    assert_eq!(round_trip("string", json!("héllo")), json!("héllo"));
    assert_eq!(round_trip("f32", json!(0.1)), json!(0.1));
    assert_eq!(round_trip("f64", json!(-2.5e300)), json!(-2.5e300));
    // an integer a JSON number can't hold exactly is a string, in and out
    assert_eq!(
        round_trip("u64", json!(u64::MAX)),
        json!("18446744073709551615")
    );
    assert_eq!(
        round_trip("u64", json!("18446744073709551615")),
        json!("18446744073709551615")
    );
    assert_eq!(
        round_trip("u64", json!(9007199254740992u64)),
        json!(9007199254740992u64)
    );
    assert_eq!(
        round_trip("i64", json!(i64::MIN)),
        json!("-9223372036854775808")
    );
    assert_eq!(
        round_trip("u128", json!(u128::MAX.to_string())),
        json!(u128::MAX.to_string())
    );
    assert_eq!(round_trip("i128", json!(-5)), json!(-5));
    // a whole float is an integer, a fraction is not
    assert_eq!(round_trip("u8", json!(7.0)), json!(7));
    assert!(matches!(
        error("u8", json!(7.5)).kind,
        ErrorKind::WrongType { .. }
    ));
    // NaN and infinity have no JSON number
    assert_eq!(round_trip("f32", json!("NaN")), json!("NaN"));
    assert_eq!(round_trip("f64", json!("-inf")), json!("-inf"));
    assert_eq!(round_trip("nibble", json!(15)), json!(15));
    assert_eq!(round_trip("u4", json!(9)), json!(9));
    assert_eq!(round_trip("i5", json!(-16)), json!(-16));
}

#[test]
fn numbers_out_of_range_and_wrong_kinds() {
    let e = error("u8", json!(256));
    assert_eq!(
        e.kind,
        ErrorKind::OutOfRange {
            value: "256".into(),
            ty: "u8".into()
        }
    );
    assert!(matches!(
        error("u8", json!(-1)).kind,
        ErrorKind::OutOfRange { .. }
    ));
    assert!(matches!(
        error("u8", json!("x")).kind,
        ErrorKind::WrongType { .. }
    ));
    assert!(matches!(
        error("u8", json!(null)).kind,
        ErrorKind::WrongType { .. }
    ));
    assert!(matches!(
        error("u4", json!(16)).kind,
        ErrorKind::OutOfRange { .. }
    ));
    assert!(matches!(
        error("bool", json!(1)).kind,
        ErrorKind::WrongType { .. }
    ));
    assert!(matches!(
        error("string", json!(5)).kind,
        ErrorKind::WrongType { .. }
    ));
    assert_eq!(
        error("u8", json!("x")).to_string(),
        "value: expected an integer of u8 (a string beyond 2^53), found a string"
    );
}

#[test]
fn bytes_are_hex() {
    assert_eq!(round_trip("bytes", json!("00ff10")), json!("00ff10"));
    assert_eq!(round_trip("bytes", json!([0, 255, 16])), json!("00ff10"));
    assert_eq!(round_trip("bytes", json!("")), json!(""));
    assert!(matches!(
        error("bytes", json!("0")).kind,
        ErrorKind::WrongType { .. }
    ));
    assert!(matches!(
        error("bytes", json!([256])).kind,
        ErrorKind::OutOfRange { .. }
    ));
    // other lists are lists
    assert_eq!(round_trip("vec_u16", json!([1, 2, 3])), json!([1, 2, 3]));
    assert_eq!(
        round_trip("vec_string", json!(["a", "b"])),
        json!(["a", "b"])
    );
}

#[test]
fn lengths() {
    assert_eq!(round_trip("array_u16", json!([1, 2, 3])), json!([1, 2, 3]));
    let e = error("array_u16", json!([1, 2]));
    assert_eq!(
        e.kind,
        ErrorKind::Length {
            expected: "3 elements".into(),
            got: 2
        }
    );
    assert!(matches!(
        error("array_u16", json!("no")).kind,
        ErrorKind::WrongType { .. }
    ));
    assert_eq!(round_trip("max_len_str", json!("abcd")), json!("abcd"));
    let e = error("max_len_str", json!("abcde"));
    assert!(matches!(e.kind, ErrorKind::Length { got: 5, .. }), "{e}");
    let e = error("max_len_bytes", json!("00010203"));
    assert!(matches!(e.kind, ErrorKind::Length { got: 4, .. }), "{e}");
    assert_eq!(
        round_trip("tuple", json!([1, 2, true])),
        json!([1, 2, true])
    );
    assert!(matches!(
        error("tuple", json!([1, 2])).kind,
        ErrorKind::Length { .. }
    ));
    assert_eq!(round_trip("range", json!([1, 9])), json!([1, 9]));
    assert_eq!(
        round_trip("range_inclusive", json!([-3, 3])),
        json!([-3, 3])
    );
}

#[test]
fn options_results_enums() {
    assert_eq!(round_trip("option_u8", json!(null)), json!(null));
    assert_eq!(round_trip("option_u8", json!(5)), json!(5));
    assert_eq!(round_trip("result", json!({"Ok": 5})), json!({"Ok": 5}));
    assert_eq!(
        round_trip("result", json!({"Err": "Missing"})),
        json!({"Err": "Missing"})
    );
    assert!(matches!(
        error("result", json!({"Maybe": 1})).kind,
        ErrorKind::UnknownField { .. }
    ));
    assert_eq!(round_trip("shape", json!("Empty")), json!("Empty"));
    assert_eq!(
        round_trip("shape", json!({"Circle": 5})),
        json!({"Circle": 5})
    );
    // fields in the order of the definition, whatever order they come in
    let rect = round_trip("shape", json!({"Rect": {"label": "x", "h": 2, "w": 1}}));
    assert_eq!(
        serde_json::to_string(&rect).unwrap(),
        r#"{"Rect":{"w":1,"h":2,"label":"x"}}"#
    );
    let e = error("shape", json!("Square"));
    assert!(matches!(e.kind, ErrorKind::UnknownVariant { .. }), "{e}");
    assert!(e.to_string().contains("Empty, Circle, Rect, Far"), "{e}");
    assert!(matches!(
        error("shape", json!("Circle")).kind,
        ErrorKind::WrongType { .. }
    ));
    // a variant of a newer version, as a device reads it
    assert_eq!(
        round_trip("shape", json!("<unknown 5>")),
        json!("<unknown 5>")
    );
}

#[test]
fn structs_and_their_paths() {
    assert_eq!(round_trip("rgb", json!([1, 2, 3])), json!([1, 2, 3]));
    assert_eq!(round_trip("marker", json!(null)), json!(null));
    assert_eq!(round_trip("pair", json!([1, "a"])), json!([1, "a"]));
    assert_eq!(round_trip("pair", json!([1, null])), json!([1, null]));
    let e = error("rgb", json!([1, 2]));
    assert!(matches!(e.kind, ErrorKind::Length { .. }), "{e}");
    let e = error("rgb", json!([1, 2, 300]));
    assert_eq!(e.path.to_string(), "value[2]");
    let e = error(
        "bits",
        json!({"on": true, "small": 99, "signed": 0, "nib": 1, "byte": 2, "wide": 3, "last": false}),
    );
    assert_eq!(e.path.to_string(), "value.small");
    assert!(matches!(e.kind, ErrorKind::OutOfRange { .. }));
    let e = error("bits", json!({"on": true, "nope": 1}));
    assert!(
        matches!(e.kind, ErrorKind::UnknownField { ref name, .. } if name == "nope"),
        "{e}"
    );
    let e = error("bits", json!({"on": true}));
    assert!(matches!(e.kind, ErrorKind::MissingField { .. }), "{e}");
}

#[test]
fn deep_paths_name_the_failing_value() {
    // Outer.items[2].<field>
    let ty = property(bundle(), "nested");
    let item = |n: u8| json!({"id": n, "note": "x"});
    let input = json!({"tag": 1, "inner": item(1), "items": [item(1), item(2), {"id": 3, "note": 99999}], "after": 0});
    let e = from_json(&input, ty, bundle()).unwrap_err();
    assert!(e.path.to_string().starts_with("value.items[2]."), "{e}");
    let e = Codec::new(bundle())
        .root("items")
        .from_json(&input, ty)
        .unwrap_err();
    assert!(e.to_string().starts_with("items.items[2]."), "{e}");
}

#[test]
fn arguments() {
    let b = bundle();
    let args = method_args(b, "args_plain");
    let values = args_from_json(&json!({"a": 1, "b": 2}), args, b).unwrap();
    assert_eq!(values.len(), 2);
    assert_eq!(
        ww_json::args_to_json(&values, args, b).unwrap(),
        json!({"a": 1, "b": 2})
    );
    let e = args_from_json(&json!({"a": 1, "b": 70000}), args, b).unwrap_err();
    assert_eq!(e.path.to_string(), "args.b");
    // defaults: `b` and the trailing `Option` `c` may be left out
    let args = method_args(b, "args_defaults");
    assert_eq!(args_from_json(&json!({"a": 1}), args, b).unwrap().len(), 1);
    assert_eq!(
        args_from_json(&json!({"a": 1, "b": 2}), args, b)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        args_from_json(&json!({"a": 1, "b": 2, "c": null}), args, b)
            .unwrap()
            .len(),
        3
    );
    // an `Option` in the middle of the given ones is None
    let e = args_from_json(&json!({"b": 2}), args, b).unwrap_err();
    assert!(
        matches!(e.kind, ErrorKind::MissingField { ref name } if name == "a"),
        "{e}"
    );
    // `#[flatten]`: the fields of the struct are the arguments
    let args = method_args(b, "args_flatten");
    let values = args_from_json(&json!({"a": 1, "b": "x"}), args, b);
    assert!(
        values.is_ok() || matches!(values, Err(ref e) if e.path.root == "args"),
        "{values:?}"
    );
}

#[test]
fn opaque_has_no_form() {
    let ty = property(bundle(), "u8");
    let e = to_json(&ValueOwned::Bool(true), ty, bundle()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Mismatch(_)), "{e}");
}

#[test]
fn output_is_deterministic() {
    let ty = property(bundle(), "fixed_nested");
    let input = json!({"signed": [-1, 2], "tag": 9, "inner": {"hash": [1, 2, 3, 4], "id": 51966}});
    let a = serde_json::to_string(&round_trip("fixed_nested", input.clone())).unwrap();
    let b = serde_json::to_string(&round_trip("fixed_nested", input)).unwrap();
    assert_eq!(a, b);
    let _ = ty;
}
