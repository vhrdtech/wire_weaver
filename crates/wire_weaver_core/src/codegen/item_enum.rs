use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};

use crate::ast::ident::Ident as WWIdent;
use crate::ast::item::Repr;
use crate::ast2::Fields;
use crate::codegen::op::cg_op_write;

pub fn cg_enum_serdes(
    repr: Repr,
    enum_name: WWIdent,
    variants: &[(WWIdent, u32, Fields)],
    alloc: bool,
    tokens: &mut TokenStream,
) {
    cg_enum_ser(repr, enum_name.clone(), variants, alloc, tokens);
    cg_enum_des(repr, enum_name, variants, alloc, tokens);
}

fn write_discriminant(repr: Repr, tokens: &mut TokenStream) {
    let write_fn = match repr {
        Repr::U4 => "write_u4",
        Repr::U8 => "write_u8",
        Repr::U16 => "write_u16",
        Repr::Vlu16N => "write_vlu16n",
        Repr::U32 => "write_u32",
    };
    let write_fn = Ident::new(write_fn, Span::call_site());
    tokens.append_all(quote! { wr.#write_fn(self.discriminant())?; });
}

fn cg_enum_ser(
    repr: Repr,
    enum_name: WWIdent,
    variants: &[(WWIdent, u32, Fields)],
    alloc: bool,
    tokens: &mut TokenStream,
) {
    write_discriminant(repr, tokens);
    let mut ser_data_variants = TokenStream::new();
    let enum_name: Ident = enum_name.into();
    for (variant_name, _discriminant, fields) in variants {
        match fields {
            Fields::Named(named) => {
                let mut fields_names = vec![];
                let mut ser = quote!();
                for (field_name, op) in named {
                    let field_name: Ident = field_name.into();
                    fields_names.push(field_name.clone());
                    let field_path = quote!(#field_name);
                    cg_op_write(op, field_path, alloc, &mut ser);
                }
                let variant_name: Ident = variant_name.into();
                ser_data_variants.append_all(
                    quote!(#enum_name::#variant_name { #(#fields_names),* } => { #ser }),
                );
            }
            Fields::Unnamed(_) => {}
            Fields::Unit => {}
        }
    }
}

fn cg_enum_des(
    repr: Repr,
    enum_name: WWIdent,
    variants: &[(WWIdent, u32, Fields)],
    alloc: bool,
    tokens: &mut TokenStream,
) {
}
