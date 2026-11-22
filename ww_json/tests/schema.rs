//! The JSON Schema of types, arguments and items (INT-15): its shape, and that what the codec writes validates.

mod common;

use common::{load, method_args, property, quantities};
use serde_json::{Value as J, json};
use ww_json::schema::{DIALECT, args_schema, item_schemas, type_schema};
use ww_json::{Codec, args_from_json};
use ww_self::{ApiBundleOwned, ApiItemKindOwned};

fn conformance() -> &'static ApiBundleOwned {
    static B: std::sync::OnceLock<ApiBundleOwned> = std::sync::OnceLock::new();
    B.get_or_init(|| load("conformance_api", "Conformance"))
}

fn validator(schema: &J) -> jsonschema::Validator {
    jsonschema::validator_for(schema).unwrap_or_else(|e| panic!("not a schema: {e}\n{schema:#}"))
}

/// The definition a schema refers to, or the schema itself.
fn deref(schema: &J) -> J {
    match schema.get("$ref").and_then(J::as_str) {
        Some(r) => schema["$defs"][r.trim_start_matches("#/$defs/")].clone(),
        None => schema.clone(),
    }
}

fn schema_of(bundle: &ApiBundleOwned, name: &str) -> J {
    type_schema(property(bundle, name), bundle).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn every_item_has_a_valid_schema() {
    for bundle in [quantities(), conformance()] {
        let mut checked = 0;
        for item in &bundle.root.items {
            if matches!(item.kind, ApiItemKindOwned::Trait { .. }) {
                continue;
            }
            let schemas =
                item_schemas(item, bundle).unwrap_or_else(|e| panic!("{}: {e}", item.ident));
            for schema in [
                &schemas.arguments,
                &schemas.result,
                &schemas.value,
                &schemas.write_error,
            ]
            .into_iter()
            .flatten()
            {
                assert_eq!(schema["$schema"], DIALECT);
                validator(schema);
                checked += 1;
            }
        }
        assert!(checked > 20, "{checked} schemas");
    }
}

#[test]
fn numbers() {
    let b = conformance();
    assert_eq!(schema_of(b, "u8")["maximum"], 255);
    assert_eq!(schema_of(b, "u8")["minimum"], 0);
    assert_eq!(schema_of(b, "i8")["minimum"], -128);
    assert_eq!(schema_of(b, "u4")["maximum"], 15);
    assert_eq!(schema_of(b, "i5")["minimum"], -16);
    assert_eq!(schema_of(b, "bool")["type"], "boolean");
    // 64 bits and wider: a number up to 2^53, a string beyond
    let u64 = schema_of(b, "u64");
    let v = validator(&u64);
    assert!(v.is_valid(&json!(5)));
    assert!(v.is_valid(&json!("18446744073709551615")));
    assert!(!v.is_valid(&json!("5.5")));
    assert!(!v.is_valid(&json!(-1)));
    let f = validator(&schema_of(b, "f64"));
    assert!(f.is_valid(&json!(1.5)) && f.is_valid(&json!("NaN")) && f.is_valid(&json!("-inf")));
    assert!(!f.is_valid(&json!("maybe")));
}

#[test]
fn bytes_lists_and_limits() {
    let b = conformance();
    let bytes = validator(&schema_of(b, "bytes"));
    assert!(bytes.is_valid(&json!("00ff10")) && bytes.is_valid(&json!([0, 255])));
    assert!(!bytes.is_valid(&json!("0")) && !bytes.is_valid(&json!([256])));
    let array = validator(&schema_of(b, "array_u16"));
    assert!(array.is_valid(&json!([1, 2, 3])));
    assert!(!array.is_valid(&json!([1, 2])) && !array.is_valid(&json!([1, 2, 3, 4])));
    let s = schema_of(b, "max_len_str");
    assert_eq!(s["maxLength"], 4);
    let bytes = validator(&schema_of(b, "max_len_bytes"));
    assert!(bytes.is_valid(&json!("000102")) && !bytes.is_valid(&json!("00010203")));
    let tuple = validator(&schema_of(b, "tuple"));
    assert!(tuple.is_valid(&json!([1, 2, true])) && !tuple.is_valid(&json!([1, 2])));
}

#[test]
fn structs_enums_options() {
    let b = conformance();
    let bits = deref(&schema_of(b, "bits"));
    assert_eq!(bits["additionalProperties"], false);
    assert_eq!(bits["required"].as_array().unwrap().len(), 7);
    let order: Vec<_> = bits["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        order,
        ["on", "small", "signed", "nib", "byte", "wide", "last"]
    );
    let shape = validator(&schema_of(b, "shape"));
    assert!(shape.is_valid(&json!("Empty")));
    assert!(shape.is_valid(&json!({"Circle": 5})));
    assert!(shape.is_valid(&json!({"Rect": {"w": 1, "h": 2, "label": "x"}})));
    assert!(shape.is_valid(&json!("<unknown 5>")));
    assert!(!shape.is_valid(&json!("Square")));
    assert!(!shape.is_valid(&json!({"Rect": {"w": 1}})));
    let opt = validator(&schema_of(b, "option_u8"));
    assert!(opt.is_valid(&json!(null)) && opt.is_valid(&json!(5)) && !opt.is_valid(&json!(256)));
    let res = validator(&schema_of(b, "result"));
    assert!(res.is_valid(&json!({"Ok": 5})) && res.is_valid(&json!({"Err": "Missing"})));
    assert!(!res.is_valid(&json!({"Ok": 5, "Err": "Missing"})));
}

#[test]
fn recursive_types_are_finite() {
    let tree = schema_of(conformance(), "tree");
    assert!(!tree["$defs"].as_object().unwrap().is_empty(), "{tree:#}");
    validator(&tree);
}

#[test]
fn quantities_name_their_unit_and_bounds() {
    let b = quantities();
    let timeout = schema_of(b, "timeout");
    assert_eq!(timeout["type"], "string");
    assert_eq!(timeout["x-unit"], "ms");
    assert_eq!(timeout["x-quantity"], "fixed");
    assert!(timeout["description"].as_str().unwrap().contains("5 ms"));
    let window = schema_of(b, "window");
    assert_eq!(window["x-min"], "1 ms");
    assert_eq!(window["x-max"], "5000 ms");
    assert!(
        window["description"]
            .as_str()
            .unwrap()
            .contains("from 1 ms to 5000 ms")
    );
    assert_eq!(schema_of(b, "voltage")["x-unit"], "V");
    assert_eq!(schema_of(b, "board_temp")["x-quantity"], "point");
    assert_eq!(schema_of(b, "board_temp")["x-unit"], "°C");
    assert_eq!(schema_of(b, "sensor")["x-quantity"], "any");
    // counts are numbers, with their bounds
    let input = schema_of(b, "input");
    assert_eq!(
        (input["minimum"].clone(), input["maximum"].clone()),
        (json!(1), json!(4))
    );
    assert_eq!(schema_of(b, "frame")["maximum"], 65535);
}

#[test]
fn arguments_list_what_is_required() {
    let b = conformance();
    let schema = args_schema(method_args(b, "args_defaults"), b).unwrap();
    assert_eq!(schema["required"], json!(["a"]));
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"].as_object().unwrap().len(), 3);
    let v = validator(&schema);
    assert!(v.is_valid(&json!({"a": 1})) && v.is_valid(&json!({"a": 1, "b": 2, "c": null})));
    assert!(!v.is_valid(&json!({"b": 2})) && !v.is_valid(&json!({"a": 1, "z": 2})));
    // what the codec takes, the schema takes
    let args = method_args(b, "args_defaults");
    for input in [
        json!({"a": 1}),
        json!({"a": 1, "b": 2}),
        json!({"a": 1, "b": 2, "c": 3}),
    ] {
        assert!(v.is_valid(&input));
        let values = args_from_json(&input, args, b).unwrap();
        let out = Codec::new(b)
            .root("args")
            .args_to_json(&values, args)
            .unwrap();
        assert!(v.is_valid(&out), "{out}");
    }
    let flat = args_schema(method_args(b, "args_flatten"), b).unwrap();
    assert!(
        flat["properties"].as_object().unwrap().len() >= 2,
        "{flat:#}"
    );
}

#[test]
fn doc_comments_are_descriptions() {
    let b = quantities();
    let rail = schema_of(b, "rail");
    let _ = rail;
    let item = b.root.items.iter().find(|i| i.ident == "rail").unwrap();
    let s = item_schemas(item, b).unwrap();
    assert_eq!(
        s.description.as_deref(),
        Some("A supply rail, `0 V ..= 3.3 V` whatever prefix a value is sent with (STD-59)")
    );
    let wait = b.root.items.iter().find(|i| i.ident == "wait").unwrap();
    let s = item_schemas(wait, b).unwrap();
    assert!(s.arguments.is_some() && s.result.is_some() && s.value.is_none());
    let bound = &s.arguments.unwrap()["properties"]["delay"];
    assert_eq!(bound["x-max"], "5000000 µs");
}
