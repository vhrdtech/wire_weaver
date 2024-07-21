use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, TokenStreamExt};

use crate::ast2::{Type, UserLayout};

impl Type {
    fn plain_write_fn(&self) -> Option<Ident> {
        let write_fn = match self {
            Type::Bool => "write_bool",
            Type::U4 => "write_u4",
            Type::U8 => "write_u8",
            Type::U16 => "write_u16",
            Type::Unib16 => "write_vlu16n",
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
            _ => return None,
        };
        Some(Ident::new(write_fn, Span::call_site()))
    }
}

pub(crate) fn cg_op_write(
    op: &Type,
    field_path: TokenStream,
    alloc: bool,
    tokens: &mut TokenStream,
) {
    if let Some(write_fn) = op.plain_write_fn() {
        tokens.append_all(quote! { wr.#write_fn(#field_path)?; });
    } else {
        match op {
            Type::ULeb128(_native_integer) => {
                unimplemented!()
            }
            Type::ILeb128(_native_integer) => {
                unimplemented!()
            }
            Type::Bytes => {
                if alloc {
                    tokens.append_all(quote! { wr.write_raw_slice(&#field_path)?; });
                } else {
                    tokens.append_all(quote! { wr.write_raw_slice(#field_path)?; });
                }
            }
            Type::String => {
                if alloc {
                    tokens.append_all(quote! { wr.write_string(#field_path.as_str())?; });
                } else {
                    tokens.append_all(quote! { wr.write_string(#field_path)?; });
                }
            }
            Type::Array(len, layout) => {}
            Type::Tuple(ops) => {}
            Type::Vec(layout) => {}
            Type::User(layout) => match layout {
                UserLayout::Unsized(ty_name) => {}
                UserLayout::Sized(ty_name, size) => {}
            },
            Type::IsSome => {
                tokens.append_all(quote! { wr.write_bool(#field_path.is_some())?; });
            }
            Type::IsOk => {
                tokens.append_all(quote! { wr.write_bool(#field_path.is_ok())?; });
            }
            _ => {}
        }
    }
}
