use proc_macro2::TokenStream;
use quote::quote;
use shrink_wrap::SerializeShrinkWrap;
use ww_self::*;

/// Collect information about API items and referenced data types.
/// Serialize into ww_self and create a byte array to be put into device firmware.
pub fn introspect(api_level: &crate::ast::api::ApiLevel) -> TokenStream {
    let api_bundle = ApiBundle {
        root: ApiLevel {
            docs: "",
            ident: "test",
            items: Default::default(),
        },
        types: Default::default(),
        ext_crates: Default::default(),
    };
    let mut scratch = [0u8; 4096]; // TODO: use Vec based BufWriter here
    let bytes = api_bundle.to_ww_bytes(&mut scratch).unwrap();
    let len = bytes.len();
    quote! {
        [u8; #len] = [ #(#bytes),* ]
    }
}
