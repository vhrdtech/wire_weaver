use crate::ast::object_size::ObjectSize;
use crate::ast::{Field, ItemStruct, Type};
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{serdes_scaffold, strings_to_derive};
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};

impl ItemStruct {
    pub fn def_rust(&self, no_alloc: bool) -> TokenStream {
        let ident = &self.ident;
        let fields = CGStructFieldsDef {
            fields: &self.fields,
            no_alloc,
        };
        let lifetime = if no_alloc && self.potential_lifetimes() {
            quote!(<'i>)
        } else {
            quote!()
        };
        let derive = strings_to_derive(&self.derive);
        let docs = &self.docs;
        let cfg = &self.cfg;
        let cfg_attr_defmt = &self.defmt;
        let assert_size = if let Some(size) = &self.size_assumption {
            size.assert_element_size(&self.ident, &self.cfg)
        } else {
            quote! {}
        };
        let ts = quote! {
            #cfg
            #docs
            #derive
            #cfg_attr_defmt
            pub struct #ident #lifetime { #fields }
            #assert_size
        };
        ts
    }

    pub fn serdes_rust(&self, no_alloc: bool, skip_owned: bool) -> TokenStream {
        let struct_name = &self.ident;
        let struct_ser = CGStructSer {
            item_struct: self,
            no_alloc,
        };
        let struct_des = CGStructDes {
            item_struct: self,
            no_alloc,
            owned: false,
        };
        let (lifetime, struct_des_owned) = if no_alloc && self.potential_lifetimes() {
            (quote!(<'i>), None)
        } else if skip_owned {
            (quote!(), None)
        } else {
            let struct_des_owned = CGStructDes {
                item_struct: self,
                no_alloc,
                owned: true,
            };
            (quote!(), Some(struct_des_owned))
        };

        let mut unknown_unsized = vec![];
        let mut sum = self.size_assumption.unwrap_or(ObjectSize::Unsized); // struct is Unsized by default.
        // No need to check if it's already Unsized.
        if !matches!(sum, ObjectSize::Unsized) {
            // NOTE: make sure to not accidentally bump Unsized to UFS here if any of the fields is UFS.
            // See ElementSize docs and comments on sum method.
            for f in &self.fields {
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
            sum.sum_recursively(unknown_unsized)
        };
        serdes_scaffold(
            struct_name,
            struct_ser,
            struct_des,
            struct_des_owned,
            lifetime,
            &self.cfg,
            element_size,
        )
    }
}

struct CGStructFieldsDef<'a> {
    fields: &'a [Field],
    no_alloc: bool,
}

impl ToTokens for CGStructFieldsDef<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                continue;
            }
            let ident = &struct_field.ident;
            let ty = struct_field.ty.def(self.no_alloc);
            let docs = &struct_field.docs;
            tokens.append_all(quote! {
                #docs
                pub #ident: #ty,
            });
        }
    }
}

struct CGStructSer<'a> {
    item_struct: &'a ItemStruct,
    no_alloc: bool,
}

struct CGStructDes<'a> {
    item_struct: &'a ItemStruct,
    no_alloc: bool,
    owned: bool,
}

impl ToTokens for CGStructSer<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // tokens.append_all(trace_extended_key_val(
        //     "Serialize struct",
        //     self.item_struct.ident.to_string().as_str(),
        // ));
        for struct_field in &self.item_struct.fields {
            let field_name = &struct_field.ident;
            let field_path = if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                FieldPath::Value(quote! {self})
            } else {
                // TODO: if field is already a reference, this is not quite correct, but this information is not used anymore
                FieldPath::Value(quote! {self.#field_name})
            };
            // tokens.append_all(trace_extended_key_val(
            //     "Serialize struct field",
            //     struct_field.ident.to_string().as_str(),
            // ));
            struct_field
                .ty
                .buf_write(field_path, self.no_alloc, quote! { ? }, tokens);
        }
        tokens.append_all(quote! {
            Ok(())
        });
    }
}

impl ToTokens for CGStructDes<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut field_names = vec![];
        // tokens.append_all(trace_extended_key_val(
        //     "Deserialize struct",
        //     self.item_struct.ident.to_string().as_str(),
        // ));
        for struct_field in &self.item_struct.fields {
            let field_name = &struct_field.ident;
            if !matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                field_names.push(field_name.clone());
            }
            let handle_eob = struct_field.handle_eob();
            // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
            // tokens.append_all(trace_extended_key_val(
            //     "Deserialize struct field",
            //     struct_field.ident.to_string().as_str(),
            // ));
            struct_field.ty.buf_read(
                field_name,
                self.no_alloc,
                self.owned,
                handle_eob,
                &quote! { _ },
                tokens,
            );
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
                let value = value.ts();
                quote!(.unwrap_or(#value))
            }
        }
    }
}

// #[cfg(test)]
// mod tests {
//
// }
