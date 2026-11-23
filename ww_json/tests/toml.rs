//! The TOML form of every kind of type (INT-18), decode onto a base value, write-back into a parsed document, and
//! the errors with their paths.

mod common;

use common::{load, property};
use toml_edit::{DocumentMut, Item};
use ww_json::toml::{decode_onto, from_toml, to_toml, write_onto};
use ww_json::{Codec, ErrorKind, JsonError};
use ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};

fn conformance() -> &'static ApiBundleOwned {
    static B: std::sync::OnceLock<ApiBundleOwned> = std::sync::OnceLock::new();
    B.get_or_init(|| load("conformance_api", "Conformance"))
}

fn settings_bundle() -> &'static ApiBundleOwned {
    static B: std::sync::OnceLock<ApiBundleOwned> = std::sync::OnceLock::new();
    B.get_or_init(|| load("toml_settings_api", "SettingsTest"))
}

fn settings_ty() -> &'static TypeOwned {
    property(settings_bundle(), "settings")
}

/// The item of `v` in the document `v = <text>`.
fn item(text: &str) -> Item {
    let doc: DocumentMut = format!("v = {text}").parse().expect("TOML");
    doc["v"].clone()
}

/// `item` printed as the value of `v` in a document.
fn text(item: Item) -> String {
    let mut doc = DocumentMut::new();
    doc["v"] = item;
    doc.to_string()
}

/// The TOML written for `input` read as property `name` of the conformance API, after reading it back to the same
/// value.
fn round_trip(name: &str, input: &str) -> String {
    let b = conformance();
    let ty = property(b, name);
    let value = from_toml(&item(input), ty, b).unwrap_or_else(|e| panic!("{name}: {e}"));
    let out = to_toml(&value, ty, b).unwrap_or_else(|e| panic!("{name}: {e}"));
    let back = from_toml(&out, ty, b).unwrap_or_else(|e| panic!("{name}: {e}"));
    // NaN equals nothing, also itself: compared by its Debug text
    assert_eq!(format!("{back:?}"), format!("{value:?}"), "{name}");
    text(out)
}

fn error(name: &str, input: &str) -> JsonError {
    let b = conformance();
    from_toml(&item(input), property(b, name), b).unwrap_err()
}

#[test]
fn scalars() {
    assert_eq!(round_trip("u8", "255"), "v = 255\n");
    assert_eq!(round_trip("bool", "true"), "v = true\n");
    assert_eq!(round_trip("string", "\"héllo\""), "v = \"héllo\"\n");
    assert_eq!(round_trip("f32", "0.1"), "v = 0.1\n");
    assert_eq!(round_trip("f64", "-2.5e3"), "v = -2500.0\n");
    assert!(round_trip("f64", "-2.5e300").starts_with("v = -25000"));
    assert_eq!(round_trip("f64", "nan"), "v = nan\n");
    assert_eq!(round_trip("f32", "-inf"), "v = -inf\n");
    // TOML holds i64: integers stay integers up to it, beyond it they are strings
    assert_eq!(
        round_trip("i64", &i64::MIN.to_string()),
        format!("v = {}\n", i64::MIN)
    );
    assert_eq!(
        round_trip("u64", "9007199254740993"),
        "v = 9007199254740993\n"
    );
    assert_eq!(
        round_trip("u64", "\"18446744073709551615\""),
        "v = \"18446744073709551615\"\n"
    );
    assert_eq!(round_trip("u8", "7.0"), "v = 7\n");
    assert_eq!(round_trip("i5", "-16"), "v = -16\n");
}

#[test]
fn lists_tuples_options_results() {
    assert_eq!(round_trip("bytes", "\"00ff10\""), "v = \"00ff10\"\n");
    assert_eq!(round_trip("bytes", "[0, 255, 16]"), "v = \"00ff10\"\n");
    assert_eq!(
        round_trip("vec_string", "[\"a\", \"b\"]"),
        "v = [\"a\", \"b\"]\n"
    );
    assert_eq!(round_trip("array_u16", "[1, 2, 3]"), "v = [1, 2, 3]\n");
    assert_eq!(round_trip("tuple", "[1, 2, true]"), "v = [1, 2, true]\n");
    assert_eq!(round_trip("range", "[1, 9]"), "v = [1, 9]\n");
    assert_eq!(round_trip("option_u8", "5"), "v = 5\n");
    assert_eq!(round_trip("result", "{ Ok = 5 }"), "[v]\nOk = 5\n");
    assert_eq!(
        round_trip("result", "{ Err = \"missing\" }"),
        "[v]\nErr = \"missing\"\n"
    );
    // a `None` inside a list has no TOML form
    let b = conformance();
    let ty = property(b, "pair");
    let value = ww_json::from_json(&serde_json::json!([1, null]), ty, b).unwrap();
    let e = to_toml(&value, ty, b).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Unsupported(_)), "{e}");
    assert_eq!(e.path.to_string(), "value[1]");
}

#[test]
fn enums_are_snake_case() {
    assert_eq!(round_trip("shape", "\"empty\""), "v = \"empty\"\n");
    // the name as defined is read too
    assert_eq!(round_trip("shape", "\"Empty\""), "v = \"empty\"\n");
    assert_eq!(round_trip("shape", "{ circle = 5 }"), "[v]\ncircle = 5\n");
    assert_eq!(
        round_trip("shape", "{ rect = { w = 1, h = 2, label = \"x\" } }"),
        "[v.rect]\nw = 1\nh = 2\nlabel = \"x\"\n"
    );
    let e = error("shape", "\"square\"");
    let ErrorKind::UnknownVariant { expected, .. } = e.kind else {
        panic!("{e}");
    };
    assert!(expected.contains(&"rect".to_owned()), "{expected:?}");
    assert_eq!(round_trip("marker", "{}"), "[v]\n");
}

#[test]
fn errors_name_their_path() {
    let e = error("u8", "\"x\"");
    assert_eq!(
        e.to_string(),
        "value: expected an integer of u8 (a string beyond i64), found a string"
    );
    let e = error("shape", "{ rect = { w = 1, h = 2, label = \"x\", d = 4 } }");
    assert_eq!(e.path.to_string(), "value.rect");
    assert!(matches!(e.kind, ErrorKind::UnknownField { ref name, .. } if name == "d"));
    let e = error("u8", "1979-05-27");
    assert!(matches!(e.kind, ErrorKind::Unsupported(_)), "{e}");
}

// ------------------------------------------------------------------------------------------------ settings levels

const BUILT_IN: &str = r#"
seed = 7

[audio]
window = "300 ms"
rate = "16 kHz"
duty = "50 %"
gain = 1.5
mode = "fast_start"

[audio.limits]
items = "10 item"
strict = false

[swear]
tokens = ["ha", "he"]
enabled = true
"#;

fn built_in() -> ValueOwned {
    let doc: DocumentMut = BUILT_IN.parse().unwrap();
    from_toml(doc.as_item(), settings_ty(), settings_bundle()).unwrap()
}

/// `value` as a TOML document.
fn document(value: &ValueOwned) -> String {
    let item = to_toml(value, settings_ty(), settings_bundle()).unwrap();
    let Item::Table(table) = item else {
        panic!("a struct is a table");
    };
    DocumentMut::from(table).to_string()
}

#[test]
fn settings_round_trip() {
    let value = built_in();
    let out = document(&value);
    assert_eq!(out.trim(), BUILT_IN.trim());
    let doc: DocumentMut = out.parse().unwrap();
    let back = from_toml(doc.as_item(), settings_ty(), settings_bundle()).unwrap();
    assert_eq!(back, value);
}

#[test]
fn quantities_read_in_any_exact_unit() {
    let base = built_in();
    let level: DocumentMut = "[audio]\nwindow = \"0.25 s\"\nrate = \"16000 Hz\""
        .parse()
        .unwrap();
    let value = decode_onto(&base, &level, settings_ty(), settings_bundle()).unwrap();
    let out = document(&value);
    assert!(out.contains("window = \"250 ms\""), "{out}");
    assert!(out.contains("rate = \"16 kHz\""), "{out}");
}

#[test]
fn decode_onto_keeps_untouched_keys() {
    let base = built_in();
    let level: DocumentMut = r#"
seed = "18446744073709551615"
[audio]
mode = { custom = { level = 3 } }
[audio.limits]
strict = true
[swear]
action = "mute"
"#
    .parse()
    .unwrap();
    let value = decode_onto(&base, &level, settings_ty(), settings_bundle()).unwrap();
    let out = document(&value);
    let expected = BUILT_IN
        .replace("seed = 7", "seed = \"18446744073709551615\"")
        .replace(
            "mode = \"fast_start\"\n",
            "\n[audio.mode.custom]\nlevel = 3\n",
        )
        .replace("strict = false", "strict = true")
        .replace("enabled = true\n", "enabled = true\naction = \"mute\"\n");
    let reparsed: DocumentMut = expected.parse().unwrap();
    let want = from_toml(reparsed.as_item(), settings_ty(), settings_bundle()).unwrap();
    assert_eq!(value, want, "{out}");
    // an empty level is the base itself
    let empty = DocumentMut::new();
    assert_eq!(
        decode_onto(&base, &empty, settings_ty(), settings_bundle()).unwrap(),
        base
    );
}

#[test]
fn decode_onto_errors_name_the_key() {
    let base = built_in();
    let codec = Codec::new(settings_bundle()).root("streamer");
    let fail = |text: &str| {
        let level: DocumentMut = text.parse().unwrap();
        codec.decode_onto(&base, &level, settings_ty()).unwrap_err()
    };
    let e = fail("[audio]\nspan = \"1 s\"");
    assert_eq!(e.path.to_string(), "streamer.audio");
    let ErrorKind::UnknownField { name, expected } = &e.kind else {
        panic!("{e}");
    };
    assert_eq!(name, "span");
    assert!(expected.contains(&"window".to_owned()), "{expected:?}");
    let e = fail("[swear]\nenabled = 1");
    assert_eq!(e.path.to_string(), "streamer.swear.enabled");
    assert!(matches!(e.kind, ErrorKind::WrongType { .. }), "{e}");
    let e = fail("[audio]\nwindow = \"3 V\"");
    assert_eq!(e.path.to_string(), "streamer.audio.window");
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
    let e = fail("[audio]\nduty = \"120 %\"");
    assert_eq!(e.path.to_string(), "streamer.audio.duty");
    assert!(matches!(e.kind, ErrorKind::Quantity(_)), "{e}");
    let e = fail("[audio.limits]\nitems = 1.5");
    assert_eq!(e.path.to_string(), "streamer.audio.limits.items");
}

#[test]
fn write_onto_edits_only_what_changed() {
    let base = built_in();
    let level_text = r#"# the streamer's level
[audio]
gain = 2.0 # louder
window = "0.4 s"   # as typed

[swear]
tokens = ["ha"]
"#;
    let mut level: DocumentMut = level_text.parse().unwrap();
    let current = decode_onto(&base, &level, settings_ty(), settings_bundle()).unwrap();

    // nothing changed: nothing is rewritten, also a value in another unit than the encoder writes
    write_onto(
        &mut level,
        &base,
        &current,
        settings_ty(),
        settings_bundle(),
    )
    .unwrap();
    assert_eq!(level.to_string(), level_text);

    // gain changes, the strictness of the limits and the action are new, the rest stays as typed
    let mut changed = from_toml(
        &to_toml(&current, settings_ty(), settings_bundle()).unwrap(),
        settings_ty(),
        settings_bundle(),
    )
    .unwrap();
    let edit: DocumentMut =
        "[audio]\ngain = 3.0\n[audio.limits]\nstrict = true\n[swear]\naction = \"beep\""
            .parse()
            .unwrap();
    changed = decode_onto(&changed, &edit, settings_ty(), settings_bundle()).unwrap();
    write_onto(
        &mut level,
        &base,
        &changed,
        settings_ty(),
        settings_bundle(),
    )
    .unwrap();
    assert_eq!(
        level.to_string(),
        r#"# the streamer's level
[audio]
gain = 3.0 # louder
window = "0.4 s"   # as typed

[audio.limits]
strict = true

[swear]
tokens = ["ha"]
action = "beep"
"#
    );
    let again = decode_onto(&base, &level, settings_ty(), settings_bundle()).unwrap();
    assert_eq!(again, changed);

    // `None` over a base that has a value has no form
    let mut no_limits = changed.clone();
    let ValueOwned::Struct { fields } = &mut no_limits else {
        panic!("a struct");
    };
    let ww_self::FieldsValueOwned::Named(fields) = fields else {
        panic!("named fields");
    };
    let audio = &mut fields.iter_mut().find(|(n, _)| n == "audio").unwrap().1;
    let ValueOwned::Struct {
        fields: ww_self::FieldsValueOwned::Named(audio),
    } = audio
    else {
        panic!("a struct");
    };
    audio.iter_mut().find(|(n, _)| n == "limits").unwrap().1 = ValueOwned::Option(None);
    let e = write_onto(
        &mut level,
        &base,
        &no_limits,
        settings_ty(),
        settings_bundle(),
    )
    .unwrap_err();
    assert_eq!(e.path.to_string(), "value.audio.limits");
    assert!(matches!(e.kind, ErrorKind::Unsupported(_)), "{e}");
}

/// A program reads its levels with the model compiled into the API crate (API-73), no source at hand: the bytes are
/// the bundle the crate walker loads, and decode the same levels to the same values.
#[test]
fn compiled_bundle_reads_levels_as_the_loaded_one() {
    use shrink_wrap::{DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/toml_settings_api");
    let loaded = wire_weaver_core::load(&path, Some("SettingsTest".into()), false).unwrap();
    assert_eq!(
        &toml_settings_api::SETTINGS_TEST_BUNDLE[..],
        &loaded.to_ww_bytes_owned().unwrap()[..]
    );

    let mut compiled =
        ApiBundleOwned::from_ww_bytes_owned(&toml_settings_api::SETTINGS_TEST_BUNDLE).unwrap();
    ww_self::generics::prepare(&mut compiled).unwrap();
    let ty = property(&compiled, "settings");
    let doc: DocumentMut = BUILT_IN.parse().unwrap();
    let base = from_toml(doc.as_item(), ty, &compiled).unwrap();
    assert_eq!(base, built_in());
    let level: DocumentMut = "[audio]\nwindow = \"0.25 s\"\n[swear]\naction = \"mute\""
        .parse()
        .unwrap();
    assert_eq!(
        decode_onto(&base, &level, ty, &compiled).unwrap(),
        decode_onto(&base, &level, settings_ty(), settings_bundle()).unwrap()
    );
}
