use crate::ast::{Field, ItemStruct, Type};
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{
    assert_element_size, element_size_ts, serdes, strings_to_derive, sum_element_sizes_recursively,
};
use crate::transform::syn_util::trace_extended_key_val;
use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use shrink_wrap::ElementSize;

pub fn struct_def(item_struct: &ItemStruct, no_alloc: bool) -> TokenStream {
    let ident = &item_struct.ident;
    let fields = CGStructFieldsDef {
        fields: &item_struct.fields,
        no_alloc,
    };
    let lifetime = if no_alloc && item_struct.potential_lifetimes() {
        quote!(<'i>)
    } else {
        quote!()
    };
    let derive = strings_to_derive(&item_struct.derive);
    let docs = &item_struct.docs;
    let cfg = item_struct.cfg();
    let assert_size = if let Some(size) = item_struct.size_assumption {
        assert_element_size(&item_struct.ident, size, item_struct.cfg.clone())
    } else {
        quote! {}
    };
    let ts = quote! {
        #cfg
        #docs
        #derive
        pub struct #ident #lifetime { #fields }
        #assert_size
    };
    ts
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
}

pub fn struct_serdes(item_struct: &ItemStruct, no_alloc: bool) -> TokenStream {
    let struct_name = &item_struct.ident;
    let struct_ser = CGStructSer {
        item_struct,
        no_alloc,
    };
    let struct_des = CGStructDes {
        item_struct,
        no_alloc,
    };
    let lifetime = if no_alloc && item_struct.potential_lifetimes() {
        quote!(<'i>)
    } else {
        quote!()
    };

    let mut unknown_unsized = vec![];
    let mut sum = item_struct.size_assumption.unwrap_or(ElementSize::Unsized); // struct is Unsized by default.
    // No need to check if it's already Unsized.
    if !matches!(sum, ElementSize::Unsized) {
        // NOTE: make sure to not accidentally bump Unsized to UFS here if any of the fields is UFS.
        // See ElementSize docs and comments on sum method.
        for f in &item_struct.fields {
            if let Some(size) = f.ty.element_size() {
                sum = sum.add(size);
            }
            if let Type::External(path, _) = &f.ty {
                if let Some(ident) = path.segments.last() {
                    unknown_unsized.push(ident.clone());
                }
            }
        }
    }
    let implicitly_unsized = sum.is_unsized() && unknown_unsized.is_empty();
    let element_size = if implicitly_unsized {
        element_size_ts(ElementSize::Unsized)
    } else {
        sum_element_sizes_recursively(sum, unknown_unsized)
    };
    serdes(
        struct_name,
        struct_ser,
        struct_des,
        lifetime,
        item_struct.cfg(),
        element_size,
    )
}

impl ToTokens for CGStructSer<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(trace_extended_key_val(
            "Serialize struct",
            self.item_struct.ident.to_string().as_str(),
        ));
        for struct_field in &self.item_struct.fields {
            let field_name = &struct_field.ident;
            let field_path = if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                FieldPath::Value(quote! {self})
            } else {
                // TODO: if field is already a reference, this is not quite correct, but this information is not used anymore
                FieldPath::Value(quote! {self.#field_name})
            };
            tokens.append_all(trace_extended_key_val(
                "Serialize struct field",
                struct_field.ident.to_string().as_str(),
            ));
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
        tokens.append_all(trace_extended_key_val(
            "Deserialize struct",
            self.item_struct.ident.to_string().as_str(),
        ));
        for struct_field in &self.item_struct.fields {
            let field_name = &struct_field.ident;
            if !matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                field_names.push(field_name.clone());
            }
            let handle_eob = struct_field.handle_eob();
            // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
            tokens.append_all(trace_extended_key_val(
                "Deserialize struct field",
                struct_field.ident.to_string().as_str(),
            ));
            struct_field
                .ty
                .buf_read(field_name, self.no_alloc, handle_eob, tokens);
        }
        let struct_name = &self.item_struct.ident;
        tokens.append_all(quote! {
            Ok(#struct_name {
                #(#field_names),*
            })
        });
    }
}

// #[cfg(test)]
// mod tests {
//
// }
