use crate::ast::docs::Docs;
use crate::ast::item_struct::Field;
use crate::ast::object_size::ObjectSize;
use crate::ast::ty::Type;
use crate::codegen::ty::FieldPath;
use crate::codegen::util::maybe_quote;
use crate::codegen::util::serdes_scaffold;
use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use syn::Path;

#[derive(Clone, Debug)]
pub(crate) struct CGItemStruct<'i> {
    pub(crate) docs: &'i Docs,
    pub(crate) ident: &'i Ident,
    pub(crate) fields: &'i [Field],
    pub(crate) cfg: Option<&'i TokenStream>,
    pub(crate) cfg_attr: &'i [TokenStream],
    pub(crate) derive: &'i [Path],
    pub(crate) size_assumption: Option<ObjectSize>,
    /// Set for a plain, lifetime-less type generated only because a `borrowed`/`owned` directive
    /// was used to disambiguate it (see `TyKind::Ambiguous`). Such a type never gets a `<'i>`
    /// generic, even on its "borrowed" (`is_ref: true`) side.
    pub(crate) ambiguous: bool,
}

impl<'i> CGItemStruct<'i> {
    pub(crate) fn def_rust(&self, is_ref: bool) -> TokenStream {
        let cfg = self.cfg.map(|cond| quote! { #[cfg(#cond)] });
        let docs = &self.docs;
        let derive = maybe_quote(!self.derive.is_empty(), || {
            let d = self.derive.iter();
            quote! { #[derive( #(#d),* )] }
        });
        let cfg_attr = self.cfg_attr.iter();
        let ident = &self.ident;
        let fields = CGStructFieldsDef {
            fields: &self.fields,
            is_ref,
        };
        let lifetime = maybe_quote(is_ref && !self.ambiguous, || quote! { <'i> });
        let assert_size = if let Some(size) = &self.size_assumption {
            size.assert_element_size(&self.ident, self.cfg, is_ref)
        } else {
            quote! {}
        };
        let ts = quote! {
            #cfg
            #docs
            #derive
            #(#[cfg_attr(#cfg_attr)])*
            pub struct #ident #lifetime { #fields }
            #assert_size
        };
        ts
    }

    pub(crate) fn serdes_rust(&self, is_ref: bool) -> TokenStream {
        let struct_name = &self.ident;
        let struct_ser = CGStructSer {
            item_struct: self,
            is_ref,
        };
        let struct_des = CGStructDes {
            item_struct: self,
            is_ref,
        };

        let mut unknown_unsized = vec![];
        let mut sum = self.size_assumption.unwrap_or(ObjectSize::Unsized); // struct is Unsized by default.
        // No need to check if it's already Unsized.
        if !matches!(sum, ObjectSize::Unsized) {
            // NOTE: make sure to not accidentally bump Unsized to UFS here if any of the fields is UFS.
            // See ElementSize docs and comments on sum method.
            for f in self.fields {
                if let Some(size) = f.ty.element_size() {
                    sum = sum.add(size);
                }
                if let Type::External(path, _) = &f.ty
                    && let Some(ident) = path.segments.last()
                {
                    unknown_unsized.push(ident.clone());
                }
            }
        }
        let implicitly_unsized = sum.is_unsized() && unknown_unsized.is_empty();
        let element_size = if implicitly_unsized {
            let r#unsized = ObjectSize::Unsized;
            quote! { #r#unsized }
        } else {
            sum.sum_recursively(unknown_unsized, is_ref)
        };
        serdes_scaffold(
            struct_name,
            struct_ser,
            struct_des,
            self.cfg,
            element_size,
            is_ref,
            is_ref && !self.ambiguous,
        )
    }
}

struct CGStructFieldsDef<'a> {
    fields: &'a [Field],
    is_ref: bool,
}

impl ToTokens for CGStructFieldsDef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                continue;
            }
            let ident = &struct_field.ident;
            let ty = struct_field.ty.def(self.is_ref);
            let docs = &struct_field.docs;
            tokens.append_all(quote! {
                #docs
                pub #ident: #ty,
            });
        }
    }
}

struct CGStructSer<'a> {
    item_struct: &'a CGItemStruct<'a>,
    is_ref: bool,
}

struct CGStructDes<'a> {
    item_struct: &'a CGItemStruct<'a>,
    is_ref: bool,
}

impl ToTokens for CGStructSer<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.item_struct.fields {
            let field_name = &struct_field.ident;
            let field_path = if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                FieldPath::Value(quote! {self})
            } else {
                // TODO: if field is already a reference, this is not quite correct, but this information is not used anymore
                FieldPath::Value(quote! {self.#field_name})
            };
            struct_field
                .ty
                .buf_write(field_path, self.is_ref, quote! { ? }, tokens);
        }
        tokens.append_all(quote! {
            Ok(())
        });
    }
}

impl ToTokens for CGStructDes<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut field_names = vec![];
        for struct_field in self.item_struct.fields {
            let field_name = &struct_field.ident;
            if !matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                field_names.push(field_name.clone());
            }
            let handle_eob = struct_field.handle_eob();
            struct_field
                .ty
                .buf_read(field_name, !self.is_ref, handle_eob, &quote! { _ }, tokens);
        }
        let struct_name = &self.item_struct.ident;
        tokens.append_all(quote! {
            Ok(#struct_name {
                #(#field_names),*
            })
        });
    }
}

impl Field {
    pub(crate) fn handle_eob(&self) -> TokenStream {
        match &self.default {
            None => quote!(?),
            Some(value) => {
                quote!(.unwrap_or(#value))
            }
        }
    }
}
