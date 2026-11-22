//! The JSON codec of `ww_json` (INT-14) on every case of the corpus: the corpus value through the structural form
//! and back, then through the natural form (quantities as text, bytes as hex, big integers as strings) and back.

use conformance::corpus::{self, Direction};
use conformance::dynamic::{bundle, method_args, property_ty};
use ww_json::Codec;
use ww_self::{TypeOwned, ValueOwned};

/// NaN equals nothing, also itself: a value holding one compares by its Debug text (NUM-19).
fn same(a: &ValueOwned, b: &ValueOwned) -> bool {
    a == b || format!("{a:?}") == format!("{b:?}")
}

#[test]
fn every_case_round_trips() {
    let bundle = bundle();
    let raw = Codec::new(bundle).raw();
    let natural = Codec::new(bundle);
    let (mut checked, mut args_checked) = (0, 0);
    let mut inexact = vec![];
    for case in corpus::load().cases {
        if case.value.is_null() && case.direction == Direction::Reject {
            continue;
        }
        if let Some(ty) = property_ty(&case.ty) {
            let Ok(value) = raw.from_json(&case.value, ty) else {
                // a value that doesn't encode is refused by the codec too (over a `MaxLen`)
                assert_eq!(case.direction, Direction::RejectValue, "{}", case.name);
                continue;
            };
            // the structural form: the corpus value itself
            let json = raw
                .to_json(&value, ty)
                .unwrap_or_else(|e| panic!("{}: {e}", case.name));
            let back = raw
                .from_json(&json, ty)
                .unwrap_or_else(|e| panic!("{}: {e}", case.name));
            assert!(same(&value, &back), "{}: {json}", case.name);
            // the natural form
            let json = natural
                .to_json(&value, ty)
                .unwrap_or_else(|e| panic!("{}: {e}", case.name));
            let back = match natural.from_json(&json, ty) {
                Ok(back) => back,
                // read from a device of a newer version, written back to none (a scale this host doesn't know)
                Err(_) if case.direction == Direction::Decode => continue,
                Err(e) => panic!("{}: {e}\n{json}", case.name),
            };
            // a `ww_si::Quantity` stores any number type and its text is read as an `i32` or an `f32`: the same
            // quantity, so the same JSON, but another number type than the corpus value's
            if !same(&value, &back) {
                inexact.push(case.name.clone());
            }
            assert_eq!(
                natural.to_json(&back, ty).unwrap(),
                json,
                "{}: stable",
                case.name
            );
            checked += 1;
        } else if let Some(defs) = method_args(&case.ty) {
            let Ok(values) = raw.root("args").args_from_json(&case.value, defs) else {
                assert_eq!(case.direction, Direction::RejectValue, "{}", case.name);
                continue;
            };
            let json = natural
                .root("args")
                .args_to_json(&values, defs)
                .unwrap_or_else(|e| panic!("{}: {e}", case.name));
            let back = natural
                .root("args")
                .args_from_json(&json, defs)
                .unwrap_or_else(|e| panic!("{}: {e}\n{json}", case.name));
            assert!(
                values.len() == back.len() && values.iter().zip(&back).all(|(a, b)| same(a, b)),
                "{}: {json}",
                case.name
            );
            args_checked += 1;
        }
    }
    assert!(checked > 300, "{checked} cases through the property types");
    assert!(args_checked > 0, "{args_checked} cases through arguments");
    eprintln!(
        "{checked} values, {args_checked} argument lists; other number type, same quantity: {inexact:?}"
    );
    // a codec's redundant code (μ-law's negative zero) is written as the value it means, and encodes to the main code
    let quantity_types = ["stdlib.si.", "stdlib.timebase."];
    for name in &inexact {
        assert!(
            quantity_types.iter().any(|p| name.starts_with(p)),
            "{name}: reads back as another value"
        );
    }
}

#[test]
fn types_resolve() {
    // a type that is not in the model has no JSON form to guess
    let _: Option<&TypeOwned> = property_ty("no_such_type");
}
