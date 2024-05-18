use crate::ast::ty::{Type, TypeDiscrete};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

impl TypeDiscrete {
    fn sign(&self) -> char {
        if self.is_signed {
            'i'
        } else {
            'u'
        }
    }
}

impl Type {
    pub fn ty_def(&self, no_alloc: bool) -> TokenStream {
        match self {
            Type::Bool => quote!(bool),
            Type::Discrete(ty_discrete) => {
                let is_nib = ty_discrete.bits == 4 && !ty_discrete.is_signed;
                if [8, 16, 32, 64, 128].contains(&ty_discrete.bits) {
                    let sign = ty_discrete.sign();
                    let ty = format!("{sign}{}", ty_discrete.bits);
                    let ty = Ident::new(ty.as_str(), Span::call_site());
                    quote!(#ty)
                } else if is_nib {
                    quote!(u8)
                } else {
                    unimplemented!()
                }
            }
            Type::Floating(ty_floating) => {
                if ty_floating.bits == 32 || ty_floating.bits == 64 {
                    let ty = format!("f{}", ty_floating.bits);
                    let ty = Ident::new(ty.as_str(), Span::call_site());
                    quote!(#ty)
                } else {
                    unimplemented!()
                }
            }
            Type::String => {
                if no_alloc {
                    quote!(&'i str)
                } else {
                    quote!(String)
                }
            }
            Type::Path(path) => {
                let segments = &path.segments;
                quote!(#(#segments)::*)
            }
        }
    }

    pub fn is_sized(&self) -> bool {
        match self {
            Type::Bool => true,
            Type::Discrete(_) => true,
            Type::Floating(_) => true,
            Type::String => false,
            // TODO: need to resolve path's before codegen
            Type::Path(_) => todo!(),
        }
    }

    pub fn is_ref(&self) -> bool {
        match self {
            Type::Bool => false,
            Type::Discrete(_) => false,
            Type::Floating(_) => false,
            Type::String => false,
            Type::Path(_) => false,
        }
    }

    pub fn buf_write(&self, field_path: TokenStream, no_alloc: bool) -> TokenStream {
        match self {
            Type::Bool | Type::Discrete(_) | Type::Floating(_) => {
                let fn_name = match self {
                    Type::Bool => Ident::new("write_bool", Span::call_site()),
                    Type::Discrete(ty_discrete) => {
                        let sign = ty_discrete.sign();
                        let fn_name = format!("write_{sign}{}", ty_discrete.bits);
                        Ident::new(fn_name.as_str(), Span::call_site())
                    }
                    Type::Floating(ty_floating) => {
                        let fn_name = format!("write_f{}", ty_floating.bits);
                        Ident::new(fn_name.as_str(), Span::call_site())
                    }
                    _ => unreachable!(),
                };
                quote!(wr.#fn_name(#field_path)?;)
            }
            Type::String => {
                if no_alloc {
                    quote!(wr.write_str(#field_path)?;)
                } else {
                    quote!(wr.write_str(#field_path.as_str())?;)
                }
            }
            Type::Path(_) => {
                quote! {
                    let handle = wr.write_u16_rev(0)?;
                    let unsized_start = wr.pos().0;
                    wr.write(& #field_path)?;
                    wr.align_byte();
                    wr.encode_vlu16n_rev(handle)?;
                    let size = wr.pos().0 - unsized_start;
                    wr.update_u16_rev(handle, size as u16)?;
                }
            }
        }
    }

    pub fn buf_read(
        &self,
        variable_name: Ident,
        handle_eob: TokenStream,
        no_alloc: bool,
    ) -> TokenStream {
        match self {
            Type::Bool | Type::Discrete(_) | Type::Floating(_) => {
                let fn_name = match self {
                    Type::Bool => Ident::new("read_bool", Span::call_site()),
                    Type::Discrete(ty_discrete) => {
                        let sign = ty_discrete.sign();
                        let fn_name = format!("read_{sign}{}", ty_discrete.bits);
                        Ident::new(fn_name.as_str(), Span::call_site())
                    }
                    Type::Floating(ty_floating) => {
                        let fn_name = format!("read_f{}", ty_floating.bits);
                        Ident::new(fn_name.as_str(), Span::call_site())
                    }
                    _ => unreachable!(),
                };
                quote!(let #variable_name = rd.#fn_name() #handle_eob;)
            }
            Type::String => {
                if no_alloc {
                    quote!(let #variable_name = rd.read_str() #handle_eob;)
                } else {
                    quote!(let #variable_name = rd.read_str() #handle_eob .to_string();)
                }
            }
            Type::Path(_) => {
                quote! {
                    let size = rd.read_vlu16n_rev()? as usize;
                    let mut rd_split = rd.split(size)?;
                    let #variable_name = rd_split.read()?;
                }
            }
        }
    }
}
