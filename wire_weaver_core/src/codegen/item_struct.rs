use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};

use crate::ast::{Field, ItemStruct, Type};
use crate::codegen::ty::FieldPath;
use crate::codegen::util::{serdes, strings_to_derive};

pub fn struct_def(item_struct: &ItemStruct, no_alloc: bool) -> TokenStream {
    let ident: Ident = (&item_struct.ident).into();
    let fields = CGStructFieldsDef {
        fields: &item_struct.fields,
        no_alloc,
    };
    let lifetime = if no_alloc && item_struct.potential_lifetimes() {
        quote!(<'i>)
    } else {
        quote!()
    };
    let derive = strings_to_derive(&item_struct.derive);
    let docs = item_struct.docs.ts();
    let ts = quote! {
        #docs
        #derive
        pub struct #ident #lifetime { #fields }
    };
    ts
}

struct CGStructFieldsDef<'a> {
    fields: &'a [Field],
    no_alloc: bool,
}

impl<'a> ToTokens for CGStructFieldsDef<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                continue;
            }
            let ident: Ident = (&struct_field.ident).into();
            let ty = struct_field.ty.def(self.no_alloc);
            let docs = struct_field.docs.ts();
            tokens.append_all(quote! {
                #docs
                pub #ident: #ty,
            });
        }
    }
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
    let lifetime = if no_alloc && item_struct.potential_lifetimes() {
        quote!(<'i>)
    } else {
        quote!()
    };
    serdes(struct_name, struct_ser, struct_des, lifetime)
}

impl<'a> ToTokens for CGStructSer<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in &self.item_struct.fields {
            let field_name: Ident = (&struct_field.ident).into();
            let field_path = if matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                FieldPath::Value(quote! {self})
            } else {
                FieldPath::Value(quote! {self.#field_name})
            };
            struct_field
                .ty
                .buf_write(field_path, self.no_alloc, quote! { ? }, tokens);
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
            if !matches!(struct_field.ty, Type::IsOk(_) | Type::IsSome(_)) {
                field_names.push(field_name.clone());
            }
            let handle_eob = struct_field.handle_eob();
            // let x = rd.read_()?; or let x = rd.read_().unwrap_or(default);
            struct_field
                .ty
                .buf_read(field_name, self.no_alloc, handle_eob, tokens);
        }
        let struct_name: Ident = (&self.item_struct.ident).into();
        tokens.append_all(quote! {
            Ok(#struct_name {
                #(#field_names),*
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use crate::ast::ident::Ident;
    use crate::ast::value::Value;
    use crate::ast::{Docs, Field, ItemStruct, Type, Version};
    use crate::codegen::item_struct::struct_serdes;

    fn construct_struct_one() -> ItemStruct {
        ItemStruct {
            docs: Docs::empty(),
            derive: vec![],
            is_final: false,
            ident: Ident::new("X1"),
            fields: vec![
                Field {
                    docs: Docs::empty(),
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: None,
                    default: None,
                },
                Field {
                    docs: Docs::empty(),
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: Some(Version {
                        major: 0,
                        minor: 0,
                        patch: 0,
                    }),
                    default: Some(Value::Bool(true)),
                },
            ],
        }
    }

    fn construct_struct_two() -> ItemStruct {
        ItemStruct {
            docs: Docs::empty(),
            derive: vec![],
            is_final: false,
            ident: Ident::new("X2"),
            fields: vec![
                Field {
                    docs: Docs::empty(),
                    id: 0,
                    ident: Ident::new("a"),
                    ty: Type::Bool,
                    since: None,
                    default: None,
                },
                Field {
                    docs: Docs::empty(),
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
        let cg = struct_serdes(&s, true);
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
        let cg = struct_serdes(&s, true);
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
