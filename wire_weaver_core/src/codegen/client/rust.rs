use crate::ast::api::{ApiLevel, Argument, PropertyAccess};
use crate::ast::visit::{Context, Visit};
use crate::codegen::api_common::args_structs;
use crate::codegen::index_chain::IndexChain;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use shrink_wrap_core::ast::Type;
use shrink_wrap_core::ast::path::Path;
use syn::LitStr;

struct CGRust {}

impl Visit for CGRust {
    fn visit_level(&mut self, cx: &Context, level: &ApiLevel) {
        let mut ts = TokenStream::new();
        let args_structs = args_structs(level, cx.model.no_alloc());

        let crate_name = level.source_location.crate_name();
        let mod_name = level.mod_ident(Some(crate_name));
        let use_external = level.use_external_types(
            Path::new_ident(crate_name.clone()),
            // .map(|n| Path::new_ident(n.clone()))
            // .unwrap_or(Path::new_path("super::super")),
            cx.model.no_alloc(),
        );
        let client_struct_name = level.client_struct_name(Some(crate_name));
        let gid_paths = level.gid_paths();

        Visit::visit_items(self, cx, &level.items);

        // call before increment_length so that root level does not have it
        let maybe_index_chain_field = IndexChain::from_len(cx.index_chain_len).struct_field_def();
        let mut child_ts = TokenStream::new();

        Visit::visit_child_levels(self, cx, &level.items);

        let trait_name = LitStr::new(&level.name.to_string(), level.name.span());
        let is_at_root = cx.index_chain_len == 0;
        let index_chain = if is_at_root {
            quote! { vec![] }
        } else {
            quote! { self.index_chain.to_vec() }
        };
        let full = &gid_paths.0;
        let attachment = quote! {
            pub fn attachment(&self) -> wire_weaver_client_common::Attachment {
                let mut cmd_tx = self.cmd_tx.clone();
                cmd_tx.set_base_path(#index_chain);
                wire_weaver_client_common::Attachment::new(
                    cmd_tx,
                    #full,
                    #trait_name
                )
            }
        };

        let impl_new_or_user_struct = if let Some(client_struct) = is_at_root {
            quote! {
                impl super::super::#client_struct {
                    #methods
                    #attachment
                }
            }
        } else {
            quote! {
                pub struct #client_struct_name<'i> {
                    #maybe_index_chain_field
                    pub args_scratch: &'i mut [u8],
                    pub cmd_tx: &'i mut wire_weaver_client_common::CommandSender,
                }

                impl<'i> #client_struct_name<'i> {
                    #methods
                    #attachment
                }
            }
        };
        ts.extend(quote! {
            mod #mod_name {
                use super::*;
                #use_external
                #args_structs

                #impl_new_or_user_struct

                #child_ts
            }
        });
        ts
    }

    fn visit_method(
        &mut self,
        cx: &Context,
        ident: &Ident,
        args: &[Argument],
        return_ty: &Option<Type>,
    ) {
        todo!()
    }

    fn visit_property(&mut self, cx: &Context, ident: &Ident, ty: &Type, access: PropertyAccess) {
        todo!()
    }

    fn visit_stream(&mut self, cx: &Context, ident: &Ident, ty: &Type, is_up: bool) {
        todo!()
    }

    fn visit_trait(&mut self, cx: &Context, level: &ApiLevel) {
        todo!()
    }
}
