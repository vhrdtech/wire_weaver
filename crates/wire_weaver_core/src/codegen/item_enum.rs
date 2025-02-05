use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{Lit, LitInt};

use crate::ast::{Field, Fields, ItemEnum, Repr, Variant};
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{serdes, strings_to_derive};

pub fn enum_def(item_enum: &ItemEnum, no_alloc: bool) -> TokenStream {
    let enum_name: Ident = (&item_enum.ident).into();
    let variants = CGEnumFieldsDef {
        variants: &item_enum.variants,
        no_alloc,
    };
    let lifetime = lifetime(item_enum, no_alloc);
    let repr_ty = enum_discriminant_type(item_enum);
    let derive = strings_to_derive(&item_enum.derive);
    let mut ts = quote! {
        #derive
        #[repr(#repr_ty)]
        pub enum #enum_name #lifetime { #variants }
    };
    ts.append_all(enum_discriminant(item_enum, lifetime));
    ts
}

fn lifetime(item_enum: &ItemEnum, no_alloc: bool) -> TokenStream {
    if no_alloc && item_enum.potential_lifetimes() {
        quote!(<'i>)
    } else {
        quote!()
    }
}

pub fn enum_serdes(item_enum: &ItemEnum, no_alloc: bool) -> TokenStream {
    let enum_name: Ident = (&item_enum.ident).into();
    let enum_ser = CGEnumSer {
        item_enum,
        no_alloc,
    };
    let enum_des = CGEnumDes {
        item_enum,
        no_alloc,
    };
    let lifetime = lifetime(item_enum, no_alloc);
    serdes(enum_name, enum_ser, enum_des, lifetime)
}

fn enum_discriminant_type(item_enum: &ItemEnum) -> Ident {
    let ty = match item_enum.repr {
        Repr::U4 => "u8",
        Repr::U8 => "u8",
        Repr::U16 => "u16",
        Repr::Nib16 => "u16",
        Repr::U32 => "u32",
    };
    Ident::new(ty, Span::call_site())
}

fn enum_discriminant(item_enum: &ItemEnum, lifetime: TokenStream) -> TokenStream {
    let enum_name: Ident = (&item_enum.ident).into();
    let ty = enum_discriminant_type(item_enum);
    quote! {
        impl #lifetime #enum_name #lifetime {
            pub fn discriminant(&self) -> #ty {
                unsafe { *<*const _>::from(self).cast::<#ty>() }
            }
        }
    }
}

struct CGEnumFieldsDef<'a> {
    variants: &'a [Variant],
    no_alloc: bool,
}

impl<'a> ToTokens for CGEnumFieldsDef<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for variant in self.variants {
            let ident: Ident = (&variant.ident).into();
            let discriminant = variant.discriminant_lit();
            let variant = match &variant.fields {
                Fields::Named(fields_named) => {
                    let field_names: Vec<Ident> =
                        fields_named.iter().map(|f| (&f.ident).into()).collect();
                    let field_types: Vec<TokenStream> = fields_named
                        .iter()
                        .map(|f| f.ty.def(self.no_alloc))
                        .collect();
                    quote!(#ident { #(#field_names: #field_types),* } = #discriminant,)
                }
                Fields::Unnamed(fields_unnamed) => {
                    let field_types: Vec<TokenStream> = fields_unnamed
                        .iter()
                        .map(|ty| ty.def(self.no_alloc))
                        .collect();
                    quote!(#ident ( #(#field_types),* ) = #discriminant,)
                }
                Fields::Unit => quote!(#ident = #discriminant,),
            };
            tokens.append_all(variant);
        }
    }
}

struct CGEnumSer<'a> {
    item_enum: &'a ItemEnum,
    no_alloc: bool,
}

struct CGEnumDes<'a> {
    item_enum: &'a ItemEnum,
    no_alloc: bool,
}

fn write_discriminant(repr: Repr, tokens: &mut TokenStream) {
    let write_fn = match repr {
        Repr::U4 => "write_u4",
        Repr::U8 => "write_u8",
        Repr::U16 => "write_u16",
        Repr::Nib16 => "write_nib16",
        Repr::U32 => "write_u32",
    };
    let write_fn = Ident::new(write_fn, Span::call_site());
    tokens.append_all(quote! { wr.#write_fn(self.discriminant())?; });
}

impl<'a> ToTokens for CGEnumSer<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // TODO: forbid empty enums or not?
        // if self.item_enum.variants.is_empty() {
        //     tokens.append_all(quote!( wr.write_vlu16n(0)?; ));
        // } else {
        write_discriminant(self.item_enum.repr, tokens);
        // }

        // if self.item_enum.is_final && !self.item_enum.contains_data_fields() {
        //     tokens.append_all(quote!( Ok(()) ));
        //     return;
        // }

        let enum_name: Ident = (&self.item_enum.ident).into();
        // let unit_variants: Vec<_> = self
        //     .item_enum
        //     .variants
        //     .iter()
        //     .filter_map(|v| {
        //         if v.is_unit() {
        //             Some(v.ident.clone())
        //         } else {
        //             None
        //         }
        //     })
        //     .collect();
        let mut ser_data_variants = quote! {};
        for variant in &self.item_enum.variants {
            match &variant.fields {
                Fields::Named(fields_named) => {
                    let mut fields_names = vec![];
                    let mut ser = quote!();
                    for field in fields_named {
                        let field_name: Ident = (&field.ident).into();
                        fields_names.push(field_name.clone());
                        let field_path = FieldPath::Ref(quote!(#field_name));
                        field.ty.buf_write(field_path, self.no_alloc, &mut ser);
                    }
                    let variant_name: Ident = (&variant.ident).into();
                    ser_data_variants.append_all(
                        quote!(#enum_name::#variant_name { #(#fields_names),* } => { #ser }),
                    );
                }
                Fields::Unnamed(fields_unnamed) => {
                    let mut fields_numbers = vec![];
                    let mut ser = quote!();
                    for (idx, ty) in fields_unnamed.iter().enumerate() {
                        let field_name = Ident::new(format!("_{idx}").as_str(), Span::call_site());
                        fields_numbers.push(field_name.clone());
                        let field_path = FieldPath::Ref(quote!(#field_name));
                        ty.buf_write(field_path, self.no_alloc, &mut ser);
                    }
                    let variant_name: Ident = (&variant.ident).into();
                    ser_data_variants.append_all(
                        quote!(#enum_name::#variant_name ( #(#fields_numbers),* ) => { #ser }),
                    );
                }
                Fields::Unit => continue,
            }
        }

        if ser_data_variants.is_empty() {
            tokens.append_all(quote!(Ok(())));
        } else {
            tokens.append_all(quote! {
                match &self {
                    #ser_data_variants,
                    _ => {}
                }
                Ok(())
            });
        }
    }
}

fn read_discriminant(repr: Repr) -> Ident {
    let read_fn = match repr {
        Repr::U4 => "read_u4",
        Repr::U8 => "read_u8",
        Repr::U16 => "read_u16",
        Repr::Nib16 => "read_nib16",
        Repr::U32 => "read_u32",
    };
    Ident::new(read_fn, Span::call_site())
}

impl<'a> ToTokens for CGEnumDes<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let known_variants = CGEnumVariantsDes {
            item_enum: self.item_enum,
            no_alloc: self.no_alloc,
        };
        // let des = quote! {
        //     Ok(match discriminant {
        //         #known_variants
        //         _ => { return Err(shrink_wrap::Error::EnumFutureVersionOrMalformedData); }
        //     })
        // };
        let read_discriminant = read_discriminant(self.item_enum.repr);
        tokens.append_all(quote! {
            let discriminant = rd.#read_discriminant()?;
            Ok(match discriminant {
                #known_variants
                _ => { return Err(wire_weaver::shrink_wrap::Error::EnumFutureVersionOrMalformedData); }
            })
        });
        // if self.item_enum.is_final {
        //     tokens.append_all(quote! {
        //         let discriminant = rd.read_vlu16n()?;
        //         #des
        //     });
        // } else if !self.item_enum.contains_data_fields() {
        //     tokens.append_all(quote! {
        //         let discriminant = rd.read_vlu16n()?;
        //         // let _future_size = rd.read_vlu16n_rev()?;
        //         #des
        //     });
        // } else {
        //     tokens.append_all(quote! {
        //         {
        //             let discriminant = rd.read_vlu16n()?;
        //             let size = rd.read_vlu16n_rev()? as usize;
        //             let mut rd = rd.split(size)?;
        //             #des
        //         }
        //     });
        // }
    }
}

struct CGEnumVariantsDes<'a> {
    item_enum: &'a ItemEnum,
    no_alloc: bool,
}

impl Variant {
    pub(crate) fn discriminant_lit(&self) -> syn::Lit {
        Lit::Int(LitInt::new(
            format!("{}", self.discriminant).as_str(),
            Span::call_site(),
        ))
    }

    pub fn is_unit(&self) -> bool {
        matches!(self.fields, Fields::Unit)
    }
}

impl Field {
    pub(crate) fn handle_eob(&self) -> TokenStream {
        match &self.default {
            None => quote!(?),
            Some(value) => {
                let value = value.to_lit();
                quote!(.unwrap_or(#value))
            }
        }
    }
}

impl<'a> ToTokens for CGEnumVariantsDes<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for variant in &self.item_enum.variants {
            let discriminant = variant.discriminant_lit();
            let enum_name: Ident = (&self.item_enum.ident).into();
            let variant_name: Ident = (&variant.ident).into();
            match &variant.fields {
                Fields::Named(fields_named) => {
                    let mut field_names = vec![];
                    let mut des_fields = TokenStream::new();
                    for field in fields_named {
                        let field_name: Ident = (&field.ident).into();
                        field_names.push(field_name.clone());
                        let handle_eob = field.handle_eob();
                        // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
                        field
                            .ty
                            .buf_read(field_name, self.no_alloc, handle_eob, &mut des_fields);
                    }
                    tokens.append_all(quote!(#discriminant => { #des_fields #enum_name::#variant_name{ #(#field_names),* } }))
                }
                Fields::Unnamed(fields_unnamed) => {
                    let mut field_names = vec![];
                    let mut des_fields = TokenStream::new();
                    for (idx, ty) in fields_unnamed.iter().enumerate() {
                        let field_name = Ident::new(format!("_{idx}").as_str(), Span::call_site());
                        field_names.push(field_name.clone());
                        let handle_eob = quote! { ? };
                        // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
                        ty.buf_read(field_name, self.no_alloc, handle_eob, &mut des_fields);
                    }
                    tokens.append_all(quote!(#discriminant => { #des_fields #enum_name::#variant_name( #(#field_names),* ) }))
                }
                Fields::Unit => {
                    tokens.append_all(quote!(#discriminant => #enum_name::#variant_name,));
                }
            }
        }
    }
}
