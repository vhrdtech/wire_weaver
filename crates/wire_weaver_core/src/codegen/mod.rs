use crate::ast::item::{Item, ItemStruct, StructField};
use crate::ast::File;
use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};

pub fn rust_no_std_file(file: &File) -> TokenStream {
    let mut ts = TokenStream::new();
    for item in &file.items {
        match item {
            Item::Enum(_) => {}
            Item::Struct(item_struct) => {
                ts.append_all(rust_no_std_struct_def(item_struct));
                ts.append_all(rust_no_std_struct_serde(item_struct));
            }
        }
    }
    ts
}

struct CGFieldsDef<'a> {
    fields: &'a [StructField],
}

impl<'a> ToTokens for CGFieldsDef<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            let ident: Ident = (&struct_field.ident).into();
            let ty = struct_field.ty.to_tokens();
            tokens.append_all(quote! {
                pub #ident: #ty,
            });
        }
    }
}

pub fn rust_no_std_struct_def(item_struct: &ItemStruct) -> TokenStream {
    let ident: Ident = (&item_struct.ident).into();
    let fields = CGFieldsDef {
        fields: &item_struct.fields,
    };
    let ts = quote! { pub struct #ident { #fields } };
    ts
}

struct CGFieldsSer<'a> {
    fields: &'a [StructField],
}

pub fn rust_no_std_struct_serde(item_struct: &ItemStruct) -> TokenStream {
    let ident: Ident = (&item_struct.ident).into();
    let fields_ser = CGFieldsSer {
        fields: &item_struct.fields,
    };
    quote! {
        impl #ident {
            pub fn ser_wfdb(&self, wr: &mut wfdb::WfdbBufMut) -> Result<(), wfdb::Error> {
                #fields_ser
            }
        }
    }
}

impl<'a> ToTokens for CGFieldsSer<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for struct_field in self.fields {
            let ident: Ident = (&struct_field.ident).into();
            let ser_fn = struct_field.ty.to_ser_fn_name();
            tokens.append_all(quote! {
                wr.#ser_fn(self.#ident)?;
            });
        }
        tokens.append_all(quote! {
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::ident::Ident;
    use crate::ast::item::{ItemStruct, StructField};
    use crate::ast::ty::Type;
    use crate::ast::value::Value;
    use crate::ast::version::Version;
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
        let cg = super::rust_no_std_struct_serde(&s);
        let correct = quote! {
            impl X1 {
                pub fn ser_wfdb(&self, wr: &mut wfdb::WfdbBufMut) -> Result<(), wfdb::Error> {
                    wr.write_bool(self.a)?;
                    wr.write_bool(self.b)?;
                    Ok(())
                }

                pub fn des_wfdb(rd: &wfdb::WfdbBuf) -> Result<Self, wfdb::Error> {
                    Ok(Self {
                        a: rd.read_bool()?,
                        b: rd.read_bool().unwrap_or(false),
                    })
                }
            }
        };
        assert_eq!(cg.to_string(), correct.to_string());
    }

    #[test]
    fn struct_two_serdes() {
        let s = construct_struct_two();
        let cg = super::rust_no_std_struct_serde(&s);
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
