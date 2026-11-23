//! The whole model of an API root as a byte const (API-73): `ww_codegen!(.., bundle = "no_docs" | "with_docs")`.

use convert_case::{Case, Casing};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use shrink_wrap::SerializeShrinkWrapOwned;
use ww_self::ApiBundleOwned;
use ww_self::signature;

/// Which doc comments the bundle of [gen_bundle] keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BundleDocs {
    /// Every doc comment cleared, as in a device's `introspect = "no_docs"` data.
    NoDocs,
    /// The doc comments of every trait, item, type and field.
    WithDocs,
}

impl BundleDocs {
    /// The value of the `bundle` argument: `"no_docs"` or `"with_docs"`.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "no_docs" => Ok(Self::NoDocs),
            "with_docs" => Ok(Self::WithDocs),
            u => Err(format!(
                "unknown bundle mode '{u}', supported: \"no_docs\" and \"with_docs\""
            )),
        }
    }
}

/// `pub const <ROOT>_BUNDLE: [u8; N]`, the `ApiBundleOwned` of `bundle` in shrink_wrap bytes.
///
/// Unlike a device's `WW_SELF_BYTES`, nothing is left out: the definitions of dependency crates (`ww_si`'s units,
/// a standard library trait) are in it, so a program decodes it with no snapshot at hand. It serves host code that
/// needs the model at run time (`ww_json`'s codec over settings files in an installed binary), never firmware.
///
/// # Panics
///
/// When the bundle can't be serialized, which a bundle loaded from source never is.
pub fn gen_bundle(bundle: &ApiBundleOwned, docs: BundleDocs) -> TokenStream {
    let mut bundle = bundle.clone();
    if docs == BundleDocs::NoDocs {
        signature::drop_docs(&mut bundle);
    }
    let bytes = bundle
        .to_ww_bytes_owned()
        .expect("a bundle loaded from source serializes");
    let len = bytes.len();
    let trait_name = &bundle.root.trait_name;
    let ident = Ident::new(
        &format!("{trait_name}_BUNDLE").to_case(Case::Constant),
        Span::call_site(),
    );
    let doc = format!(
        " The model of `{trait_name}` and every type it reaches, dependencies included, as shrink_wrap bytes of \
         `ww_self::ApiBundleOwned` (API-73)."
    );
    quote! {
        #[doc = #doc]
        pub const #ident: [u8; #len] = [ #(#bytes),* ];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shrink_wrap::DeserializeShrinkWrapOwned;
    use std::path::Path;

    fn quantities_api() -> ApiBundleOwned {
        let crate_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/quantities_api");
        crate::load(&crate_path, None, false).unwrap()
    }

    fn bytes_of(ts: &TokenStream) -> Vec<u8> {
        let item: syn::ItemConst = syn::parse2(ts.clone()).unwrap();
        let syn::Expr::Array(array) = *item.expr else {
            panic!("an array");
        };
        array
            .elems
            .iter()
            .map(|e| match e {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(i),
                    ..
                }) => i.base10_parse().unwrap(),
                _ => panic!("a byte"),
            })
            .collect()
    }

    /// The bytes are the loaded bundle's, with its docs or without them, and decode back.
    #[test]
    fn bundle_bytes_are_the_loaded_model() {
        let full = quantities_api();
        let with_docs = bytes_of(&gen_bundle(&full, BundleDocs::WithDocs));
        assert_eq!(with_docs, full.to_ww_bytes_owned().unwrap());

        let no_docs = bytes_of(&gen_bundle(&full, BundleDocs::NoDocs));
        let mut expected = full.clone();
        signature::drop_docs(&mut expected);
        assert_eq!(no_docs, expected.to_ww_bytes_owned().unwrap());
        assert!(no_docs.len() < with_docs.len());
        ApiBundleOwned::from_ww_bytes_owned(&no_docs).unwrap();
    }

    /// Dependency definitions stay in: `ww_si`'s types are what `ww_json` reads quantities by.
    #[test]
    fn dependency_types_are_not_left_out() {
        let full = quantities_api();
        let bytes = bytes_of(&gen_bundle(&full, BundleDocs::NoDocs));
        let decoded = ApiBundleOwned::from_ww_bytes_owned(&bytes).unwrap();
        assert!(decoded.ext_crates.iter().any(|c| c.crate_id == "ww_si"));
        assert_eq!(decoded.types.len(), full.types.len());
        assert!(
            decoded
                .types
                .iter()
                .all(|t| matches!(t, ww_self::TypeLocationOwned::InLine { .. }))
        );
    }

    #[test]
    fn unknown_mode_is_refused() {
        assert_eq!(BundleDocs::parse("with_docs"), Ok(BundleDocs::WithDocs));
        assert!(
            BundleDocs::parse("docs")
                .unwrap_err()
                .contains("\"no_docs\"")
        );
    }
}
