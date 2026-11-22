//! Quantities in JSON (INT-14): the text form `5 ms`, exact conversion on input, the stored number, errors.

mod common;

use common::{method_args, property, quantities};
use serde_json::json;
use shrink_wrap::SerializeShrinkWrapOwned;
use ww_json::{Codec, ErrorKind, args_from_json, args_to_json, from_json, to_json};
use ww_self::ValueOwned;
use ww_si::prefix::{Kibi, Micro, Milli};
use ww_si::units::{Byte, Second};
use ww_si::{Fixed, Volt};

fn read(property_name: &str, typed: impl SerializeShrinkWrapOwned) -> serde_json::Value {
    let bundle = quantities();
    let ty = property(bundle, property_name);
    let bytes = typed.to_ww_bytes_owned().unwrap();
    let value = ValueOwned::des_shrink_wrap_dyn(&bytes, ty, bundle).unwrap();
    let json = to_json(&value, ty, bundle).unwrap();
    // back: the same quantity. A `Quantity<U>` takes any number type (text is read as `i32` or `f32`), so the
    // bytes are the same only for what stores one (`Fixed`, counts)
    let again = from_json(&json, ty, bundle).unwrap();
    assert_eq!(
        to_json(&again, ty, bundle).unwrap(),
        json,
        "{property_name}"
    );
    if !property_name.starts_with("voltage") {
        assert_eq!(
            again.ser_shrink_wrap_dyn(ty, bundle).unwrap(),
            bytes,
            "{property_name}: {json}"
        );
    }
    json
}

fn write(property_name: &str, json: serde_json::Value) -> Result<Vec<u8>, ww_json::JsonError> {
    let bundle = quantities();
    let ty = property(bundle, property_name);
    from_json(&json, ty, bundle).map(|v| v.ser_shrink_wrap_dyn(ty, bundle).unwrap())
}

fn bytes(typed: impl SerializeShrinkWrapOwned) -> Vec<u8> {
    typed.to_ww_bytes_owned().unwrap()
}

#[test]
fn fixed_is_text_in_its_own_unit_and_scale() {
    assert_eq!(
        read("timeout", Fixed::<Second, Milli, u16>::new(5)),
        json!("5 ms")
    );
    assert_eq!(
        read("uptime", Fixed::<Second, Micro, u64>::new(1_500_000)),
        json!("1500000 µs")
    );
    assert_eq!(
        read("buffer", Fixed::<Byte, Kibi, u16>::new(4)),
        json!("4 KiB")
    );
    assert_eq!(read("voltage", Volt::milli(3300u16)), json!("3300 mV"));
}

#[test]
fn input_converts_exactly_through_the_units() {
    let five_ms = bytes(Fixed::<Second, Milli, u16>::new(5));
    assert_eq!(write("timeout", json!("5 ms")).unwrap(), five_ms);
    assert_eq!(write("timeout", json!("0.005 s")).unwrap(), five_ms);
    assert_eq!(write("timeout", json!("5000 µs")).unwrap(), five_ms);
    assert_eq!(write("timeout", json!("5000 us")).unwrap(), five_ms);
    // the plain stored number, as Python accepts it
    assert_eq!(write("timeout", json!(5)).unwrap(), five_ms);
    assert_eq!(write("timeout", json!("5")).unwrap(), five_ms);
}

#[test]
fn input_refuses_what_does_not_convert_exactly() {
    // a fraction of the stored step
    let e = write("timeout", json!("1.5 µs")).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
    assert_eq!(e.path.to_string(), "value");
    // wrong dimension
    let e = write("timeout", json!("5 V")).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
    // out of the storage
    let e = write("timeout", json!("70 s")).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
    // out of bounds, 1..=5000 ms
    let e = write("window", json!("6 s")).unwrap_err();
    assert!(e.to_string().contains("out of"), "{e}");
    // not a quantity at all
    let e = write("timeout", json!(true)).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::WrongType { .. }), "{e}");
}

#[test]
fn quantity_of_a_unit_takes_its_unit_or_a_number_of_it() {
    // `Quantity<U>` stores any number: text is read as an `i32` or an `f32`
    assert_eq!(
        write("voltage", json!("3300 mV")).unwrap(),
        bytes(Volt::milli(3300i32))
    );
    assert_eq!(
        write("voltage", json!("3.3 V")).unwrap(),
        bytes(Volt::from_f32(3.3))
    );
    // a number alone is in the unit itself
    assert!(write("voltage", json!(5)).is_ok());
    let e = write("voltage", json!("5 s")).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
}

#[test]
fn counts_are_numbers() {
    assert_eq!(
        read(
            "input",
            ww_si::IndexFrom1::<ww_si::units::Item, u8, ww_si::Range<1, 4>>::try_new(3).unwrap()
        ),
        json!(3)
    );
    let e = write("input", json!(0)).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
    let e = write("input", json!(5)).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
}

#[test]
fn arguments_by_name() {
    let bundle = quantities();
    let args = method_args(bundle, "wait");
    let values = args_from_json(&json!({"delay": "2 s"}), args, bundle).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(
        args_to_json(&values, args, bundle).unwrap(),
        json!({"delay": "2000000 µs"})
    );
    let e = args_from_json(&json!({"delay": "6 s"}), args, bundle).unwrap_err();
    assert_eq!(e.path.to_string(), "args.delay");
    let e = args_from_json(&json!({"delay": "1 s", "extra": 1}), args, bundle).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::UnknownField { .. }), "{e}");
    let e = args_from_json(&json!({}), args, bundle).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::MissingField { .. }), "{e}");
    let e = args_from_json(&json!([1]), args, bundle).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::WrongType { .. }), "{e}");
    let _ = Codec::new(bundle);
}

fn convert(
    property_name: &str,
    json: serde_json::Value,
) -> Result<serde_json::Value, ww_json::JsonError> {
    let bundle = quantities();
    let ty = property(bundle, property_name);
    from_json(&json, ty, bundle).and_then(|v| to_json(&v, ty, bundle))
}

#[test]
fn points_are_shown_in_their_origin_and_accepted_in_it_or_in_the_unit() {
    // raw / 256 + 25 °C
    assert_eq!(
        convert("board_temp", json!("25 °C")).unwrap(),
        json!("25 °C")
    );
    assert_eq!(
        convert("board_temp", json!("298.15 K")).unwrap(),
        json!("25 °C")
    );
    assert_eq!(
        convert("board_temp", json!("26.00390625 °C")).unwrap(),
        json!("26.00390625 °C")
    );
    assert_eq!(
        convert("heading", json!("90.25 °")).unwrap(),
        json!("90.25 °")
    );
    assert_eq!(
        convert("shaft", json!("12.5 ° from index")).unwrap(),
        json!("12.5 ° from index")
    );
    // between two steps of 1/256 K
    let e = convert("board_temp", json!("25.001 °C")).unwrap_err();
    assert!(
        matches!(e.kind, ErrorKind::Quantity(ref m) if m.contains("not a whole number")),
        "{e}"
    );
    // a plain number is raw or shown: no guess
    let e = convert("board_temp", json!(5)).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
    let e = convert("heading", json!("1.5 rad")).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
}

#[test]
fn any_quantity_carries_its_unit() {
    assert_eq!(convert("sensor", json!("3.3 V")).unwrap(), json!("3.3 V"));
    assert_eq!(convert("sensor", json!("5 mA")).unwrap(), json!("5 mA"));
    // a number alone has no unit
    let e = convert("sensor", json!(5)).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
    let e = convert("sensor", json!("5 parsecs")).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
}

#[test]
fn floats_and_non_finite() {
    assert_eq!(convert("sense", json!("3.3 V")).unwrap(), json!("3300 mV"));
    assert_eq!(convert("sense", json!(2.5)).unwrap(), json!("2.5 mV"));
    assert_eq!(convert("sense", json!("inf")).unwrap(), json!("inf"));
    // NaN only where the type is `MaybeNan`
    assert_eq!(convert("raw_sense", json!("NaN")).unwrap(), json!("NaN"));
    let e = convert("sense", json!("NaN")).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
}

#[test]
fn scales_without_a_symbol_and_codes() {
    // one ADC step is 33/40960 V
    assert_eq!(
        convert("adc", json!("1.65 V")).unwrap(),
        json!("2048·33/40960 V")
    );
    assert_eq!(
        convert("adc", json!("2048·33/40960 V")).unwrap(),
        json!("2048·33/40960 V")
    );
    assert_eq!(convert("adc", json!(100)).unwrap(), json!("100·33/40960 V"));
    // a G.711 code reads as the linear value it means
    assert!(convert("voice", json!(5)).is_ok());
}

#[test]
fn big_stored_numbers_stay_exact() {
    assert_eq!(
        convert("uptime", json!("18446744073709551615")).unwrap(),
        json!("18446744073709551615 µs")
    );
    assert_eq!(
        convert("uptime", json!("1 s")).unwrap(),
        json!("1000000 µs")
    );
    assert_eq!(
        convert("uptime", json!(9007199254740993u64)).unwrap(),
        json!("9007199254740993 µs")
    );
}
