//! The API bundles of the tests: loaded from source as `wire_weaver_core::load` does for a device's crate.
#![allow(dead_code, reason = "each test crate uses a part of these helpers")]

use std::path::Path;
use ww_self::{ApiBundleOwned, ApiItemKindOwned, ArgumentOwned, TypeOwned};

pub fn load(dir: &str, root: &str) -> ApiBundleOwned {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests")
        .join(dir);
    let mut bundle = wire_weaver_core::load(&path, Some(root.into()), false)
        .unwrap_or_else(|e| panic!("{dir}: {e:#}"));
    ww_self::generics::prepare(&mut bundle).expect("prepared bundle");
    bundle
}

pub fn quantities() -> &'static ApiBundleOwned {
    static B: std::sync::OnceLock<ApiBundleOwned> = std::sync::OnceLock::new();
    B.get_or_init(|| load("quantities_api", "Quantities"))
}

pub fn property<'a>(bundle: &'a ApiBundleOwned, name: &str) -> &'a TypeOwned {
    bundle
        .root
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ApiItemKindOwned::Property { ty, .. } if item.ident == name => Some(ty),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no property {name}"))
}

pub fn method_args<'a>(bundle: &'a ApiBundleOwned, name: &str) -> &'a [ArgumentOwned] {
    bundle
        .root
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ApiItemKindOwned::Method { args, .. } if item.ident == name => Some(args.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no method {name}"))
}
