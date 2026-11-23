//! The document tree the value walk reads and writes, free of any text format, and its JSON form.
//!
//! The walk ([crate::value]) turns values into a [Doc] and back; each text format is a thin conversion of the tree
//! that applies only what the format itself can't hold: JSON writes integers beyond ±2⁵³ and floats that are no
//! number as strings, TOML ([crate::toml]) integers beyond `i64` as strings and leaves a `null` out of its table.
//!
//! [Marker] is the TOML-only escape from that: decoding onto a base value reads `{ unset_to_none = true }` and
//! `{ use_parent = true }` before trying a field's ordinary type (Q22/Q23).

use serde_json::{Map, Number, Value as J};
use ww_numeric::NumericValue as N;
use ww_self::ValueOwned;

/// 2⁵³: integers up to it are exact in a JSON number of the usual double.
pub const EXACT_INT: i128 = 1 << 53;

/// A document: what JSON and TOML have in common, with integers and floats apart.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Doc {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    String(String),
    List(Vec<Doc>),
    /// Members in their order.
    Map(Vec<(String, Doc)>),
}

impl Doc {
    /// The member `key` of a map.
    pub(crate) fn get<'d>(map: &'d [(String, Doc)], key: &str) -> Option<&'d Doc> {
        map.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// The reserved key of each [Marker]: no struct field may use either name, so a map of exactly one of them never
/// reads as a struct's value (Q22/Q23).
pub(crate) const UNSET_TO_NONE: &str = "unset_to_none";
pub(crate) const USE_PARENT: &str = "use_parent";

/// What a one-key `{ name = true }` table means for the field it sits on, read only while decoding onto a base
/// value ([crate::toml::Codec::decode_onto]): [Marker::UnsetToNone] sets an `Option` field to `None` over any base
/// value; [Marker::UseParent] keeps the base's value, as if the key were left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Marker {
    UnsetToNone,
    UseParent,
}

impl Marker {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Marker::UnsetToNone => UNSET_TO_NONE,
            Marker::UseParent => USE_PARENT,
        }
    }
}

/// `j` as a [Marker], if it is shaped like one: a map of exactly one key, `unset_to_none` or `use_parent`, set to
/// `true`.
pub(crate) fn marker(j: &Doc) -> Option<Marker> {
    let Doc::Map(members) = j else { return None };
    let [(key, Doc::Bool(true))] = members.as_slice() else {
        return None;
    };
    match key.as_str() {
        UNSET_TO_NONE => Some(Marker::UnsetToNone),
        USE_PARENT => Some(Marker::UseParent),
        _ => None,
    }
}

/// The text format a walk writes for, where the forms differ in more than the leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    Json,
    Toml,
}

impl Form {
    /// The largest integer the format writes as a number: beyond it, a decimal string.
    pub(crate) fn big_int(self) -> &'static str {
        match self {
            Form::Json => "2^53",
            Form::Toml => "i64",
        }
    }

    /// What the format calls a map.
    pub(crate) fn map_word(self) -> &'static str {
        match self {
            Form::Json => "an object",
            Form::Toml => "a table",
        }
    }

    /// The kind of a document value, in the format's words.
    pub(crate) fn kind_of(self, doc: &Doc) -> &'static str {
        match doc {
            Doc::Null => "null",
            Doc::Bool(_) => "a boolean",
            Doc::Int(_) | Doc::Float(_) => "a number",
            Doc::String(_) => "a string",
            Doc::List(_) => "a list",
            Doc::Map(_) => self.map_word(),
        }
    }

    /// The name an enum variant is written with: as defined in JSON, in snake_case in TOML (`FastStart` is
    /// `fast_start`).
    pub(crate) fn variant_name(self, ident: &str) -> String {
        match self {
            Form::Json => ident.to_owned(),
            Form::Toml => snake_case(ident),
        }
    }
}

/// `FastStart` as `fast_start`, `HTTPServer` as `http_server`, `V2` as `v2`.
pub(crate) fn snake_case(ident: &str) -> String {
    let chars: Vec<char> = ident.chars().collect();
    let mut out = String::with_capacity(ident.len() + 4);
    for (idx, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && idx > 0 {
            let prev = chars[idx - 1];
            let next_lower = chars.get(idx + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                out.push('_');
            }
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// The text of a float that reads back to it: shortest digits, an exponent for very large and small ones.
pub(crate) fn float_text(x: f64) -> String {
    format!("{x:?}")
}

/// Hex of `bytes`, lowercase, no separators.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        _ = write!(s, "{b:02x}");
        s
    })
}

pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    let digits: String = text
        .trim()
        .trim_start_matches("0x")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect();
    if !digits.len().is_multiple_of(2) || !digits.is_ascii() {
        return None;
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).ok())
        .collect()
}

/// What a unit is written as: `null` in JSON, an empty table in TOML; either is read.
pub(crate) fn is_unit(j: &Doc) -> bool {
    match j {
        Doc::Null => true,
        Doc::Map(members) => members.is_empty(),
        _ => false,
    }
}

/// A number or a string that is only a number, as the text the stored-number parsers read.
pub(crate) fn stored_text(j: &Doc) -> Option<String> {
    match j {
        Doc::Int(i) => Some(i.to_string()),
        Doc::Float(x) => Some(float_text(*x)),
        Doc::String(s) => {
            let t = s.trim();
            let plain = t.parse::<f64>().is_ok() && !t.is_empty();
            plain.then(|| t.to_string())
        }
        _ => None,
    }
}

/// An integer: a number, a number that is whole (`5.0`), or a decimal string.
pub(crate) fn int_of(j: &Doc) -> Option<i128> {
    match j {
        Doc::Int(i) => Some(*i),
        Doc::Float(x) => (x.fract() == 0.0 && x.abs() <= EXACT_INT as f64).then_some(*x as i128),
        Doc::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A float by its shortest decimal text, so 0.1f32 reads 0.1 and not 0.10000000149011612.
fn float_doc(x: f64, shortest: &str) -> Doc {
    Doc::Float(if x.is_finite() {
        shortest.parse::<f64>().unwrap_or(x)
    } else {
        x
    })
}

/// A number as a document, `None` for a variant the forms don't cover.
pub(crate) fn numeric_doc(n: &N) -> Option<Doc> {
    Some(match n {
        N::F32(x) => float_doc(f64::from(*x), &x.to_string()),
        N::F64(x) => Doc::Float(*x),
        N::U128(x) => match i128::try_from(*x) {
            Ok(i) => Doc::Int(i),
            Err(_) => Doc::String(x.to_string()),
        },
        n => Doc::Int(n.to_i128()?),
    })
}

/// The bytes of a list of `u8` values.
pub(crate) fn u8s(items: &[ValueOwned]) -> Option<Vec<u8>> {
    items
        .iter()
        .map(|v| match v {
            ValueOwned::Numeric(N::U8(b)) => Some(*b),
            _ => None,
        })
        .collect()
}

/// A JSON value as a document.
pub(crate) fn from_json(j: &J) -> Doc {
    match j {
        J::Null => Doc::Null,
        J::Bool(b) => Doc::Bool(*b),
        J::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => Doc::Int(i128::from(i)),
            (_, Some(u)) => Doc::Int(i128::from(u)),
            _ => Doc::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        J::String(s) => Doc::String(s.clone()),
        J::Array(a) => Doc::List(a.iter().map(from_json).collect()),
        J::Object(o) => Doc::Map(o.iter().map(|(k, v)| (k.clone(), from_json(v))).collect()),
    }
}

/// A document as JSON: integers beyond ±2⁵³ and floats that are no number (`"NaN"`, `"inf"`, `"-inf"`) as strings.
pub(crate) fn to_json(doc: Doc) -> J {
    match doc {
        Doc::Null => J::Null,
        Doc::Bool(b) => J::Bool(b),
        Doc::Int(i) if (-EXACT_INT..=EXACT_INT).contains(&i) => {
            J::Number(Number::from(i64::try_from(i).expect("within 2^53")))
        }
        Doc::Int(i) => J::String(i.to_string()),
        Doc::Float(x) => {
            Number::from_f64(x).map_or_else(|| J::String(non_finite_text(x).into()), J::Number)
        }
        Doc::String(s) => J::String(s),
        Doc::List(items) => J::Array(items.into_iter().map(to_json).collect()),
        Doc::Map(members) => {
            let mut object = Map::new();
            for (k, v) in members {
                object.insert(k, to_json(v));
            }
            J::Object(object)
        }
    }
}

/// `NaN`, `inf` or `-inf`.
fn non_finite_text(x: f64) -> &'static str {
    if x.is_nan() {
        "NaN"
    } else if x > 0.0 {
        "inf"
    } else {
        "-inf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn marker_shapes() {
        assert_eq!(
            marker(&Doc::Map(vec![("unset_to_none".into(), Doc::Bool(true))])),
            Some(Marker::UnsetToNone)
        );
        assert_eq!(
            marker(&Doc::Map(vec![("use_parent".into(), Doc::Bool(true))])),
            Some(Marker::UseParent)
        );
        // not a marker: wrong value, an extra key, an unrelated name, an empty table (a unit struct), no table
        assert_eq!(
            marker(&Doc::Map(vec![("unset_to_none".into(), Doc::Bool(false))])),
            None
        );
        assert_eq!(
            marker(&Doc::Map(vec![
                ("use_parent".into(), Doc::Bool(true)),
                ("extra".into(), Doc::Bool(true))
            ])),
            None
        );
        assert_eq!(
            marker(&Doc::Map(vec![("other".into(), Doc::Bool(true))])),
            None
        );
        assert_eq!(marker(&Doc::Map(vec![])), None);
        assert_eq!(marker(&Doc::Null), None);
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(hex(&[0xde, 0xad, 0x01]), "dead01");
        assert_eq!(unhex("de ad_01"), Some(vec![0xde, 0xad, 0x01]));
        assert_eq!(unhex("0xDEAD"), Some(vec![0xde, 0xad]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }

    #[test]
    fn snake_case_names() {
        assert_eq!(snake_case("FastStart"), "fast_start");
        assert_eq!(snake_case("HTTPServer"), "http_server");
        assert_eq!(snake_case("V2"), "v2");
        assert_eq!(snake_case("Off"), "off");
        assert_eq!(snake_case("Mode2Fast"), "mode2_fast");
    }

    #[test]
    fn big_integers_are_strings() {
        assert_eq!(to_json(Doc::Int(5)), json!(5));
        assert_eq!(to_json(Doc::Int(EXACT_INT)), json!(9007199254740992i64));
        assert_eq!(to_json(Doc::Int(EXACT_INT + 1)), json!("9007199254740993"));
        assert_eq!(
            to_json(Doc::Int(-EXACT_INT - 1)),
            json!("-9007199254740993")
        );
    }

    #[test]
    fn floats() {
        assert_eq!(to_json(Doc::Float(0.1)), json!(0.1));
        assert_eq!(to_json(Doc::Float(f64::NAN)), json!("NaN"));
        assert_eq!(to_json(Doc::Float(f64::NEG_INFINITY)), json!("-inf"));
        assert_eq!(float_text(-2.5e300), "-2.5e300");
        assert_eq!(float_text(0.1), "0.1");
        assert_eq!(numeric_doc(&N::F32(0.1)), Some(Doc::Float(0.1)));
        assert!(matches!(numeric_doc(&N::F64(f64::NAN)), Some(Doc::Float(x)) if x.is_nan()));
    }
}
