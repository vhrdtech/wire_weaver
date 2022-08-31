use semver::VersionReq;
use vhl::ast::ty::TyKind;
use crate::dependencies::{Import, Dependencies, Package, RustCrateSource, Depends};
use crate::prelude::*;
use crate::rust::identifier::CGIdentifier;
use crate::rust::struct_def::CGStructDef;
use crate::rust::ty::CGTy;

pub struct StructSer<'ast> {
    pub inner: CGStructDef<'ast>
}

pub struct StructDes<'ast> {
    pub inner: CGStructDef<'ast>
}

struct StructSerField<'ast> {
    ty: CGTy<'ast>,
}

impl<'ast> ToTokens for StructSerField<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match &self.ty.inner.kind {
            TyKind::Boolean => {
                tokens.append_all(mquote!(rust r#"
                    wr.put_bool
                "#));
            }
            TyKind::Discrete(discrete) => {
                if discrete.is_standard() {
                    let sign = if discrete.is_signed {
                        'i'
                    } else {
                        'u'
                    };
                    let is_le = if discrete.bits == 8 {
                        ""
                    } else {
                        "_le"
                    };
                    let method = format!("put_{}{}{}", sign, discrete.bits, is_le);
                    tokens.append_all(mquote!(rust r#"
                        wr.#method
                    "#));
                } else {
                    // Ix / Ux / UxSpy / UxSny / IxSpy / IxSny, use generic ser<T: SerializeBuf>()
                    tokens.append_all(mquote!(rust r#"
                        wr.ser
                    "#));
                }
            }
        }
    }
}

struct StructDesField<'ast> {
    ty: CGTy<'ast>,
}

impl<'ast> ToTokens for StructDesField<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match &self.ty.inner.kind {
            TyKind::Boolean => {
                tokens.append_all(mquote!(rust r#"
                    get_bool()?
                "#));
            }
            TyKind::Discrete(discrete) => {
                if discrete.is_standard() {
                    let sign = if discrete.is_signed {
                        'i'
                    } else {
                        'u'
                    };
                    let is_le = if discrete.bits == 8 {
                        ""
                    } else {
                        "_le"
                    };
                    let method = format!("get_{}{}{}", sign, discrete.bits, is_le);
                    tokens.append_all(mquote!(rust r#"
                        #method()?
                    "#));
                } else {
                    // Ix / Ux / UxSpy / UxSny / IxSpy / IxSny, use generic des<T: DeserializeBuf>()
                    tokens.append_all(mquote!(rust r#"
                        des()?
                    "#));
                }
            }
        }
    }
}

impl<'ast> ToTokens for StructSer<'ast> {
    fn to_tokens(&self, ts: &mut TokenStream) {
        let field_names = self.inner.inner.fields.fields
            .iter()
            .map(|field| {
                CGIdentifier { inner: &field.name }
            });
        let field_ser_methods = self.inner.inner.fields.fields
            .iter()
            .map(|f| StructSerField {
                ty: CGTy { inner: &f.ty },
            });
        ts.append_all(mquote!(rust r#"
            impl SerializeBytes for #{self.inner.typename} {
                type Error = BufError;

                fn ser_bytes(&self, wr: &mut BufMut) -> Result<(), Self::Error> {
                    #( #field_ser_methods \\( self.#field_names \\) ?; )*
                    Ok(())
                }
            }
        "#));
        ts.recreate_trees();
    }
}

impl<'ast> Depends for StructSer<'ast> {
    fn dependencies(&self) -> Dependencies {
        let depends = vec![Package::RustCrate(
            RustCrateSource::Crates("vhl-stdlib".to_string()),
            VersionReq::parse("0.1.0").unwrap()
        )];
        let uses = vec![
            Import::Submodule(
                "vhl_stdlib", Box::new(Import::Submodule(
                    "serdes", Box::new(Import::Entities(vec![
                        Import::Submodule("traits", Box::new(Import::Entity(
                            "SerializeBytes"
                        ))),

                        Import::Submodule("buf", Box::new(Import::Entities(vec![
                            Import::Entity("BufMut"),
                            Import::EntityAs("Error", "BufError")
                        ])))
                    ])
                    ))
                ))
        ];

        Dependencies {
            depends,
            uses
        }
    }
}

impl<'ast> ToTokens for StructDes<'ast> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let field_names = self.inner.inner.fields.fields
            .iter()
            .map(|field| {
                CGIdentifier { inner: &field.name }
            });
        let field_des_methods = self.inner.inner.fields.fields
            .iter()
            .map(|f| StructDesField {
                ty: CGTy { inner: &f.ty },
            });
        tokens.append_all(mquote!(rust r#"
            impl<'i> DeserializeBytes<'i> for #{self.inner.typename} {
                type Error = BufError;

                fn des_bytes<'di>(rdr: &'di mut Buf<'i>) -> Result<Self, Self::Error> {
                    Ok(#{self.inner.typename} {
                        #( #field_names : #field_des_methods ),*
                    })
                }
            }
        "#));
    }
}

impl<'ast> Depends for StructDes<'ast> {
    fn dependencies(&self) -> Dependencies {
        let depends = vec![Package::RustCrate(
            RustCrateSource::Crates("vhl-stdlib".to_string()),
            VersionReq::parse("0.1.0").unwrap()
        )];
        let uses = vec![
            Import::Submodule(
                "vhl_stdlib", Box::new(Import::Submodule(
                    "serdes", Box::new(Import::Entities(vec![
                        Import::Submodule("traits", Box::new(Import::Entity(
                            "DeserializeBytes"
                        ))),

                        Import::Submodule("buf", Box::new(Import::Entities(vec![
                            Import::Entity("Buf"),
                            Import::EntityAs("Error", "BufError")
                        ])))
                    ])
                    ))
                ))
        ];

        Dependencies {
            depends,
            uses
        }
    }
}

#[cfg(test)]
mod test {
    use mquote::mquote;
    use vhl::ast::file::Definition;
    use vhl::span::{SourceOrigin, SpanOrigin};
    use mtoken::ToTokens;
    use mtoken::ext::TokenStreamExt;
    use crate::prelude::{Span, IdentFlavor, Rc};

    #[test]
    fn struct_ser_buf() {
        let vhl_input = "struct Point { x: u16, y: u16 }";
        let origin = SpanOrigin::Parser(SourceOrigin::Str);
        let ast_parser = parser::ast::file::File::parse(vhl_input).unwrap();
        let ast_core = vhl::ast::file::File::from_parser_ast(ast_parser, origin);
        match &ast_core.items[0] {
            Definition::Struct(struct_def) => {
                let cg_struct_def = super::CGStructDef::new(struct_def);
                let cg_struct_serdes = super::StructSer { inner: cg_struct_def };
                let ts = mquote!(rust r#" #cg_struct_serdes "#);

                let ts_should_be = mquote!(rust r#"
                    impl SerializeBytes for Point {
                        type Error = BufError;
                        fn ser_bytes(&self, wr: &mut BufMut) -> Result<(), Self::Error> {
                            wr.put_u16_le(self.x)?;
                            wr.put_u16_le(self.y)?;
                            Ok(())
                        }
                    }
                "#);

                assert_eq!(format!("{}", ts), format!("{}", ts_should_be));
            }
            _ => panic!("Expected struct definition")
        }
    }

    #[test]
    fn struct_des_buf() {
        let vhl_input = "struct Point { x: u16, y: u16 }";
        let origin = SpanOrigin::Parser(SourceOrigin::Str);
        let ast_parser = parser::ast::file::File::parse(vhl_input).unwrap();
        let ast_core = vhl::ast::file::File::from_parser_ast(ast_parser, origin);
        match &ast_core.items[0] {
            Definition::Struct(struct_def) => {
                let cg_struct_def = super::CGStructDef::new(struct_def);
                let cg_struct_serdes = super::StructDes { inner: cg_struct_def };
                let ts = mquote!(rust r#" #cg_struct_serdes "#);

                let ts_should_be = mquote!(rust r#"
                    impl<'i> DeserializeBytes<'i> for Point {
                        type Error = BufError;
                        fn des_bytes<'di>(rdr: &'di mut Buf<'i>) -> Result<Self, Self::Error> {
                            Ok(Point {
                                x: get_u16_le()?,
                                y: get_u16_le()?
                            })
                        }
                    }
                "#);
                assert_eq!(format!("{}", ts), format!("{}", ts_should_be));
            }
            _ => panic!("Expected struct definition")
        }
    }
}