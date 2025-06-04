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
                // size is handled here as a small optimization, generic write and read implementations would also work.
                // Since String is very simple and does not contain any inner objects, there is no need to do no-op calculations.
                let field_path_value = field_path.clone().by_value();
                let field_path_ref = field_path.by_ref();
                tokens.append_all(quote! {
                    let len = u16::try_from(#field_path_value.len()).map_err(|_| ShrinkWrapError::StrTooLong)?;
                    wr.write_u16_rev(len)?;
                });
                if no_alloc {
                    tokens.append_all(quote! { wr.write_raw_str(#field_path_ref) #handle_eob; });
                    return;
                } else {
                    tokens.append_all(
                        quote! { wr.write_raw_str(#field_path_ref.as_str()) #handle_eob; },
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
                // same as Sized, special handling of Unsized moved to the BufWriter::write and BufRead::read instead
                tokens.append_all(quote! { wr.write(#field_path) #handle_eob; });
                return;
            }
        };
        let write_fn = Ident::new(write_fn, Span::call_site());
        let field_path = field_path.by_value();
        tokens.append_all(quote! { wr.#write_fn(#field_path) #handle_eob; });
    }

    // fn write_unsized(field_path: TokenStream, handle_eob: TokenStream) -> TokenStream {
    //     quote! {
    //         wr.align_byte();
    //         // reserve one size slot
    //         let size_slot_pos = wr.write_u16_rev(0) #handle_eob;
    //         let unsized_start_bytes = wr.pos().0;
    //         wr.write(#field_path) #handle_eob;
    //         // type's serializer might have written several nib16_rev's as well,
    //         // encode and place them after type's data
    //         wr.encode_nib16_rev(wr.u16_rev_pos(), size_slot_pos) #handle_eob;
    //         // e.g., enum, only one nib discriminant is written => need to align
    //         wr.align_byte();
    //         let size_bytes = wr.pos().0 - unsized_start_bytes;
    //         let Ok(size_bytes) = u16::try_from(size_bytes) else {
    //             return Err(ShrinkWrapError::ItemTooLong);
    //         };
    //         // write actual Unsized size
    //         wr.update_u16_rev(size_slot_pos, size_bytes) #handle_eob;
    //     }
    // }

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
                tokens.append_all(quote! { let #variable_name = rd.read()?; });
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
                        quote! { let #variable_name = rd_split.read_raw_str() #handle_eob; },
                    );
                    return;
                } else {
                    tokens.append_all(quote! { let #variable_name = rd_split.read_raw_str() #handle_eob .to_string(); });
                    return;
                }
            }
            Type::Array(_, _) => unimplemented!("array"),
            Type::Tuple(_) => unimplemented!("tuple"),
            Type::Vec(layout) => match layout {
                Layout::Builtin(_inner_ty) => {
                    // TODO: how to handle eob to be zero length?
                    tokens.append_all(quote! { let #variable_name = rd.read()?; });
                    return;
                }
                Layout::Option(_) => unimplemented!("vec of options"),
                Layout::Result(_) => unimplemented!("vec of results"),
            },
            // Type::User(_) => unimplemented!(),
            Type::Unsized(_, _) => {
                tokens.append_all(quote! {
                    // let size = rd.read_unib32_rev()? as usize;
                    // let mut rd_split = rd.split(size)?;
                    let #variable_name = rd.read()?;
                });
                return;
            }
            Type::Sized(_, _) => {
                tokens.append_all(quote! {
                    let #variable_name = rd.read()?;
                });
                return;
            }
            Type::Result(flag_ident, _ok_err_ty) => {
                let is_ok: Ident = flag_ident.into();
                tokens.append_all(quote! {
                    let #variable_name = if #is_ok {
                        Ok(rd.read()?)
                    } else {
                        Err(rd.read()?)
                    };
                });
                return;
            }
            Type::Option(flag_ident, _option_ty) => {
                let is_some: Ident = flag_ident.into();
                tokens.append_all(quote! {
                    let #variable_name = if #is_some {
                        Some(rd.read()?)
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

    /// Return ElementSize if it is known. None is returned for Unsized.
    pub fn element_size(&self) -> Option<ElementSize> {
        let size_bits = match self {
            Type::Bool => 1,
            Type::U4 => 4,
            Type::U8 => 8,
            Type::U16 => 16,
            Type::U32 => 32,
            Type::U64 => 64,
            Type::U128 => 128,
            Type::UNib32 => return Some(ElementSize::SelfDescribing),
            Type::ULeb32 => return Some(ElementSize::SelfDescribing),
            Type::ULeb64 => return Some(ElementSize::SelfDescribing),
            Type::ULeb128 => return Some(ElementSize::SelfDescribing),
            Type::I4 => 4,
            Type::I8 => 8,
            Type::I16 => 16,
            Type::I32 => 32,
            Type::I64 => 64,
            Type::I128 => 128,
            Type::ILeb32 => return Some(ElementSize::SelfDescribing),
            Type::ILeb64 => return Some(ElementSize::SelfDescribing),
            Type::ILeb128 => return Some(ElementSize::SelfDescribing),
            Type::F32 => 32,
            Type::F64 => 64,
            Type::String => return Some(ElementSize::Unsized),
            Type::Array(len, layout) => {
                return match layout {
                    Layout::Builtin(ty) => {
                        let size = match ty.element_size()? {
                            ElementSize::Unsized => ElementSize::Unsized,
                            ElementSize::UnsizedFinalStructure => {
                                ElementSize::UnsizedFinalStructure
                            }
                            ElementSize::SelfDescribing => ElementSize::SelfDescribing,
                            ElementSize::Sized { size_bits } => ElementSize::Sized {
                                size_bits: len * size_bits,
                            },
                        };
                        Some(size)
                    }
                    Layout::Option(some_ty) => {
                        Some(ElementSize::SelfDescribing.add(some_ty.element_size()?))
                    }
                    Layout::Result(ok_err_ty) => {
                        let mut sum = ElementSize::SelfDescribing;
                        sum = sum.add(ok_err_ty.0.element_size()?);
                        sum = sum.add(ok_err_ty.1.element_size()?);
                        Some(sum)
                    }
                };
            }
            Type::Tuple(types) => {
                let mut sum = ElementSize::Sized { size_bits: 0 };
                for ty in types {
                    sum = sum.add(ty.element_size()?);
                }
                return Some(sum);
            }
            Type::Vec(_) => return Some(ElementSize::UnsizedFinalStructure),
            Type::Unsized(_, _) => return None, // cannot know if it's actually Unsized or not, const calculation will be performed instead
            Type::Sized(_, _) => {
                return Some(ElementSize::Sized { size_bits: 0 }); // TODO: size of Sized is important?
                // unimplemented!("element_size of Sized");
            }
            Type::IsSome(_) | Type::IsOk(_) => return Some(ElementSize::Sized { size_bits: 1 }),
            Type::Result(_, ok_err_ty) => {
                let mut sum = ElementSize::SelfDescribing;
                sum = sum.add(ok_err_ty.0.element_size()?);
                sum = sum.add(ok_err_ty.1.element_size()?);
                return Some(sum);
            }
            Type::Option(_, option_ty) => {
                return Some(option_ty.element_size()?.add(ElementSize::SelfDescribing));
            }
        };
        Some(ElementSize::Sized { size_bits })
    }
}
