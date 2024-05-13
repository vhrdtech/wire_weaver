use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};
use ww_ast::item::{Item, ItemStruct, StructField};
use ww_ast::File;

pub fn rust_no_std_file(file: &File) -> TokenStream {
    let mut ts = TokenStream::new();
    for item in &file.items {
        match item {
            Item::Enum(_) => {}
            Item::Struct(item_struct) => {
                ts.append_all(rust_no_std_struct_def(item_struct));
                ts.append_all(rust_no_std_struct_serde(item_struct));
            }
        }
    }
    ts
}

struct CGFieldsDef<'a> {
    fields: &'a [StructField],
}

impl<'a> ToTokens for CGFieldsDef<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            let ident: Ident = (&struct_field.ident).into();
            let ty = struct_field.ty.to_tokens();
            tokens.append_all(quote! {
                pub #ident: #ty,
            });
        }
    }
}

pub fn rust_no_std_struct_def(item_struct: &ItemStruct) -> TokenStream {
    let ident: Ident = (&item_struct.ident).into();
    let fields = CGFieldsDef {
        fields: &item_struct.fields,
    };
    let ts = quote! { pub struct #ident { #fields } };
    ts
}

struct CGFieldsSer<'a> {
    fields: &'a [StructField],
}

pub fn rust_no_std_struct_serde(item_struct: &ItemStruct) -> TokenStream {
    let ident: Ident = (&item_struct.ident).into();
    let fields_ser = CGFieldsSer {
        fields: &item_struct.fields,
    };
    quote! {
        impl #ident {
            pub fn ser_wfdb(&self, wr: &mut wfdb::WfdbBufMut) -> Result<(), wfdb::Error> {
                #fields_ser
            }
        }
    }
}

impl<'a> ToTokens for CGFieldsSer<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            let ident: Ident = (&struct_field.ident).into();
            let ser_fn = struct_field.ty.to_ser_fn_name();
            tokens.append_all(quote! {
                wr.#ser_fn(self.#ident)?;
            });
        }
        tokens.append_all(quote! {
            Ok(())
        });
    }
}
