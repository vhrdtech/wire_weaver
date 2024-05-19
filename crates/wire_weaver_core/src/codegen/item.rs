use crate::ast::data::Variant;
use crate::ast::item::{ItemEnum, ItemStruct, StructField};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{Lit, LitInt};

struct CGStructFieldsDef<'a> {
    fields: &'a [StructField],
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
            fn des_shrink_wrap<'di>(rd: &'di mut shrink_wrap::BufReader<'i>) -> Result<Self, shrink_wrap::Error> {
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
            tokens.append_all(struct_field.ty.buf_write(field_path, self.no_alloc));
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

impl StructField {
    fn handle_eob(&self) -> TokenStream {
        match &self.default {
            None => quote!(?),
            Some(value) => {
                let value = value.to_lit();
                quote!(.unwrap_or(#value))
            }
        }
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
            // let ty = struct_field.ty.ty_def(self.no_alloc);
            let discriminant = variant.discriminant_lit();
            tokens.append_all(quote! {
                #ident = #discriminant,
            });
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
        tokens.append_all(quote! {
            wr.write_vlu16n(self.discriminant())?;
            Ok(())
        });
    }
}

impl<'a> ToTokens for CGEnumDes<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let known_variants = CGEnumVariantsDes {
            item_enum: self.item_enum,
            no_alloc: self.no_alloc,
        };
        let handle_unknown = quote! {
            _ => { return Err(shrink_wrap::Error::EnumFutureVersionOrMalformedData); }
        };
        tokens.append_all(quote! {
            Ok(match rd.read_vlu16n()? {
                #known_variants
                #handle_unknown
            })
        });
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
            let variant: Ident = (&variant.ident).into();
            tokens.append_all(quote! {
                #discriminant => #enum_name::#variant,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::ident::Ident;
    use crate::ast::item::{ItemStruct, StructField};
    use crate::ast::ty::Type;
    use crate::ast::value::Value;
    use crate::ast::version::Version;
    use crate::codegen::item;
    use quote::quote;

    fn construct_struct_one() -> ItemStruct {
        ItemStruct {
            ident: Ident::new("X1"),
            fields: vec![
                StructField {
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: None,
                    default: None,
                },
                StructField {
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
            ident: Ident::new("X2"),
            fields: vec![
                StructField {
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: None,
                    default: None,
                },
                StructField {
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
