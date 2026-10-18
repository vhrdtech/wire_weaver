//! The `ww_self` dynamic client as a consumer: the type from the API model of `conformance_api` (loaded from
//! source, as `wire_weaver_core::load` does for a device's crate), the value from the corpus through [crate::json].

use crate::corpus::Case;
use std::path::Path;
use std::sync::OnceLock;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, TypeOwned, ValueOwned};

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

pub struct Dynamic;

impl Dynamic {
    fn value(case: &Case, ty: &TypeOwned) -> Result<ValueOwned, String> {
        crate::json::to_value(&case.value, ty, bundle()).map_err(|e| format!("corpus value: {e}"))
    }
}

impl crate::Consumer for Dynamic {
    const NAME: &'static str = "dynamic";

    fn encode(&self, case: &Case) -> Option<Result<Vec<u8>, String>> {
        let ty = property_ty(&case.ty)?;
        Some(Self::value(case, ty).and_then(|v| {
            v.ser_shrink_wrap_dyn(ty, bundle())
                .map_err(|e| format!("{e:#}"))
        }))
    }

    fn decode(&self, case: &Case) -> Option<crate::DecodeResult> {
        let ty = property_ty(&case.ty)?;
        let got = match ValueOwned::des_shrink_wrap_dyn(&case.golden(), ty, bundle()) {
            Ok(v) => v,
            Err(e) => return Some(Err(format!("{e:#}"))),
        };
        if case.value.is_null() && matches!(case.direction, crate::corpus::Direction::Reject) {
            return Some(Ok(Err(format!("decoded {got:?}"))));
        }
        Some(Ok(match Self::value(case, ty) {
            Ok(want) if want == got => Ok(()),
            Ok(want) => Err(format!("decoded {got:?}, expected {want:?}")),
            Err(e) => Err(e),
        }))
    }
}
