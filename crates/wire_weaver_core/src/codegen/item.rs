use crate::ast::item::{ItemStruct, StructField};
use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};

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
    let fields_ser = CGStructSer {
        item_struct,
        no_alloc,
    };
    let struct_des = CGStructDes {
        item_struct,
        no_alloc,
    };
    let lifetime = if no_alloc && item_struct.contains_ref_types() {
        quote!(<'i>)
    } else {
        quote!()
    };
    quote! {
        impl #lifetime shrink_wrap::SerializeShrinkWrap for #struct_name #lifetime {
            fn ser_shrink_wrap(&self, wr: &mut shrink_wrap::BufWriter) -> Result<(), shrink_wrap::Error> {
                #fields_ser
            }
        }

        impl<'i> shrink_wrap::DeserializeShrinkWrap<'i> for #struct_name #lifetime {
            fn des_shrink_wrap<'di>(rd: &'di mut shrink_wrap::BufReader<'i>) -> Result<Self, shrink_wrap::Error> {
                #struct_des
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
