use proc_macro2::TokenStream;
use quote::quote;

use crate::ast::api::{ApiItemKind, ApiLevel};

pub fn client(api_level: &ApiLevel, api_model_location: &syn::Path, no_alloc: bool) -> TokenStream {
    let root_level = level_methods(api_level, api_model_location, no_alloc);
    quote! {
        impl Client {
            pub fn new() -> Self {
                Client {}
            }

            #root_level
        }
    }
}

fn level_methods(
    api_level: &ApiLevel,
    api_model_location: &syn::Path,
    no_alloc: bool,
) -> TokenStream {
    let handlers = api_level
        .items
        .iter()
        .map(|item| level_method(&item.kind, item.id, api_model_location, no_alloc));
    quote! {
        #(#handlers)*
    }
}

fn level_method(
    kind: &ApiItemKind,
    id: u16,
    api_model_location: &syn::Path,
    _no_alloc: bool,
) -> TokenStream {
    match kind {
        ApiItemKind::Method { ident, args } => {
            let _ = args;
            quote! {
                pub fn #ident(&mut self, ) -> Vec<u8> {
                    use wire_weaver::shrink_wrap::{
                        traits::SerializeShrinkWrap,
                        buf_writer::BufWriter,
                        nib16::Nib16,
                    };
                    use #api_model_location::{Request, RequestKind};
                    let request = Request {
                        // TODO: Handle sequence numbers properly
                        seq: 123,
                        // TODO: Handle sub-levels
                        path: vec![Nib16(#id)],
                        kind: RequestKind::Call { args: vec![] }
                    };
                    // TODO: get Vec from pool
                    let mut buf = [0u8; 128];
                    let mut wr = BufWriter::new(&mut buf);
                    request.ser_shrink_wrap(&mut wr).unwrap();
                    let request_bytes = wr.finish().unwrap().to_vec();
                    request_bytes
                }
            }
        }
        // ApiItemKind::Property => {}
        // ApiItemKind::Stream => {}
        // ApiItemKind::ImplTrait => {}
        // ApiItemKind::Level(_) => {}
        _ => unimplemented!(),
    }
}
