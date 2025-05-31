use std::ops::Deref;

use proc_macro2::{Ident, Span, TokenStream};
use quote::{TokenStreamExt, quote};
use syn::{Lit, LitInt};

use shrink_wrap::ElementSize;

use crate::ast::{Layout, Type};

#[derive(Clone)]
pub(crate) enum FieldPath {
    Ref(TokenStream),
    Value(TokenStream),
}

impl FieldPath {
    fn by_ref(self) -> TokenStream {
        match self {
            FieldPath::Ref(path) => path,
            FieldPath::Value(path) => quote! { &#path },
        }
    }

    fn by_value(self) -> TokenStream {
        match self {
            FieldPath::Ref(path) => quote! { *#path },
            FieldPath::Value(path) => path,
        }
    }

    fn as_provided(self) -> TokenStream {
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
    pub(crate) fn def(&self, no_alloc: bool) -> TokenStream {
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
            Type::Array(len, layout) => {
                let item_ty = match layout {
                    Layout::Builtin(ty) => ty.def(no_alloc),
                    Layout::Option(ty) => ty.def(no_alloc),
                    Layout::Result(_ok_err_ty) => unimplemented!("array of results"),
                    // Layout::Unsized(_) => unimplemented!(),
                    // Layout::Sized(_, _) => unimplemented!(),
                };
                let len = Lit::Int(LitInt::new(format!("{}", len).as_str(), Span::call_site()));
                quote! { [#item_ty; #len] }
            }
            Type::Tuple(types) => {
                let types = types.iter().map(|ty| ty.def(no_alloc));
                quote! { ( #(#types),* ) }
            }
            Type::Vec(layout) => match layout {
                Layout::Builtin(inner_ty) => {
                    let inner_ty = inner_ty.def(no_alloc);
                    if no_alloc {
                        quote! { RefVec<'i, #inner_ty> }
                    } else {
                        quote! { Vec<#inner_ty> }
                    }
                }
                Layout::Option(_) => unimplemented!("vec of options"),
                Layout::Result(_) => unimplemented!("vec of results"),
            },
            // Type::User(user_layout) => {
            //     let path = user_layout.path();
            //     quote! { #path }
            // }
            Type::Unsized(path, is_lifetime) | Type::Sized(path, is_lifetime) => {
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
            Type::IsSome(_) | Type::IsOk(_) => quote! { bool },
        }
    }

    pub(crate) fn arg_pos_def(&self, no_alloc: bool) -> TokenStream {
        match self {
            Type::String => {
                if no_alloc {
                    quote! { &str }
                } else {
                    quote! { String }
                }
            }
            Type::Vec(layout) => match layout {
                Layout::Builtin(inner_ty) => {
                    let inner_ty = inner_ty.def(no_alloc);
                    if no_alloc {
                        quote! { RefVec<'_, #inner_ty> }
                    } else {
                        quote! { Vec<#inner_ty> }
                    }
                }
                Layout::Option(_) => unimplemented!("vec of options"),
                Layout::Result(_) => unimplemented!("vec of results"),
            },
            Type::Unsized(path, is_lifetime) | Type::Sized(path, is_lifetime) => {
                if *is_lifetime && no_alloc {
                    quote! { #path<'_> }
                } else {
                    quote! { #path }
                }
            }
            _ => self.def(no_alloc),
        }
    }

    pub(crate) fn buf_write(
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
            Type::String => {
                let field_path_value = field_path.clone().by_value();
                let field_path_ref = field_path.by_ref();
                tokens.append_all(quote! {
                    let len = u16::try_from(#field_path_value.len()).map_err(|_| ShrinkWrapError::StrTooLong)?;
                    wr.write_u16_rev(len)?;
                });
                if no_alloc {
                    tokens.append_all(quote! { wr.write_string(#field_path_ref) #handle_eob; });
                    return;
                } else {
                    tokens.append_all(
                        quote! { wr.write_string(#field_path_ref.as_str()) #handle_eob; },
                    );
                    return;
                }
            }
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
            Type::Option(_, _) => {
                let field_path = field_path.by_ref();
                tokens.append_all(quote! {
                    if let Some(v) = #field_path {
                        wr.write(v) #handle_eob;
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
                let field_path = field_path.by_ref();
                tokens.append_all(quote! {
                    match #field_path {
                        Ok(v) => {
                            wr.write(v) #handle_eob;
                        }
                        Err(e) => {
                            wr.write(e) #handle_eob;
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
            Type::Array(_, _) => unimplemented!("array"),
            Type::Tuple(_) => unimplemented!("tuple"),
            Type::Vec(layout) => {
                match layout {
                    Layout::Builtin(inner_ty) => {
                        let is_vec_u8 = matches!(inner_ty.deref(), Type::U8);
                        if is_vec_u8 && no_alloc {
                            let field_path = field_path.as_provided();
                            tokens.append_all(
                                quote! { #field_path.ser_shrink_wrap_vec_u8(wr) #handle_eob; },
                            );
                        } else {
                            let field_path = field_path.by_ref();
                            tokens.append_all(quote! { wr.write(#field_path) #handle_eob; });
                        }
                    }
                    Layout::Option(_) => unimplemented!("vec of options"),
                    Layout::Result(_) => unimplemented!("vec of results"),
                }
                return;
            }
            // Type::User(_) => unimplemented!(),
            Type::Sized(_, _) => {
                let field_path = field_path.by_ref();
                tokens.append_all(quote! { wr.write(#field_path) #handle_eob; });
                return;
            }
            Type::Unsized(_path, _) => {
                let field_path = field_path.by_ref();
                tokens.append_all(quote! {
                    wr.align_byte();
                    // reserve one size slot
                    let size_slot_pos = wr.write_u16_rev(0) #handle_eob;
                    let unsized_start_bytes = wr.pos().0;
                    wr.write(#field_path) #handle_eob;
                    // encode Type's internal sizes if any
                    wr.encode_nib16_rev(wr.u16_rev_pos(), size_slot_pos) #handle_eob;
                    // e.g. plain enum, only one nib discriminant is written => need to align
                    wr.align_byte();
                    let size_bytes = wr.pos().0 - unsized_start_bytes;
                    let Ok(size_bytes) = u16::try_from(size_bytes) else {
                        return Err(ShrinkWrapError::ItemTooLong);
                    };
                    // write Unsized size
                    wr.update_u16_rev(size_slot_pos, size_bytes) #handle_eob;
                });
                return;
            }
        };
        let write_fn = Ident::new(write_fn, Span::call_site());
        let field_path = field_path.by_value();
        tokens.append_all(quote! { wr.#write_fn(#field_path) #handle_eob; });
    }

    pub(crate) fn buf_read(
        &self,
        variable_name: Ident,
        no_alloc: bool,
        handle_eob: TokenStream,
        tokens: &mut TokenStream,
    ) {
        let read_fn = match self {
            Type::Bool | Type::IsOk(_) | Type::IsSome(_) => "read_bool",
            Type::U4 => "read_u4",
            Type::U8 => "read_u8",
            Type::U16 => "read_u16",
            Type::U32 => "read_u32",
            Type::U64 => "read_u64",
            Type::U128 => "read_u128",
            Type::UNib32 => {
                let element_size = Type::UNib32.element_size_ts();
                tokens.append_all(quote! { let #variable_name = rd.read(#element_size)?; });
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
            // Type::Bytes => "read_bytes",
            Type::String => {
                tokens.append_all(quote! {
                    let str_len = rd.read_unib32_rev()? as usize;
                    let mut rd_split = rd.split(str_len)?;
                });
                if no_alloc {
                    tokens.append_all(
                        quote! { let #variable_name = rd_split.read_string() #handle_eob; },
                    );
                    return;
                } else {
                    tokens.append_all(quote! { let #variable_name = rd_split.read_string() #handle_eob .to_string(); });
                    return;
                }
            }
            Type::Array(_, _) => unimplemented!("array"),
            Type::Tuple(_) => unimplemented!("tuple"),
            Type::Vec(layout) => match layout {
                Layout::Builtin(inner_ty) => {
                    // TODO: how to handle eob to be zero length?
                    let inner_element_size = inner_ty.element_size_ts();
                    tokens
                        .append_all(quote! { let #variable_name = rd.read(#inner_element_size)?; });
                    return;
                }
                Layout::Option(_) => unimplemented!("vec of options"),
                Layout::Result(_) => unimplemented!("vec of results"),
            },
            // Type::User(_) => unimplemented!(),
            Type::Unsized(_, _) => {
                tokens.append_all(quote! {
                    let size = rd.read_unib32_rev()? as usize;
                    let mut rd_split = rd.split(size)?;
                    let #variable_name = rd_split.read(ElementSize::Unsized)?;
                });
                return;
            }
            Type::Sized(_, _) => {
                tokens.append_all(quote! {
                    let #variable_name = rd.read(ElementSize::Sized { size_bits: 0 })?;
                });
                return;
            }
            Type::Result(flag_ident, ok_err_ty) => {
                let is_ok: Ident = flag_ident.into();
                let ok_element_size = ok_err_ty.0.element_size_ts();
                let err_element_size = ok_err_ty.1.element_size_ts();
                tokens.append_all(quote! {
                    let #variable_name = if #is_ok {
                        Ok(rd.read(#ok_element_size)?)
                    } else {
                        Err(rd.read(#err_element_size)?)
                    };
                });
                return;
            }
            Type::Option(flag_ident, option_ty) => {
                let is_some: Ident = flag_ident.into();
                let element_size = option_ty.element_size_ts();
                tokens.append_all(quote! {
                    let #variable_name = if #is_some {
                        Some(rd.read(#element_size)?)
                    } else {
                        None
                    };
                });
                return;
            }
        };
        let read_fn = Ident::new(read_fn, Span::call_site());
        tokens.append_all(quote! { let #variable_name = rd.#read_fn() #handle_eob; })
    }

    pub fn element_size(&self) -> ElementSize {
        let size_bits = match self {
            Type::Bool => 1,
            Type::U4 => 4,
            Type::U8 => 8,
            Type::U16 => 16,
            Type::U32 => 32,
            Type::U64 => 64,
            Type::U128 => 128,
            Type::UNib32 => return ElementSize::UnsizedSelfDescribing,
            Type::ULeb32 => return ElementSize::UnsizedSelfDescribing,
            Type::ULeb64 => return ElementSize::UnsizedSelfDescribing,
            Type::ULeb128 => return ElementSize::UnsizedSelfDescribing,
            Type::I4 => 4,
            Type::I8 => 8,
            Type::I16 => 16,
            Type::I32 => 32,
            Type::I64 => 64,
            Type::I128 => 128,
            Type::ILeb32 => return ElementSize::UnsizedSelfDescribing,
            Type::ILeb64 => return ElementSize::UnsizedSelfDescribing,
            Type::ILeb128 => return ElementSize::UnsizedSelfDescribing,
            Type::F32 => 32,
            Type::F64 => 64,
            Type::String => return ElementSize::Unsized,
            Type::Array(len, layout) => match layout {
                Layout::Builtin(ty) => {
                    return match ty.element_size() {
                        ElementSize::Implied => ElementSize::Implied,
                        ElementSize::Unsized => ElementSize::Unsized,
                        ElementSize::Sized { size_bits } => ElementSize::Sized {
                            size_bits: len * size_bits,
                        },
                        ElementSize::UnsizedSelfDescribing => ElementSize::UnsizedSelfDescribing,
                    };
                }
                Layout::Option(_inner_ty) => unimplemented!("array of options"),
                Layout::Result(_inner_ty) => unimplemented!("array of results"),
            },
            Type::Tuple(_) => unimplemented!("tuple"),
            Type::Vec(_) => return ElementSize::Unsized,
            Type::Unsized(_, _) => return ElementSize::Unsized,
            Type::Sized(_, _) => {
                return ElementSize::UnsizedSelfDescribing;
                // unimplemented!("element_size of Sized");
            }
            Type::IsSome(_) | Type::IsOk(_) => return ElementSize::Sized { size_bits: 1 },
            Type::Result(_, _ok_err_ty) => {
                // TODO: Result runtime value dependent size
                eprintln!("!! Result size is not fully implemented");
                return ElementSize::Unsized;
            }
            Type::Option(_, _option_ty) => {
                eprintln!("!! Option size is not fully implemented");
                return ElementSize::Unsized;
            }
        };
        ElementSize::Sized { size_bits }
    }

    pub fn element_size_ts(&self) -> TokenStream {
        match self.element_size() {
            ElementSize::Implied => quote! { ElementSize::Implied },
            ElementSize::Unsized => quote! { ElementSize::Unsized },
            ElementSize::Sized { size_bits } => {
                let size_bits = LitInt::new(format!("{size_bits}").as_str(), Span::call_site());
                quote! { ElementSize::Sized { size_bits: #size_bits } }
            }
            ElementSize::UnsizedSelfDescribing => quote! { ElementSize::UnsizedSelfDescribing },
        }
    }
}
