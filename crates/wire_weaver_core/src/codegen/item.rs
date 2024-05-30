use crate::ast::data::{Field, Fields, Variant};
use crate::ast::item::{ItemEnum, ItemStruct};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{Lit, LitInt};

struct CGStructFieldsDef<'a> {
    fields: &'a [Field],
    no_alloc: bool,
}

impl<'a> ToTokens for CGStructFieldsDef<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            let ident: Ident = (&struct_field.ident).into();
            let ty = struct_field.ty.ty_def(self.no_alloc);
            tokens.append_all(quote! {
                pub #ident: #ty,
            });
        }
    }
}

pub fn struct_def(item_struct: &ItemStruct, no_alloc: bool) -> TokenStream {
    let ident: Ident = (&item_struct.ident).into();
    let fields = CGStructFieldsDef {
        fields: &item_struct.fields,
        no_alloc,
    };
    let lifetime = if no_alloc && item_struct.contains_ref_types() {
        quote!(<'i>)
    } else {
        quote!()
    };
    let ts = quote! {
        #[derive(Debug)]
        pub struct #ident #lifetime { #fields }
    };
    ts
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
    let struct_name: Ident = (&item_struct.ident).into();
    let struct_ser = CGStructSer {
        item_struct,
        no_alloc,
    };
    let struct_des = CGStructDes {
        item_struct,
        no_alloc,
    };
    // let lifetime = if no_alloc && item_struct.contains_ref_types() {
    //     quote!(<'i>)
    // } else {
    //     quote!()
    // };
    serdes(struct_name, struct_ser, struct_des)
}

fn serdes(ty_name: Ident, ser: impl ToTokens, des: impl ToTokens) -> TokenStream {
    let lifetime = quote!();
    quote! {
        impl #lifetime shrink_wrap::SerializeShrinkWrap for #ty_name #lifetime {
            fn ser_shrink_wrap(&self, wr: &mut shrink_wrap::BufWriter) -> Result<(), shrink_wrap::Error> {
                #ser
            }
        }

        impl<'i> shrink_wrap::DeserializeShrinkWrap<'i> for #ty_name #lifetime {
            fn des_shrink_wrap<'di>(rd: &'di mut shrink_wrap::BufReader<'i>, _element_size: shrink_wrap::ElementSize) -> Result<Self, shrink_wrap::Error> {
                #des
            }
        }
    }
}

impl<'a> ToTokens for CGStructSer<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in &self.item_struct.fields {
            let field_name: Ident = (&struct_field.ident).into();
            let field_path = quote!(self.#field_name);
            tokens.append_all(struct_field.ty.buf_write(field_path, false, self.no_alloc));
        }
        tokens.append_all(quote! {
            Ok(())
        });
    }
}

impl<'a> ToTokens for CGStructDes<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut field_names = vec![];
        for struct_field in &self.item_struct.fields {
            let field_name: Ident = (&struct_field.ident).into();
            field_names.push(field_name.clone());
            let handle_eob = struct_field.handle_eob();
            // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
            tokens.append_all(
                struct_field
                    .ty
                    .buf_read(field_name, handle_eob, self.no_alloc),
            );
        }
        let struct_name: Ident = (&self.item_struct.ident).into();
        tokens.append_all(quote! {
            Ok(#struct_name {
                #(#field_names),*
            })
        });
    }
}

pub fn enum_def(item_enum: &ItemEnum, no_alloc: bool) -> TokenStream {
    let enum_name: Ident = (&item_enum.ident).into();
    let variants = CGEnumFieldsDef {
        variants: &item_enum.variants,
        no_alloc,
    };
    let lifetime = if false { quote!(<'i>) } else { quote!() };
    // TODO: respect specified repr
    let ts = quote! {
        #[derive(Debug)]
        #[repr(u16)]
        pub enum #enum_name #lifetime { #variants }

        impl #enum_name {
            pub fn discriminant(&self) -> u16 {
                unsafe { *<*const _>::from(self).cast::<u16>() }
            }
        }
    };
    ts
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
                    let field_names: Vec<Ident> = fields_named
                        .named
                        .iter()
                        .map(|f| (&f.ident).into())
                        .collect();
                    let field_types: Vec<TokenStream> = fields_named
                        .named
                        .iter()
                        .map(|f| f.ty.ty_def(true))
                        .collect();
                    quote!(#ident { #(#field_names: #field_types),* } = #discriminant,)
                }
                Fields::Unnamed(fields_unnamed) => {
                    let field_types: Vec<TokenStream> = fields_unnamed
                        .unnamed
                        .iter()
                        .map(|f| f.ty.ty_def(true))
                        .collect();
                    quote!(#ident ( #(#field_types),* ) = #discriminant,)
                }
                Fields::Unit => quote!(#ident = #discriminant,),
            };
            tokens.append_all(variant);
        }
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
    // let lifetime = if no_alloc && item_struct.contains_ref_types() {
    //     quote!(<'i>)
    // } else {
    //     quote!()
    // };
    serdes(enum_name, enum_ser, enum_des)
}

struct CGEnumSer<'a> {
    item_enum: &'a ItemEnum,
    no_alloc: bool,
}

struct CGEnumDes<'a> {
    item_enum: &'a ItemEnum,
    no_alloc: bool,
}

impl<'a> ToTokens for CGEnumSer<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // TODO: forbid empty enums or not?
        // if self.item_enum.variants.is_empty() {
        //     tokens.append_all(quote!( wr.write_vlu16n(0)?; ));
        // } else {
        tokens.append_all(quote!( wr.write_vlu16n(self.discriminant())?; ));
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
                    for field in &fields_named.named {
                        let field_name: Ident = (&field.ident).into();
                        fields_names.push(field_name.clone());
                        let field_path = quote!(#field_name);
                        ser.append_all(field.ty.buf_write(field_path, true, self.no_alloc));
                    }
                    let variant_name: Ident = (&variant.ident).into();
                    ser_data_variants.append_all(
                        quote!(#enum_name::#variant_name { #(#fields_names),* } => { #ser }),
                    );
                }
                Fields::Unnamed(fields_unnamed) => {
                    let mut fields_numbers = vec![];
                    let mut ser = quote!();
                    for field in &fields_unnamed.unnamed {
                        let field_name: Ident = (&field.ident).into();
                        fields_numbers.push(field_name.clone());
                        let field_path = quote!(#field_name);
                        ser.append_all(field.ty.buf_write(field_path, true, self.no_alloc));
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
        tokens.append_all(quote! {
            let discriminant = rd.read_vlu16n()?;
            Ok(match discriminant {
                #known_variants
                _ => { return Err(shrink_wrap::Error::EnumFutureVersionOrMalformedData); }
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
                    for field in &fields_named.named {
                        let field_name: Ident = (&field.ident).into();
                        field_names.push(field_name.clone());
                        let handle_eob = field.handle_eob();
                        // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
                        des_fields.append_all(field.ty.buf_read(
                            field_name,
                            handle_eob,
                            self.no_alloc,
                        ));
                    }
                    tokens.append_all(quote!(#discriminant => { #des_fields #enum_name::#variant_name{ #(#field_names),* } }))
                }
                Fields::Unnamed(fields_unnamed) => {
                    let mut field_names = vec![];
                    let mut des_fields = TokenStream::new();
                    for field in &fields_unnamed.unnamed {
                        let field_name: Ident = (&field.ident).into();
                        field_names.push(field_name.clone());
                        let handle_eob = field.handle_eob();
                        // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
                        des_fields.append_all(field.ty.buf_read(
                            field_name,
                            handle_eob,
                            self.no_alloc,
                        ));
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

#[cfg(test)]
mod tests {
    use crate::ast::data::Field;
    use crate::ast::ident::Ident;
    use crate::ast::item::ItemStruct;
    use crate::ast::ty::Type;
    use crate::ast::value::Value;
    use crate::ast::version::Version;
    use crate::codegen::item;
    use quote::quote;

    fn construct_struct_one() -> ItemStruct {
        ItemStruct {
            is_final: false,
            ident: Ident::new("X1"),
            fields: vec![
                Field {
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: None,
                    default: None,
                },
                Field {
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: Some(Version::new(1, 1)),
                    default: Some(Value::Bool(true)),
                },
            ],
        }
    }

    fn construct_struct_two() -> ItemStruct {
        ItemStruct {
            is_final: false,
            ident: Ident::new("X2"),
            fields: vec![
                Field {
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: None,
                    default: None,
                },
                Field {
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: None,
                    default: None,
                },
            ],
        }
    }

    #[test]
    fn struct_one_serdes() {
        let s = construct_struct_one();
        let cg = item::struct_serdes(&s, true);
        let correct = quote! {
            impl shrink_wrap::SerializeShrinkWrap for X1 {
                fn ser_shrink_wrap(&self, wr: &mut shrink_wrap::BufWriter) -> Result<(), shrink_wrap::Error> {
                    wr.write_bool(self.a)?;
                    wr.write_bool(self.b)?;
                    Ok(())
                }
            }
            impl<'i> shrink_wrap::DeserializeShrinkWrap<'i> for X1 {
                fn des_shrink_wrap<'di>(rd: &'di mut shrink_wrap::BufReader<'i>) -> Result<Self, shrink_wrap::Error> {
                    let a = rd.read_bool()?;
                    let b = rd.read_bool().unwrap_or(false);
                    Ok(Self {
                        a,
                        b,
                    })
                }
            }
        };
        assert_eq!(cg.to_string(), correct.to_string());
    }

    #[test]
    fn struct_two_serdes() {
        let s = construct_struct_two();
        let cg = item::struct_serdes(&s, true);
        let correct = quote! {
            impl X2 {
                pub fn ser_wfdb(&self, wr: &mut wfdb::WfdbBufMut) -> Result<(), wfdb::Error> {
                    wr.write_bool(self.a)?;
                    wr.write_bool(self.b)?;
                    Ok(())
                }

                pub fn des_wfdb(rd: &wfdb::WfdbBuf) -> Result<Self, wfdb::Error> {
                    Ok(Self {
                        a: rd.read_bool()?,
                        b: rd.read_bool()?
                    })
                }
            }
        };
    }
}
