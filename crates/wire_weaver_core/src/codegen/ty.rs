use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, TokenStreamExt};
use syn::{Lit, LitInt};

use crate::ast::{Layout, Type};

impl Type {
    pub(crate) fn def(&self, no_alloc: bool) -> TokenStream {
        match self {
            Type::Bool => quote! { bool },
            Type::U4 | Type::U8 => quote! { u8 },
            Type::U16 | Type::Nib16 => quote! { u16 },
            Type::U32 | Type::ULeb32 => quote! { u32 },
            Type::U64 | Type::ULeb64 => quote! { u64 },
            Type::U128 | Type::ULeb128 => quote! { u128 },
            Type::I4 | Type::I8 => quote! { i8 },
            Type::I16 => quote! { i16 },
            Type::I32 | Type::ILeb32 => quote! { i32 },
            Type::I64 | Type::ILeb64 => quote! { i64 },
            Type::I128 | Type::ILeb128 => quote! { i128 },
            Type::F32 => quote! { f32 },
            Type::F64 => quote! { f64 },
            Type::Bytes => {
                if no_alloc {
                    quote! { RefVec<'i, u8> }
                } else {
                    quote! { Vec<u8> }
                }
            }
            Type::String => {
                if no_alloc {
                    quote! { &'i str }
                } else {
                    quote! { String }
                }
            }
            Type::Array(len, layout) => {
                let item_ty = match layout {
                    Layout::Builtin(ty) => ty.def(no_alloc),
                    Layout::Option(ty) => ty.def(no_alloc),
                    Layout::Result(_ok_err_ty) => unimplemented!(),
                    Layout::Unsized(_) => unimplemented!(),
                    Layout::Sized(_, _) => unimplemented!(),
                };
                let len = Lit::Int(LitInt::new(format!("{}", len).as_str(), Span::call_site()));
                quote! { [#item_ty; #len] }
            }
            Type::Tuple(types) => {
                let types = types.iter().map(|ty| ty.def(no_alloc));
                quote! { ( #(#types),* ) }
            }
            Type::Vec(_) => unimplemented!(),
            Type::User(user_layout) => {
                let path = user_layout.path();
                quote! { #path }
            }
            Type::IsSome | Type::IsOk => quote! { bool },
        }
    }

    pub(crate) fn buf_write(
        &self,
        field_path: TokenStream,
        no_alloc: bool,
        tokens: &mut TokenStream,
    ) {
        let write_fn = match self {
            Type::Bool => "write_bool",
            Type::U4 => "write_u4",
            Type::U8 => "write_u8",
            Type::U16 => "write_u16",
            Type::Nib16 => "write_vlu16n",
            Type::U32 => "write_u32",
            Type::U64 => "write_u64",
            Type::U128 => "write_u128",
            Type::I4 => "write_i4",
            Type::I8 => "write_i8",
            Type::I16 => "write_i16",
            Type::I32 => "write_i32",
            Type::I64 => "write_i64",
            Type::I128 => "write_i128",
            Type::F32 => "write_f32",
            Type::F64 => "write_f64",
            Type::Bytes => {
                if no_alloc {
                    "write_bytes"
                } else {
                    tokens.append_all(quote! { wr.write_raw_slice(&#field_path)?; });
                    return;
                }
            }
            Type::String => {
                if no_alloc {
                    "write_string"
                } else {
                    tokens.append_all(quote! { wr.write_string(#field_path.as_str())?; });
                    return;
                }
            }
            Type::IsSome => {
                tokens.append_all(quote! { wr.write_bool(#field_path.is_some())?; });
                return;
            }
            Type::IsOk => {
                tokens.append_all(quote! { wr.write_bool(#field_path.is_ok())?; });
                return;
            }
            Type::ULeb32 => unimplemented!(),
            Type::ULeb64 => unimplemented!(),
            Type::ULeb128 => unimplemented!(),
            Type::ILeb32 => unimplemented!(),
            Type::ILeb64 => unimplemented!(),
            Type::ILeb128 => unimplemented!(),
            Type::Array(_, _) => unimplemented!(),
            Type::Tuple(_) => unimplemented!(),
            Type::Vec(_) => unimplemented!(),
            Type::User(_) => unimplemented!(),
        };
        let write_fn = Ident::new(write_fn, Span::call_site());
        tokens.append_all(quote! { wr.#write_fn(#field_path)?; });
    }

    pub(crate) fn buf_read(
        &self,
        variable_name: Ident,
        no_alloc: bool,
        handle_eob: TokenStream,
        tokens: &mut TokenStream,
    ) {
        let read_fn = match self {
            Type::Bool => "read_bool",
            Type::U4 => "read_u4",
            Type::U8 => "read_u8",
            Type::U16 => "read_u16",
            Type::U32 => "read_u32",
            Type::U64 => "read_u64",
            Type::U128 => "read_u128",
            Type::Nib16 => "read_nib16",
            Type::ULeb32 => unimplemented!(),
            Type::ULeb64 => unimplemented!(),
            Type::ULeb128 => unimplemented!(),
            Type::I4 => unimplemented!(),
            Type::I8 => "read_i8",
            Type::I16 => "read_i16",
            Type::I32 => "read_i32",
            Type::I64 => "read_i64",
            Type::I128 => "read_i128",
            Type::ILeb32 => unimplemented!(),
            Type::ILeb64 => unimplemented!(),
            Type::ILeb128 => unimplemented!(),
            Type::F32 => "read_f32",
            Type::F64 => "read_f64",
            Type::Bytes => "read_bytes",
            Type::String => {
                if no_alloc {
                    "read_string"
                } else {
                    tokens.append_all(
                        quote! { let #variable_name = rd.read_string() #handle_eob .to_string(); },
                    );
                    return;
                }
            }
            Type::Array(_, _) => unimplemented!(),
            Type::Tuple(_) => unimplemented!(),
            Type::Vec(_) => unimplemented!(),
            Type::User(_) => unimplemented!(),
            Type::IsSome => unimplemented!(),
            Type::IsOk => unimplemented!(),
        };
        let read_fn = Ident::new(read_fn, Span::call_site());
        tokens.append_all(quote! { let #variable_name = rd.#read_fn() #handle_eob; })
    }
}
