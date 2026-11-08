//! The `ww_self` dynamic client as a consumer: the type from the API model of `conformance_api` (loaded from
//! source, as `wire_weaver_core::load` does for a device's crate), the value from the corpus through [crate::json].

use crate::corpus::Case;
use std::path::Path;
use std::sync::OnceLock;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ArgumentOwned, TypeOwned, ValueOwned};

pub fn bundle() -> &'static ApiBundleOwned {
    static BUNDLE: OnceLock<ApiBundleOwned> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance_api");
        let mut bundle = wire_weaver_core::load(&path, Some("Conformance".into()), false)
            .unwrap_or_else(|e| panic!("conformance_api: {e:#}"));
        // generic types instantiated, as a client does with introspection data
        ww_self::generics::prepare(&mut bundle).expect("conformance_api's bundle");
        bundle
    })
}

/// The type of property `name` of `Conformance`, `None` for a type that isn't in the API model.
pub fn property_ty(name: &str) -> Option<&'static TypeOwned> {
    bundle()
        .root
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ApiItemKindOwned::Property { ty, .. } if item.ident == name => Some(ty),
            _ => None,
        })
}

/// The arguments of method `name` of `Conformance` (EVO-22), `None` for a name that isn't a method.
pub fn method_args(name: &str) -> Option<&'static [ArgumentOwned]> {
    bundle()
        .root
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ApiItemKindOwned::Method { args, .. } if item.ident == name => Some(args.as_slice()),
            _ => None,
        })
}

/// The corpus value of an arguments case, an object of arguments (a `#[flatten]` struct's fields in its place), as
/// their values in order.
pub fn args_values(
    j: &serde_json::Value,
    defs: &[ArgumentOwned],
) -> Result<Vec<ValueOwned>, String> {
    let fields = ArgumentOwned::as_fields(defs, bundle()).map_err(|e| format!("{e:#}"))?;
    let object = j
        .as_object()
        .ok_or_else(|| format!("{j} is not an object of arguments"))?;
    if object.len() != fields.len() {
        return Err(format!("{j}: every argument is given"));
    }
    fields
        .iter()
        .map(|f| {
            let name = f.ident.as_deref().unwrap_or("_");
            let j = object
                .get(name)
                .ok_or_else(|| format!("{j}: no argument {name}"))?;
            crate::json::to_value(j, &f.ty, bundle()).map_err(|e| format!("corpus value: {e}"))
        })
        .collect()
}

pub struct Dynamic;

impl Dynamic {
    fn value(case: &Case, ty: &TypeOwned) -> Result<ValueOwned, String> {
        crate::json::to_value(&case.value, ty, bundle()).map_err(|e| format!("corpus value: {e}"))
    }
}

impl crate::Consumer for Dynamic {
    const NAME: &'static str = "dynamic";

    fn encode(&self, case: &Case) -> Option<Result<Vec<u8>, String>> {
        let Some(ty) = property_ty(&case.ty) else {
            let defs = method_args(&case.ty)?;
            return Some(args_values(&case.value, defs).and_then(|values| {
                ValueOwned::ser_args_dyn(&values, defs, bundle()).map_err(|e| format!("{e:#}"))
            }));
        };
        Some(Self::value(case, ty).and_then(|v| {
            v.ser_shrink_wrap_dyn(ty, bundle())
                .map_err(|e| format!("{e:#}"))
        }))
    }

    fn decode(&self, case: &Case) -> Option<crate::DecodeResult> {
        let Some(ty) = property_ty(&case.ty) else {
            let defs = method_args(&case.ty)?;
            let got = match ValueOwned::des_args_dyn(&case.golden(), defs, bundle()) {
                Ok(v) => v,
                Err(e) => return Some(Err(format!("{e:#}"))),
            };
            return Some(Ok(match args_values(&case.value, defs) {
                Ok(want) if want == got => Ok(()),
                Ok(want) => Err(format!("decoded {got:?}, expected {want:?}")),
                Err(e) => Err(e),
            }));
        };
        let got = match ValueOwned::des_shrink_wrap_dyn(&case.golden(), ty, bundle()) {
            Ok(v) => v,
            Err(e) => return Some(Err(format!("{e:#}"))),
        };
        if case.value.is_null() && matches!(case.direction, crate::corpus::Direction::Reject) {
            return Some(Ok(Err(format!("decoded {got:?}"))));
        }
        Some(Ok(match Self::value(case, ty) {
            // NaN equals nothing, also itself: a value holding one compares by its Debug text (NUM-19)
            Ok(want) if want == got || format!("{want:?}") == format!("{got:?}") => Ok(()),
            Ok(want) => Err(format!("decoded {got:?}, expected {want:?}")),
            Err(e) => Err(e),
        }))
    }
}
