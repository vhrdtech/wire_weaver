//! A corpus value (JSON in the form `wire_weaver_py` uses for Python values) -> [ValueOwned], guided by the type
//! from the API model: what a dynamic consumer is given to encode, and what it must decode. The codec is
//! `ww_json`'s (INT-14), in its structural form: quantities and counts are the structs they are on the wire.

use serde_json::Value as J;
use ww_json::Codec;
use ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};

pub fn to_value(j: &J, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned, String> {
    Codec::new(bundle)
        .raw()
        .root("corpus")
        .from_json(j, ty)
        .map_err(|e| e.to_string())
}

/// An integer of the corpus: a JSON number, or a string for one beyond 64 bits (JSON parsers lose those).
pub fn int(j: &J) -> Option<i128> {
    match j {
        J::Number(n) => n.as_i64().map(i128::from).or(n.as_u64().map(i128::from)),
        J::String(s) => s.parse().ok(),
        _ => None,
    }
}

pub fn type_name(ty: &TypeOwned, bundle: &ApiBundleOwned) -> String {
    ty.human_name(false, bundle)
        .unwrap_or_else(|_| format!("{ty:?}"))
}
