use crate::ast::Type;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{TokenStreamExt, quote};
use std::ops::Deref;
use syn::{Lit, LitInt};

#[derive(Clone)]
pub enum FieldPath {
    Ref(TokenStream),
    Value(TokenStream),
}

impl FieldPath {
    pub fn by_ref(self) -> TokenStream {
        match self {
            FieldPath::Ref(path) => path,
            FieldPath::Value(path) => quote! { &#path },
        }
    }

    pub fn by_value(self) -> TokenStream {
        match self {
            FieldPath::Ref(path) => quote! { *#path },
            FieldPath::Value(path) => path,
        }
    }

    fn into_inner(self) -> TokenStream {
        match self {
            FieldPath::Ref(path) => path,
            FieldPath::Value(path) => path,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            FieldPath::Ref(path) | FieldPath::Value(path) => path.is_empty(),
        }
    }
}

impl Type {
    pub fn def(&self, no_alloc: bool) -> TokenStream {
        match self {
            Type::Bool => quote! { bool },
            Type::U4 | Type::U8 => quote! { u8 },
            Type::U16 => quote! { u16 },
            Type::UNib32 => quote! { UNib32 },
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
            // Type::Bytes => {
            //     if no_alloc {
            //         quote! { RefVec<'i, u8> }
            //     } else {
            //         quote! { Vec<u8> }
            //     }
            // }
            Type::String => {
                if no_alloc {
                    quote! { &'i str }
                } else {
                    quote! { String }
                }
            }
            Type::Array(len, ty) => {
                let item_ty = ty.def(no_alloc);
                let len = Lit::Int(LitInt::new(format!("{}", len).as_str(), Span::call_site()));
                quote! { [#item_ty; #len] }
            }
            Type::Tuple(types) => {
                let types = types.iter().map(|ty| ty.def(no_alloc));
                quote! { ( #(#types),* ) }
            }
            Type::Vec(inner_ty) => {
                let inner_ty = inner_ty.def(no_alloc);
                if no_alloc {
                    quote! { RefVec<'i, #inner_ty> }
                } else {
                    quote! { Vec<#inner_ty> }
                }
            }
            // Type::User(user_layout) => {
            //     let path = user_layout.path();
            //     quote! { #path }
            // }
            Type::External(path, is_lifetime) => {
                if *is_lifetime && no_alloc {
                    quote! { #path<'i> }
                } else {
                    quote! { #path }
                }
            }
            Type::Result(_, ok_err_ty) => {
                let ok_ty = ok_err_ty.0.def(no_alloc);
                let err_ty = ok_err_ty.1.def(no_alloc);
                quote! { Result<#ok_ty, #err_ty> }
            }
            Type::Option(_, option_ty) => {
                let option_ty = option_ty.def(no_alloc);
                quote! { Option<#option_ty> }
            }
            Type::Range(ty) => {
                let ty = ty.def(no_alloc);
                quote! { core::ops::Range<#ty> }
            }
            Type::RangeInclusive(ty) => {
                let ty = ty.def(no_alloc);
                quote! { core::ops::RangeInclusive<#ty> }
            }
            Type::IsSome(_) | Type::IsOk(_) => quote! { bool },
            Type::RefBox(box_ty) => {
                let box_ty = box_ty.def(no_alloc);
                if no_alloc {
                    quote! { RefBox<'i, #box_ty> }
                } else {
                    quote! { Box<#box_ty>}
                }
            }
        }
    }

    // TODO: make arg_pos_def2 behavior default one
    pub fn arg_pos_def(&self, no_alloc: bool) -> TokenStream {
        match self {
            Type::String => {
                if no_alloc {
                    quote! { &str }
                } else {
                    quote! { String }
                }
            }
            Type::Vec(inner_ty) => {
                let inner_ty = inner_ty.def(no_alloc);
                if no_alloc {
                    quote! { RefVec<'_, #inner_ty> }
                } else {
                    quote! { Vec<#inner_ty> }
                }
            }
            Type::External(path, is_lifetime) => {
                if *is_lifetime && no_alloc {
                    quote! { #path<'_> }
                } else {
                    quote! { #path }
                }
            }
            _ => self.def(no_alloc),
        }
    }

    pub fn arg_pos_def2(&self, no_alloc: bool) -> TokenStream {
        if self.potential_lifetimes() && !no_alloc {
            let mut ty_owned = self.clone();
            ty_owned.make_owned();
            ty_owned.arg_pos_def(no_alloc)
        } else {
            self.arg_pos_def(no_alloc)
        }
    }

    pub fn buf_write(
        &self,
        field_path: FieldPath,
        no_alloc: bool,
        handle_eob: TokenStream,
        tokens: &mut TokenStream,
    ) {
        let write_fn = match self {
            Type::Bool => "write_bool",
            Type::U4 => "write_u4",
            Type::U8 => "write_u8",
            Type::U16 => "write_u16",
            Type::UNib32 => {
                let field_path = field_path.by_ref();
                tokens.append_all(quote! { wr.write(#field_path) #handle_eob; });
                return;
            }
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
            // Type::Bytes => {
            //     if no_alloc {
            //         "write_bytes"
            //     } else {
            //         tokens.append_all(quote! { wr.write_raw_slice(&#field_path)?; });
            //         return;
            //     }
            // }
            // Type::String => {
            //     // size is handled here as a small optimization, generic write and read implementations would also work.
            //     // Since String is very simple and does not contain any inner objects, there is no need to do no-op calculations.
            //     // let field_path_value = field_path.clone().by_value();
            //     let field_path_ref = field_path.by_ref();
            //     tokens.append_all(quote! {
            //         let len = u16::try_from(#field_path_ref.len()).map_err(|_| ShrinkWrapError::StrTooLong)?;
            //         wr.write_u16_rev(len)?;
            //     });
            //     if no_alloc {
            //         tokens.append_all(quote! { wr.write_raw_str(#field_path_ref) #handle_eob; });
            //         return;
            //     } else {
            //         tokens.append_all(
            //             quote! { wr.write_raw_str(#field_path_ref.as_str()) #handle_eob; },
            //         );
            //         return;
            //     }
            // }
            Type::IsSome(option_field) => {
                let path = if field_path.is_empty() {
                    quote! { #option_field }
                } else {
                    let path = field_path.by_value();
                    quote! { #path.#option_field }
                };
                tokens.append_all(quote! { wr.write_bool(#path.is_some()) #handle_eob; });
                return;
            }
            Type::Option(_, _ty) => {
                // handled explicitly, because is_some flag could be relocated using #[flag] attribute and generic
                // Option implementation cannot rely on it
                let field_path = field_path.by_ref();
                tokens.append_all(quote! {
                    if let Some(val) = #field_path {
                        wr.write(val) #handle_eob;
                    }
                });
                return;
            }
            Type::IsOk(result_field) => {
                let path = if field_path.is_empty() {
                    quote! { #result_field }
                } else {
                    let path = field_path.by_value();
                    quote! { #path.#result_field }
                };
                tokens.append_all(quote! { wr.write_bool(#path.is_ok()) #handle_eob; });
                return;
            }
            Type::Result(_flag_ident, _ok_err_ty) => {
                // handled explicitly, because is_ok flag could be relocated using #[flag] attribute and generic
                // Err implementation cannot rely on it
                let field_path = field_path.by_ref();
                tokens.append_all(quote! {
                    match #field_path {
                        Ok(val) => {
                            wr.write(val) #handle_eob;
                        }
                        Err(err) => {
                            wr.write(err) #handle_eob;
                        }
                    }
                });
                return;
            }
            Type::ULeb32 => unimplemented!("uleb32"),
            Type::ULeb64 => unimplemented!("uleb64"),
            Type::ULeb128 => unimplemented!("uleb128"),
            Type::ILeb32 => unimplemented!("ileb32"),
            Type::ILeb64 => unimplemented!("ileb64"),
            Type::ILeb128 => unimplemented!("ileb128"),
            Type::Array(_, _) => {
                let field_path = field_path.by_ref();
                tokens.append_all(quote! { wr.write(#field_path) #handle_eob; });
                return;
            }
            Type::Tuple(_types) => {
                let field_path = field_path.by_ref();
                tokens.append_all(quote! { wr.write(#field_path) #handle_eob; });
                return;
            }
            Type::Vec(inner_ty) => {
                let is_vec_u8 = matches!(inner_ty.deref(), Type::U8);
                if is_vec_u8 && no_alloc {
                    let field_path = field_path.into_inner();
                    tokens
                        .append_all(quote! { #field_path.ser_shrink_wrap_vec_u8(wr) #handle_eob; });
                } else {
                    let field_path = field_path.by_ref();
                    tokens.append_all(quote! { wr.write(#field_path) #handle_eob; });
                }
                return;
            }
            Type::External(_, _)
            | Type::String
            | Type::RefBox(_)
            | Type::Range(_)
            | Type::RangeInclusive(_) => {
                let field_path = field_path.by_ref();
                // same as Sized, special handling of Unsized moved to the BufWriter::write and BufRead::read instead
                tokens.append_all(quote! { wr.write(#field_path) #handle_eob; });
                return;
            }
        };
        let write_fn = Ident::new(write_fn, Span::call_site());
        let field_path = field_path.by_value();
        tokens.append_all(quote! { wr.#write_fn(#field_path) #handle_eob; });
    }

    pub fn buf_read(
        &self,
        variable_name: &Ident,
        _no_alloc: bool,
        owned: bool,
        handle_err: TokenStream,
        enforce_ty: &TokenStream,
        tokens: &mut TokenStream,
    ) {
        let read = if owned {
            quote! { read_owned }
        } else {
            quote! { read }
        };
        let read_fn = match self {
            Type::Bool | Type::IsOk(_) | Type::IsSome(_) => "read_bool",
            Type::U4 => "read_u4",
            Type::U8 => "read_u8",
            Type::U16 => "read_u16",
            Type::U32 => "read_u32",
            Type::U64 => "read_u64",
            Type::U128 => "read_u128",
            Type::UNib32 => {
                tokens.append_all(quote! { let #variable_name = rd.#read() #handle_err; });
                return;
            }
            Type::ULeb32 => unimplemented!("uleb32"),
            Type::ULeb64 => unimplemented!("uleb64"),
            Type::ULeb128 => unimplemented!("uleb128"),
            Type::I4 => unimplemented!("i4"),
            Type::I8 => "read_i8",
            Type::I16 => "read_i16",
            Type::I32 => "read_i32",
            Type::I64 => "read_i64",
            Type::I128 => "read_i128",
            Type::ILeb32 => unimplemented!("ileb32"),
            Type::ILeb64 => unimplemented!("ileb64"),
            Type::ILeb128 => unimplemented!("ileb128"),
            Type::F32 => "read_f32",
            Type::F64 => "read_f64",
            Type::Array(_len, _ty) => {
                tokens.append_all(quote! { let #variable_name = rd.#read() #handle_err; });
                return;
            }
            Type::Tuple(_) => {
                tokens.append_all(quote! { let #variable_name = rd.#read() #handle_err; });
                return;
            }
            Type::Vec(_inner_ty) => {
                // TODO: how to handle eob to be zero length?
                tokens.append_all(quote! { let #variable_name = rd.#read() #handle_err; });
                return;
            }
            Type::External(_, _)
            | Type::String
            | Type::RefBox(_)
            | Type::Range(_)
            | Type::RangeInclusive(_) => {
                tokens.append_all(quote! {
                    let #variable_name = rd.#read() #handle_err;
                });
                return;
            }
            Type::Result(flag_ident, _ok_err_ty) => {
                let is_ok = &flag_ident;
                tokens.append_all(quote! {
                    let #variable_name = if #is_ok {
                        Ok(rd.#read() #handle_err)
                    } else {
                        Err(rd.#read() #handle_err)
                    };
                });
                return;
            }
            Type::Option(flag_ident, _option_ty) => {
                let is_some = &flag_ident;
                tokens.append_all(quote! {
                    let #variable_name = if #is_some {
                        Some(rd.#read() #handle_err)
                    } else {
                        None
                    };
                });
                return;
            }
        };
        let read_fn = Ident::new(read_fn, Span::call_site());
        tokens.append_all(quote! { let #variable_name: #enforce_ty = rd.#read_fn() #handle_err; })
    }

    pub fn is_byte_slice(&self) -> bool {
        let Type::Vec(inner) = self else {
            return false;
        };
        matches!(inner.as_ref(), Type::U8)
    }
}
